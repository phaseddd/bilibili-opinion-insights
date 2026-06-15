use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use gpui::{
    App, AppContext as _, Application, Bounds, ClickEvent, Context, Entity, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, WindowBounds, WindowOptions, div, hsla,
    linear_color_stop, linear_gradient, prelude::FluentBuilder as _, px, relative, rgb, size,
};
use gpui_component::{
    Root,
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputState},
    scroll::ScrollableElement as _,
    v_flex,
};
use qrcode::{Color as QrColor, QrCode};

use crate::app::collection::{
    CollectionJobOutcome, CollectionRequest, CredentialOptions, DEFAULT_COOKIE_PATH,
    DEFAULT_OUTPUT_ROOT, DEFAULT_REQUEST_DELAY, load_cookie_header, run_collection_with_events,
    save_cookie_header,
};
use crate::app::comments::CommentOutputFormat;
use crate::app::events::CollectionEvent;
use crate::bili::auth::{LoginState, QrLoginSession, QrLoginStatus};
use crate::bili::client::BiliClient;
use crate::bili::video::normalize_bvid_input;

const EVENT_LIMIT: usize = 240;
const QR_POLL_INTERVAL: Duration = Duration::from_secs(2);

pub fn run() {
    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(1360.), px(820.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Maximized(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| BiliOpinionGui::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("open GPUI window");
        cx.activate(true);
    });
}

struct BiliOpinionGui {
    app_view: AppView,
    auth: AuthState,
    auth_cancel: Option<Arc<AtomicBool>>,
    form: FormState,
    task: TaskState,
    events: EventState,
    results: ResultState,
    visual: VisualState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppView {
    IdentityGate,
    Workbench,
}

struct FormState {
    bvids_input: Entity<InputState>,
    cookie_input: Entity<InputState>,
    output_input: Entity<InputState>,
    collect_comments: bool,
    collect_danmaku: bool,
    write_csv: bool,
    write_jsonl: bool,
}

#[derive(Default)]
struct AuthState {
    phase: AuthPhase,
    session: SessionMode,
    credential_source: CredentialSource,
    message: Option<String>,
    nav_error: Option<String>,
    qr: Option<QrState>,
    last_checked_at: Option<SystemTime>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SessionMode {
    kind: SessionKind,
    source: Option<CredentialSource>,
    cookie_path: Option<PathBuf>,
    mid: Option<u64>,
    uname: Option<String>,
    vip_status: u64,
    checked_at: Option<SystemTime>,
    completeness_warning: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SessionKind {
    #[default]
    Unknown,
    LoggedIn,
    Anonymous,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CredentialSource {
    #[default]
    None,
    DefaultCookie,
    ExplicitCookie,
    QrLogin,
    Anonymous,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AuthPhase {
    #[default]
    BootChecking,
    LoggedIn,
    AnonymousAvailable,
    CredentialMissing,
    CredentialInvalid,
    CredentialError,
    QrWaitingForScan,
    QrWaitingForConfirm,
    QrExpired,
    QrSuccessChecking,
}

struct QrState {
    status: QrLoginStatus,
    matrix: QrMatrix,
    generated_at: SystemTime,
    last_polled_at: Option<SystemTime>,
    last_status_text: String,
}

struct QrMatrix {
    width: usize,
    modules: Vec<QrColor>,
}

impl QrMatrix {
    fn from_session(session: &QrLoginSession) -> Result<Self> {
        let code = QrCode::new(session.url.as_bytes())?;
        Ok(Self {
            width: code.width(),
            modules: code.to_colors(),
        })
    }

    fn is_dark(&self, x: usize, y: usize) -> bool {
        self.modules[y * self.width + x] == QrColor::Dark
    }
}

impl AuthState {
    fn set_phase(&mut self, phase: AuthPhase, message: impl Into<String>) {
        self.phase = phase;
        self.message = Some(message.into());
        self.nav_error = None;
        self.last_checked_at = Some(SystemTime::now());
    }

    fn set_error(&mut self, phase: AuthPhase, message: impl Into<String>) {
        self.phase = phase;
        self.nav_error = Some(message.into());
        self.message = self.nav_error.clone();
        self.last_checked_at = Some(SystemTime::now());
    }

    fn status_kind(&self) -> EventKind {
        match self.phase {
            AuthPhase::LoggedIn => EventKind::Success,
            AuthPhase::AnonymousAvailable
            | AuthPhase::CredentialMissing
            | AuthPhase::CredentialInvalid
            | AuthPhase::QrExpired => EventKind::Warning,
            AuthPhase::CredentialError => EventKind::Failure,
            AuthPhase::BootChecking
            | AuthPhase::QrWaitingForScan
            | AuthPhase::QrWaitingForConfirm
            | AuthPhase::QrSuccessChecking => EventKind::Danmaku,
        }
    }

    fn is_busy(&self) -> bool {
        matches!(
            self.phase,
            AuthPhase::BootChecking
                | AuthPhase::QrWaitingForScan
                | AuthPhase::QrWaitingForConfirm
                | AuthPhase::QrSuccessChecking
        )
    }

    fn should_show_qr(&self) -> bool {
        matches!(
            self.phase,
            AuthPhase::QrWaitingForScan
                | AuthPhase::QrWaitingForConfirm
                | AuthPhase::QrExpired
                | AuthPhase::QrSuccessChecking
        ) || self.qr.is_some()
    }
}

impl SessionMode {
    fn from_login_state(
        login: LoginState,
        source: CredentialSource,
        cookie_path: Option<PathBuf>,
    ) -> Self {
        let kind = if login.is_login {
            SessionKind::LoggedIn
        } else {
            SessionKind::Unknown
        };

        Self {
            kind,
            source: Some(source),
            cookie_path,
            mid: login.mid,
            uname: login.uname,
            vip_status: login.vip_status,
            checked_at: Some(SystemTime::now()),
            completeness_warning: false,
        }
    }

    fn anonymous() -> Self {
        Self {
            kind: SessionKind::Anonymous,
            source: Some(CredentialSource::Anonymous),
            cookie_path: None,
            mid: None,
            uname: None,
            vip_status: 0,
            checked_at: Some(SystemTime::now()),
            completeness_warning: true,
        }
    }

    fn collection_ready(&self) -> Option<Self> {
        matches!(self.kind, SessionKind::LoggedIn | SessionKind::Anonymous).then(|| self.clone())
    }

    fn title(&self) -> String {
        match self.kind {
            SessionKind::LoggedIn => self
                .uname
                .clone()
                .unwrap_or_else(|| "已登录账号".to_string()),
            SessionKind::Anonymous => "匿名模式".to_string(),
            SessionKind::Unknown => "未确认身份".to_string(),
        }
    }

    fn detail(&self) -> String {
        match self.kind {
            SessionKind::LoggedIn => {
                let mid = self
                    .mid
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "未知".to_string());
                let source = self
                    .source
                    .map(credential_source_label)
                    .unwrap_or("未知来源");
                format!("mid={mid} · VIP={} · {source}", self.vip_status)
            }
            SessionKind::Anonymous => {
                "匿名请求可能导致评论、弹幕或部分接口结果不完整。".to_string()
            }
            SessionKind::Unknown => "尚未通过 nav 登录态校验。".to_string(),
        }
    }
}

impl AuthPhase {
    fn label(self) -> &'static str {
        match self {
            Self::BootChecking => "校验中",
            Self::LoggedIn => "已登录",
            Self::AnonymousAvailable => "匿名模式",
            Self::CredentialMissing => "缺少凭据",
            Self::CredentialInvalid => "凭据失效",
            Self::CredentialError => "校验异常",
            Self::QrWaitingForScan => "等待扫码",
            Self::QrWaitingForConfirm => "等待确认",
            Self::QrExpired => "二维码过期",
            Self::QrSuccessChecking => "保存复核中",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TaskPhase {
    #[default]
    Idle,
    Validating,
    Running,
    Cancelling,
    Completed,
    Failed,
}

#[derive(Default)]
struct TaskState {
    phase: TaskPhase,
    progress: ProgressState,
    active_summary: Option<RunSummary>,
    validation_error: Option<String>,
    session: SessionMode,
}

#[derive(Default)]
struct ProgressState {
    target_percent: f32,
    display_percent: f32,
    total_units: usize,
    completed_units: usize,
    comments_scanned: usize,
    comments_appended: usize,
    danmaku_scanned: usize,
    danmaku_appended: usize,
    danmaku_segments: usize,
    pulses: u64,
}

struct RunSummary {
    videos: Vec<String>,
    output: PathBuf,
    collect_comments: bool,
    collect_danmaku: bool,
}

#[derive(Default)]
struct EventState {
    lines: VecDeque<EventLine>,
    dropped_count: usize,
}

#[derive(Clone)]
struct EventLine {
    kind: EventKind,
    text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EventKind {
    System,
    Video,
    Comments,
    Danmaku,
    Output,
    Warning,
    Success,
    Failure,
}

#[derive(Default)]
struct ResultState {
    jobs: Vec<ResultItem>,
    failures: Vec<FailureItem>,
    output_root: Option<PathBuf>,
}

struct ResultItem {
    kind: ResultKind,
    bvid: String,
    scanned: usize,
    appended: usize,
    extra: String,
    outputs: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResultKind {
    Comments,
    Danmaku,
}

struct FailureItem {
    kind: String,
    bvid: String,
    error: String,
}

#[derive(Default)]
struct VisualState {
    motion_tick: u64,
    motion_loop_running: bool,
    button_rebound: Option<ButtonRebound>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ButtonMotionId {
    EnterWorkbench,
    QrLogin,
    RecheckAuth,
    Anonymous,
    HeaderClear,
    HeaderCancel,
    HeaderStart,
}

#[derive(Clone, Copy, Debug)]
struct ButtonRebound {
    id: ButtonMotionId,
    started_tick: u64,
}

impl VisualState {
    fn button_rebound_amount(&self, id: ButtonMotionId) -> f32 {
        let Some(rebound) = self.button_rebound else {
            return 0.0;
        };
        if rebound.id != id {
            return 0.0;
        }

        let age = self.motion_tick.saturating_sub(rebound.started_tick);
        if age > 18 {
            return 0.0;
        }

        let t = age as f32 / 18.0;
        ((t * TAU * 1.18).sin().abs() * (1.0 - t)).clamp(0.0, 1.0)
    }

    fn has_active_button_rebound(&self) -> bool {
        self.button_rebound
            .map(|rebound| self.motion_tick.saturating_sub(rebound.started_tick) <= 18)
            .unwrap_or(false)
    }
}

#[derive(Debug)]
struct CollectionDraft {
    bvids: Vec<String>,
    session: SessionMode,
    cookie: Option<PathBuf>,
    output: PathBuf,
    collect_comments: bool,
    collect_danmaku: bool,
    write_csv: bool,
    write_jsonl: bool,
}

enum GuiMessage {
    Auth(AuthMessage),
    Event(CollectionEvent),
    Outcome(CollectionJobOutcome),
    Failure(FailureItem),
    UnitFinished,
    Finished { success: bool, message: String },
}

enum AuthMessage {
    BootChecking,
    NavChecked {
        phase: AuthPhase,
        session: SessionMode,
        message: String,
    },
    QrGenerated {
        session: QrLoginSession,
        matrix: QrMatrix,
        message: String,
    },
    QrStatus {
        status: QrLoginStatus,
        message: String,
    },
    QrCookieSaved {
        path: PathBuf,
        message: String,
    },
    QrNavRechecked {
        session: SessionMode,
        message: String,
    },
    AuthError {
        phase: AuthPhase,
        message: String,
    },
}

impl BiliOpinionGui {
    fn cancel_auth_worker(&mut self) {
        if let Some(cancel) = self.auth_cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
    }

    fn new_auth_cancel(&mut self) -> Arc<AtomicBool> {
        self.cancel_auth_worker();
        let cancel = Arc::new(AtomicBool::new(false));
        self.auth_cancel = Some(cancel.clone());
        cancel
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let form = FormState::new(window, cx);
        let mut events = EventState::default();
        events.push(EventKind::System, "启动中：正在校验登录态。");

        let mut this = Self {
            app_view: AppView::IdentityGate,
            form,
            auth: AuthState {
                phase: AuthPhase::BootChecking,
                ..AuthState::default()
            },
            auth_cancel: None,
            task: TaskState::default(),
            events,
            results: ResultState::default(),
            visual: VisualState::default(),
        };
        this.start_auth_bootstrap(cx);
        this
    }

    fn start_auth_bootstrap(&mut self, cx: &mut Context<Self>) {
        let cancel = self.new_auth_cancel();
        let explicit_cookie = trimmed_cookie_input(&self.form.cookie_input, cx).map(PathBuf::from);
        self.auth.qr = None;
        self.auth.session = SessionMode::default();
        self.auth.credential_source = if explicit_cookie.is_some() {
            CredentialSource::ExplicitCookie
        } else {
            CredentialSource::DefaultCookie
        };
        self.auth.set_phase(
            AuthPhase::BootChecking,
            if let Some(path) = explicit_cookie.as_ref() {
                format!("正在读取显式 cookie 并调用 nav 校验：{}", path.display())
            } else {
                "正在读取默认 cookie 并调用 nav 校验登录态。".to_string()
            },
        );
        self.events
            .push(EventKind::System, "身份入口：启动 nav 登录态校验。");

        let (sender, receiver) = mpsc::channel();
        spawn_auth_bootstrap_worker(sender, cancel, explicit_cookie);
        spawn_message_pump(receiver, cx);
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn start_qr_login(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_rebound(ButtonMotionId::QrLogin, cx);
        let cancel = self.new_auth_cancel();
        self.auth.qr = None;
        self.auth.credential_source = CredentialSource::QrLogin;
        self.auth
            .set_phase(AuthPhase::QrWaitingForScan, "正在生成 Bilibili 二维码。");
        self.events
            .push(EventKind::System, "身份入口：请求二维码登录票据。");

        let (sender, receiver) = mpsc::channel();
        spawn_qr_generate_worker(sender, cancel);
        spawn_message_pump(receiver, cx);
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn recheck_auth(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_rebound(ButtonMotionId::RecheckAuth, cx);
        self.start_auth_bootstrap(cx);
    }

    fn continue_anonymous(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_rebound(ButtonMotionId::Anonymous, cx);
        self.cancel_auth_worker();
        self.auth.qr = None;
        self.auth.session = SessionMode::anonymous();
        self.auth.credential_source = CredentialSource::Anonymous;
        self.auth.set_phase(
            AuthPhase::AnonymousAvailable,
            "已明确选择匿名进入；评论、弹幕或部分接口结果可能不完整。",
        );
        self.events.push(
            EventKind::Warning,
            "身份入口：用户选择匿名模式，后续采集会带完整性风险提示。",
        );
        cx.notify();
    }

    fn enter_workbench(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_rebound(ButtonMotionId::EnterWorkbench, cx);
        if self.auth.session.collection_ready().is_none() {
            self.task.validation_error = Some("请先登录，或明确选择匿名进入。".to_string());
            cx.notify();
            return;
        }

        self.app_view = AppView::Workbench;
        self.events.push(
            EventKind::System,
            format!("进入工作台：{}", self.auth.session.detail()),
        );
        cx.notify();
    }

    fn trigger_button_rebound(&mut self, id: ButtonMotionId, cx: &mut Context<Self>) {
        self.visual.button_rebound = Some(ButtonRebound {
            id,
            started_tick: self.visual.motion_tick,
        });
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn ensure_motion_loop(&mut self, cx: &mut Context<Self>) {
        if self.visual.motion_loop_running || !self.should_animate_visuals() {
            return;
        }

        self.visual.motion_loop_running = true;
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(72))
                    .await;

                let keep_running = match view.update(cx, |view, cx| {
                    if view.should_animate_visuals() {
                        view.visual.motion_tick = view.visual.motion_tick.wrapping_add(1);
                        cx.notify();
                        true
                    } else {
                        view.visual.motion_loop_running = false;
                        false
                    }
                }) {
                    Ok(keep_running) => keep_running,
                    Err(_) => return,
                };

                if !keep_running {
                    return;
                }
            }
        })
        .detach();
    }

    fn should_animate_visuals(&self) -> bool {
        let identity_motion = self.app_view == AppView::IdentityGate
            && (self.auth.is_busy()
                || self.auth.should_show_qr()
                || matches!(
                    self.auth.phase,
                    AuthPhase::CredentialMissing
                        | AuthPhase::CredentialInvalid
                        | AuthPhase::CredentialError
                ));
        let task_motion = matches!(
            self.task.phase,
            TaskPhase::Validating | TaskPhase::Running | TaskPhase::Cancelling
        );

        identity_motion || task_motion || self.visual.has_active_button_rebound()
    }

    fn build_draft(&self, cx: &App) -> Result<CollectionDraft, String> {
        let session = self
            .auth
            .session
            .collection_ready()
            .ok_or_else(|| "请先完成身份入口校验，或明确选择匿名进入。".to_string())?;
        let mut bvids = Vec::new();
        for value in self.form.bvids_input.read(cx).value().lines() {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }

            let bvid = normalize_bvid_input(value)
                .ok_or_else(|| format!("无法从输入中识别 BVID：{value}"))?;
            bvids.push(bvid);
        }

        if bvids.is_empty() {
            return Err("至少提供一个 BVID 或视频链接".to_string());
        }
        if !self.form.collect_comments && !self.form.collect_danmaku {
            return Err("至少启用评论或弹幕采集".to_string());
        }
        if self.form.collect_comments && !self.form.write_csv && !self.form.write_jsonl {
            return Err("评论采集至少启用一种输出格式".to_string());
        }

        let cookie = trimmed_cookie_input(&self.form.cookie_input, cx).map(PathBuf::from);
        let output = trimmed_input(&self.form.output_input, cx)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT_ROOT));

        Ok(CollectionDraft {
            bvids,
            session,
            cookie,
            output,
            collect_comments: self.form.collect_comments,
            collect_danmaku: self.form.collect_danmaku,
            write_csv: self.form.write_csv,
            write_jsonl: self.form.write_jsonl,
        })
    }

    fn start_collection(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_rebound(ButtonMotionId::HeaderStart, cx);
        self.task.phase = TaskPhase::Validating;
        self.task.validation_error = None;
        cx.notify();

        match self.build_draft(cx) {
            Ok(draft) => self.start_validated_collection(draft, cx),
            Err(message) => {
                self.task.phase = TaskPhase::Idle;
                self.task.validation_error = Some(message.clone());
                self.events
                    .push(EventKind::Warning, format!("表单校验未通过：{message}"));
                cx.notify();
            }
        }
    }

    fn start_validated_collection(&mut self, draft: CollectionDraft, cx: &mut Context<Self>) {
        let total_units = draft.bvids.len() * usize::from(draft.collect_comments)
            + draft.bvids.len() * usize::from(draft.collect_danmaku);
        let (sender, receiver) = mpsc::channel();

        self.task.phase = TaskPhase::Running;
        self.task.progress = ProgressState {
            total_units,
            ..ProgressState::default()
        };
        self.task.active_summary = Some(RunSummary {
            videos: draft.bvids.clone(),
            output: draft.output.clone(),
            collect_comments: draft.collect_comments,
            collect_danmaku: draft.collect_danmaku,
        });
        self.results = ResultState {
            output_root: Some(draft.output.clone()),
            ..ResultState::default()
        };
        self.events.clear();
        self.events.push(
            EventKind::System,
            format!("已加入队列：{} 个视频", draft.bvids.len()),
        );
        self.events.push(
            EventKind::Video,
            format!("视频：{}", format_bvid_preview(&draft.bvids)),
        );
        self.events.push(
            EventKind::System,
            format!(
                "采集范围：评论={}，弹幕={}",
                yes_no(draft.collect_comments),
                yes_no(draft.collect_danmaku)
            ),
        );
        self.events.push(
            EventKind::Output,
            format!("输出目录：{}", draft.output.display()),
        );
        self.events.push(
            auth_event_kind(&draft.session),
            format!("身份模式：{}", draft.session.detail()),
        );
        self.task.session = draft.session.clone();

        spawn_collection_worker(draft, sender);
        spawn_message_pump(receiver, cx);
        cx.notify();
    }

    fn apply_message(&mut self, message: GuiMessage, cx: &mut Context<Self>) {
        match message {
            GuiMessage::Auth(message) => self.apply_auth_message(message, cx),
            GuiMessage::Event(event) => self.apply_collection_event(event),
            GuiMessage::Outcome(job) => {
                self.results.jobs.push(result_item_from_job(&job));
                self.events
                    .push(EventKind::Success, format_collection_job(&job));
            }
            GuiMessage::Failure(failure) => {
                self.events.push(
                    EventKind::Failure,
                    format!(
                        "{} 采集失败：{}：{}",
                        failure.kind, failure.bvid, failure.error
                    ),
                );
                self.results.failures.push(failure);
            }
            GuiMessage::UnitFinished => self.finish_progress_unit(),
            GuiMessage::Finished { success, message } => {
                self.task.phase = if success {
                    self.task.progress.target_percent = 100.;
                    self.task.progress.display_percent = 100.;
                    TaskPhase::Completed
                } else {
                    TaskPhase::Failed
                };
                self.events.push(
                    if success {
                        EventKind::Success
                    } else {
                        EventKind::Failure
                    },
                    message,
                );
            }
        }
        self.visual.motion_tick = self.visual.motion_tick.wrapping_add(1);
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn apply_auth_message(&mut self, message: AuthMessage, cx: &mut Context<Self>) {
        match message {
            AuthMessage::BootChecking => {
                self.auth.set_phase(
                    AuthPhase::BootChecking,
                    "正在读取默认 cookie 并调用 nav 校验登录态。",
                );
            }
            AuthMessage::NavChecked {
                phase,
                session,
                message,
            } => {
                self.auth.phase = phase;
                self.auth.session = session;
                self.auth.message = Some(message.clone());
                self.auth.nav_error = None;
                self.auth.last_checked_at = Some(SystemTime::now());
                self.events.push(self.auth.status_kind(), message);
            }
            AuthMessage::QrGenerated {
                session,
                matrix,
                message,
            } => {
                self.auth.phase = AuthPhase::QrWaitingForScan;
                self.auth.message = Some(message.clone());
                self.auth.nav_error = None;
                self.auth.last_checked_at = Some(SystemTime::now());
                self.auth.qr = Some(QrState {
                    status: QrLoginStatus::WaitingForScan,
                    matrix,
                    generated_at: SystemTime::now(),
                    last_polled_at: None,
                    last_status_text: "等待扫码".to_string(),
                });
                self.events.push(EventKind::System, message);

                let cancel = self
                    .auth_cancel
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| self.new_auth_cancel());
                let (sender, receiver) = mpsc::channel();
                spawn_qr_poll_worker(session, sender, cancel);
                spawn_message_pump(receiver, cx);
            }
            AuthMessage::QrStatus { status, message } => {
                let phase = match &status {
                    QrLoginStatus::WaitingForScan => AuthPhase::QrWaitingForScan,
                    QrLoginStatus::WaitingForConfirm => AuthPhase::QrWaitingForConfirm,
                    QrLoginStatus::Expired => AuthPhase::QrExpired,
                    QrLoginStatus::Success { .. } => AuthPhase::QrSuccessChecking,
                };
                self.auth.phase = phase;
                self.auth.message = Some(message.clone());
                self.auth.nav_error = None;
                self.auth.last_checked_at = Some(SystemTime::now());
                if let Some(qr) = self.auth.qr.as_mut() {
                    qr.status = status;
                    qr.last_polled_at = Some(SystemTime::now());
                    qr.last_status_text = message.clone();
                }
                self.events.push(self.auth.status_kind(), message);
            }
            AuthMessage::QrCookieSaved { path, message } => {
                self.auth.phase = AuthPhase::QrSuccessChecking;
                self.auth.message = Some(message.clone());
                self.auth.nav_error = None;
                self.auth.last_checked_at = Some(SystemTime::now());
                self.auth.session.cookie_path = Some(path);
                self.events.push(EventKind::Output, message);
            }
            AuthMessage::QrNavRechecked { session, message } => {
                self.auth.qr = None;
                self.auth.session = session;
                self.auth.last_checked_at = Some(SystemTime::now());
                if self.auth.session.kind == SessionKind::LoggedIn {
                    self.auth.phase = AuthPhase::LoggedIn;
                    self.auth.message = Some(message.clone());
                    self.auth.nav_error = None;
                    self.events.push(EventKind::Success, message);
                } else {
                    self.auth.set_error(
                        AuthPhase::CredentialInvalid,
                        "二维码 cookie 已保存，但 nav 未确认登录态，请刷新二维码重试。",
                    );
                    self.events.push(EventKind::Failure, message);
                }
            }
            AuthMessage::AuthError { phase, message } => {
                self.auth.set_error(phase, message.clone());
                self.events.push(EventKind::Failure, message);
            }
        }
    }

    fn apply_collection_event(&mut self, event: CollectionEvent) {
        match &event {
            CollectionEvent::CommentBatchWritten {
                records_scanned,
                records_appended,
                ..
            } => {
                self.task.progress.comments_scanned += records_scanned;
                self.task.progress.comments_appended += records_appended;
                self.bump_progress_for_live_event();
            }
            CollectionEvent::DanmakuSegmentWritten {
                records_scanned,
                records_appended,
                ..
            } => {
                self.task.progress.danmaku_scanned += records_scanned;
                self.task.progress.danmaku_appended += records_appended;
                self.task.progress.danmaku_segments += 1;
                self.bump_progress_for_live_event();
            }
            _ => {}
        }
        self.events
            .push_line(event_line_from_collection_event(&event));
    }

    fn bump_progress_for_live_event(&mut self) {
        self.task.progress.pulses = self.task.progress.pulses.wrapping_add(1);
        if self.task.progress.total_units == 0 {
            self.task.progress.target_percent = (self.task.progress.target_percent + 1.).min(92.);
            self.task.progress.display_percent = ease_towards(
                self.task.progress.display_percent,
                self.task.progress.target_percent,
            );
            return;
        }

        let completed_floor =
            self.task.progress.completed_units as f32 / self.task.progress.total_units as f32;
        let current_unit_ceiling = ((self.task.progress.completed_units + 1) as f32
            / self.task.progress.total_units as f32)
            .min(0.96);
        let nudge = 0.035;
        let target = (completed_floor + nudge).min(current_unit_ceiling) * 100.;
        self.task.progress.target_percent = self.task.progress.target_percent.max(target);
        self.task.progress.display_percent = ease_towards(
            self.task.progress.display_percent,
            self.task.progress.target_percent,
        );
    }

    fn finish_progress_unit(&mut self) {
        self.task.progress.completed_units = self.task.progress.completed_units.saturating_add(1);
        if self.task.progress.total_units > 0 {
            self.task.progress.target_percent = (self.task.progress.completed_units as f32
                / self.task.progress.total_units as f32
                * 100.)
                .min(99.);
            self.task.progress.display_percent = ease_towards(
                self.task.progress.display_percent,
                self.task.progress.target_percent,
            );
        }
    }

    fn set_collect_comments(&mut self, checked: &bool, _: &mut Window, cx: &mut Context<Self>) {
        self.form.collect_comments = *checked;
        cx.notify();
    }

    fn set_collect_danmaku(&mut self, checked: &bool, _: &mut Window, cx: &mut Context<Self>) {
        self.form.collect_danmaku = *checked;
        cx.notify();
    }

    fn set_write_csv(&mut self, checked: &bool, _: &mut Window, cx: &mut Context<Self>) {
        self.form.write_csv = *checked;
        cx.notify();
    }

    fn set_write_jsonl(&mut self, checked: &bool, _: &mut Window, cx: &mut Context<Self>) {
        self.form.write_jsonl = *checked;
        cx.notify();
    }

    fn clear_log(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_rebound(ButtonMotionId::HeaderClear, cx);
        if self.task.phase.is_busy() {
            return;
        }

        self.events.clear();
        self.events.push(EventKind::System, "事件已清空。");
        self.task.progress = ProgressState::default();
        self.results = ResultState::default();
        cx.notify();
    }

    fn request_cancel(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_rebound(ButtonMotionId::HeaderCancel, cx);
        if self.task.phase == TaskPhase::Running {
            self.task.phase = TaskPhase::Cancelling;
            self.events.push(
                EventKind::Warning,
                "取消控制已进入产品化改造队列：当前 worker 仍会等待 collector 自然返回。",
            );
            cx.notify();
        }
    }
}

impl FormState {
    fn new(window: &mut Window, cx: &mut Context<BiliOpinionGui>) -> Self {
        let bvids_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(5)
                .placeholder("每行一个 BVID 或 Bilibili 视频链接")
        });
        let cookie_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(DEFAULT_COOKIE_PATH)
                .default_value(DEFAULT_COOKIE_PATH)
        });
        let output_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(DEFAULT_OUTPUT_ROOT)
                .default_value(DEFAULT_OUTPUT_ROOT)
        });

        Self {
            bvids_input,
            cookie_input,
            output_input,
            collect_comments: true,
            collect_danmaku: true,
            write_csv: true,
            write_jsonl: true,
        }
    }
}

impl TaskPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "待命",
            Self::Validating => "校验中",
            Self::Running => "采集中",
            Self::Cancelling => "取消中",
            Self::Completed => "已完成",
            Self::Failed => "失败",
        }
    }

    fn is_busy(self) -> bool {
        matches!(self, Self::Validating | Self::Running | Self::Cancelling)
    }
}

impl EventState {
    fn clear(&mut self) {
        self.lines.clear();
        self.dropped_count = 0;
    }

    fn push(&mut self, kind: EventKind, text: impl Into<String>) {
        self.push_line(EventLine {
            kind,
            text: text.into(),
        });
    }

    fn push_line(&mut self, line: EventLine) {
        if self.lines.len() >= EVENT_LIMIT {
            self.lines.pop_front();
            self.dropped_count += 1;
        }
        self.lines.push_back(line);
    }
}

fn spawn_message_pump(receiver: mpsc::Receiver<GuiMessage>, cx: &mut Context<BiliOpinionGui>) {
    cx.spawn(async move |view, cx| {
        loop {
            let mut finished = false;
            loop {
                match receiver.try_recv() {
                    Ok(message) => {
                        finished = matches!(message, GuiMessage::Finished { .. });
                        if view
                            .update(cx, |view, cx| view.apply_message(message, cx))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }

            if finished {
                return;
            }

            cx.background_executor()
                .timer(Duration::from_millis(90))
                .await;
        }
    })
    .detach();
}

fn spawn_collection_worker(draft: CollectionDraft, sender: mpsc::Sender<GuiMessage>) {
    std::thread::spawn(move || {
        let result = run_collection_blocking(draft, sender.clone());
        match result {
            Ok(()) => {
                let _ = sender.send(GuiMessage::Finished {
                    success: true,
                    message: "采集完成。".to_string(),
                });
            }
            Err(error) => {
                let _ = sender.send(GuiMessage::Finished {
                    success: false,
                    message: format!("采集失败：{error}"),
                });
            }
        }
    });
}

fn run_collection_blocking(draft: CollectionDraft, sender: mpsc::Sender<GuiMessage>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_collection(draft, sender))
}

fn spawn_auth_bootstrap_worker(
    sender: mpsc::Sender<GuiMessage>,
    cancel: Arc<AtomicBool>,
    explicit_cookie: Option<PathBuf>,
) {
    std::thread::spawn(move || {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let _ = sender.send(GuiMessage::Auth(AuthMessage::BootChecking));
        let message = run_auth_bootstrap_blocking(explicit_cookie).unwrap_or_else(|error| {
            AuthMessage::AuthError {
                phase: AuthPhase::CredentialError,
                message: format!("登录态校验失败：{error}"),
            }
        });
        if !cancel.load(Ordering::SeqCst) {
            let _ = sender.send(GuiMessage::Auth(message));
        }
    });
}

fn spawn_qr_generate_worker(sender: mpsc::Sender<GuiMessage>, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let message = run_qr_generate_blocking().unwrap_or_else(|error| AuthMessage::AuthError {
            phase: AuthPhase::CredentialError,
            message: format!("二维码生成失败：{error}"),
        });
        if !cancel.load(Ordering::SeqCst) {
            let _ = sender.send(GuiMessage::Auth(message));
        }
    });
}

fn spawn_qr_poll_worker(
    session: QrLoginSession,
    sender: mpsc::Sender<GuiMessage>,
    cancel: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let message = run_qr_poll_blocking(session, sender.clone(), cancel.clone())
            .err()
            .map(|error| AuthMessage::AuthError {
                phase: AuthPhase::CredentialError,
                message: format!("二维码登录失败：{error}"),
            });

        if let Some(message) = message
            && !cancel.load(Ordering::SeqCst)
        {
            let _ = sender.send(GuiMessage::Auth(message));
        }
    });
}

fn run_auth_bootstrap_blocking(explicit_cookie: Option<PathBuf>) -> Result<AuthMessage> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(auth_bootstrap(explicit_cookie))
}

fn run_qr_generate_blocking() -> Result<AuthMessage> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(generate_qr_login())
}

fn run_qr_poll_blocking(
    session: QrLoginSession,
    sender: mpsc::Sender<GuiMessage>,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(poll_qr_login(session, sender, cancel))
}

async fn auth_bootstrap(explicit_cookie: Option<PathBuf>) -> Result<AuthMessage> {
    let credentials = CredentialOptions {
        cookie: explicit_cookie.clone(),
        sessdata: None,
        anonymous: false,
    };
    let source = if explicit_cookie.is_some() {
        CredentialSource::ExplicitCookie
    } else {
        CredentialSource::DefaultCookie
    };
    let credential_exists = explicit_cookie
        .as_ref()
        .map(|path| path.exists())
        .unwrap_or_else(|| Path::new(DEFAULT_COOKIE_PATH).exists());
    let cookie_header = match load_cookie_header(&credentials) {
        Ok(cookie_header) => cookie_header,
        Err(error) if credential_exists => {
            return Ok(AuthMessage::AuthError {
                phase: AuthPhase::CredentialInvalid,
                message: format!("cookie 无法作为登录凭据使用：{error}"),
            });
        }
        Err(error) => return Err(error),
    };
    let source = if cookie_header.is_some() {
        source
    } else {
        CredentialSource::None
    };
    let client = BiliClient::new(cookie_header)?;
    let login = client.login_state().await?;
    let mut session = SessionMode::from_login_state(
        login,
        source,
        match source {
            CredentialSource::DefaultCookie => Some(PathBuf::from(DEFAULT_COOKIE_PATH)),
            CredentialSource::ExplicitCookie => explicit_cookie.clone(),
            _ => None,
        },
    );

    let (phase, message) = match (source, session.kind) {
        (CredentialSource::DefaultCookie, SessionKind::LoggedIn) => (
            AuthPhase::LoggedIn,
            format!("nav 已确认登录：{}", session.detail()),
        ),
        (CredentialSource::ExplicitCookie, SessionKind::LoggedIn) => (
            AuthPhase::LoggedIn,
            format!("nav 已确认显式 cookie 登录：{}", session.detail()),
        ),
        (CredentialSource::DefaultCookie, _) => {
            session.completeness_warning = true;
            (
                AuthPhase::CredentialInvalid,
                "默认 cookie 已读取，但 nav 未确认登录态；请扫码登录或匿名进入。".to_string(),
            )
        }
        (CredentialSource::ExplicitCookie, _) => {
            session.completeness_warning = true;
            (
                AuthPhase::CredentialInvalid,
                "显式 cookie 已读取，但 nav 未确认登录态；请更换 cookie、扫码登录或匿名进入。"
                    .to_string(),
            )
        }
        (CredentialSource::None, _) => (
            AuthPhase::CredentialMissing,
            "未发现默认 cookie；已调用 nav 确认为未登录，可扫码登录或匿名进入。".to_string(),
        ),
        _ => (AuthPhase::CredentialError, "登录态来源异常。".to_string()),
    };

    Ok(AuthMessage::NavChecked {
        phase,
        session,
        message,
    })
}

async fn generate_qr_login() -> Result<AuthMessage> {
    let client = BiliClient::new(None)?;
    let session = client.generate_qr_login().await?;
    let matrix = QrMatrix::from_session(&session)?;
    Ok(AuthMessage::QrGenerated {
        session,
        matrix,
        message: "二维码已生成，请使用 Bilibili 客户端扫码。".to_string(),
    })
}

async fn poll_qr_login(
    session: QrLoginSession,
    sender: mpsc::Sender<GuiMessage>,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let client = BiliClient::new(None)?;

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        tokio::time::sleep(QR_POLL_INTERVAL).await;
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        let status = client.poll_qr_login(&session.qrcode_key).await?;

        match status {
            QrLoginStatus::WaitingForScan => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrStatus {
                    status: QrLoginStatus::WaitingForScan,
                    message: "等待扫码；二维码保持有效。".to_string(),
                }));
            }
            QrLoginStatus::WaitingForConfirm => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrStatus {
                    status: QrLoginStatus::WaitingForConfirm,
                    message: "已扫码，等待手机端确认授权。".to_string(),
                }));
            }
            QrLoginStatus::Expired => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrStatus {
                    status: QrLoginStatus::Expired,
                    message: "二维码已过期，请刷新二维码。".to_string(),
                }));
                return Ok(());
            }
            QrLoginStatus::Success { cookie_header } => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let cookie_path = PathBuf::from(DEFAULT_COOKIE_PATH);
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrStatus {
                    status: QrLoginStatus::WaitingForConfirm,
                    message: "扫码授权成功，正在保存 cookie 并复核 nav。".to_string(),
                }));
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                save_cookie_header(&cookie_path, &cookie_header)?;
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrCookieSaved {
                    path: cookie_path.clone(),
                    message: format!("cookie 已保存：{}", cookie_path.display()),
                }));

                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let client = BiliClient::new(Some(cookie_header))?;
                let login = client.login_state().await?;
                let session = SessionMode::from_login_state(
                    login,
                    CredentialSource::QrLogin,
                    Some(cookie_path),
                );
                let message = if session.kind == SessionKind::LoggedIn {
                    format!("保存后 nav 已确认登录：{}", session.detail())
                } else {
                    "保存后 nav 未确认登录态。".to_string()
                };
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrNavRechecked {
                    session,
                    message,
                }));
                return Ok(());
            }
        }
    }
}

async fn run_collection(draft: CollectionDraft, sender: mpsc::Sender<GuiMessage>) -> Result<()> {
    let comment_formats = comment_formats(&draft);
    let credentials = credentials_for_draft(&draft);
    let request = CollectionRequest {
        bvids: draft.bvids,
        credentials,
        output: draft.output,
        collect_comments: draft.collect_comments,
        collect_danmaku: draft.collect_danmaku,
        comment_formats,
        max_comment_pages: None,
        max_reply_pages: None,
        max_danmaku_segments: None,
        request_delay: Some(DEFAULT_REQUEST_DELAY),
    };
    let sender_for_events = sender.clone();
    let outcome = run_collection_with_events(&request, move |event| {
        let _ = sender_for_events.send(GuiMessage::Event(event));
        Ok(())
    })
    .await?;

    for job in &outcome.jobs {
        let _ = sender.send(GuiMessage::Outcome(job.clone()));
        let _ = sender.send(GuiMessage::UnitFinished);
    }

    for failure in &outcome.failures {
        let _ = sender.send(GuiMessage::Failure(FailureItem {
            kind: failure.kind.as_str().to_string(),
            bvid: failure.bvid.clone(),
            error: failure.error.clone(),
        }));
        let _ = sender.send(GuiMessage::UnitFinished);
    }

    outcome.ensure_success()
}

fn credentials_for_draft(draft: &CollectionDraft) -> CredentialOptions {
    if draft.session.kind == SessionKind::Anonymous {
        return CredentialOptions {
            cookie: None,
            sessdata: None,
            anonymous: true,
        };
    }

    let cookie = match draft.session.source {
        Some(CredentialSource::ExplicitCookie) => draft
            .session
            .cookie_path
            .clone()
            .or_else(|| draft.cookie.clone()),
        Some(CredentialSource::DefaultCookie) | Some(CredentialSource::QrLogin) => None,
        _ => draft.cookie.clone(),
    };

    CredentialOptions {
        cookie,
        sessdata: None,
        anonymous: false,
    }
}

fn result_item_from_job(job: &CollectionJobOutcome) -> ResultItem {
    match job {
        CollectionJobOutcome::Comments(outcome) => ResultItem {
            kind: ResultKind::Comments,
            bvid: outcome.bvid.clone(),
            scanned: outcome.summary.comments_scanned,
            appended: outcome.appended_count,
            extra: format!(
                "主评论页 {}，二级页 {}，接口预估 {}",
                outcome.summary.main_pages_scanned,
                outcome.summary.reply_pages_scanned,
                outcome.expected_total
            ),
            outputs: outcome
                .outputs
                .iter()
                .map(|output| output.path.clone())
                .collect(),
        },
        CollectionJobOutcome::Danmaku(outcome) => ResultItem {
            kind: ResultKind::Danmaku,
            bvid: outcome.bvid.clone(),
            scanned: outcome.records_scanned,
            appended: outcome.records_appended,
            extra: format!(
                "分段 {}，新增分段 {}",
                outcome.segments_scanned, outcome.segments_appended
            ),
            outputs: vec![
                outcome.record_path.clone(),
                outcome.segment_metadata_path.clone(),
            ],
        },
    }
}

fn format_collection_job(job: &CollectionJobOutcome) -> String {
    match job {
        CollectionJobOutcome::Comments(outcome) => {
            format!(
                "评论完成：{}，扫描 {}，新增 {}",
                outcome.bvid, outcome.summary.comments_scanned, outcome.appended_count
            )
        }
        CollectionJobOutcome::Danmaku(outcome) => format!(
            "弹幕完成：{}，扫描 {}，新增 {}，分段 {}",
            outcome.bvid,
            outcome.records_scanned,
            outcome.records_appended,
            outcome.segments_scanned
        ),
    }
}

fn comment_formats(draft: &CollectionDraft) -> Vec<CommentOutputFormat> {
    let mut formats = Vec::new();
    if draft.write_csv {
        formats.push(CommentOutputFormat::Csv);
    }
    if draft.write_jsonl {
        formats.push(CommentOutputFormat::Jsonl);
    }
    formats
}

fn event_line_from_collection_event(event: &CollectionEvent) -> EventLine {
    match event {
        CollectionEvent::VideoStarted { bvid } => EventLine {
            kind: EventKind::Video,
            text: format!("开始处理视频：{bvid}"),
        },
        CollectionEvent::OutputInitialized { bvid, path } => EventLine {
            kind: EventKind::Output,
            text: format!("{bvid} 输出已就绪：{}", path.display()),
        },
        CollectionEvent::CommentBatchWritten {
            bvid,
            records_scanned,
            records_appended,
        } => EventLine {
            kind: EventKind::Comments,
            text: format!("{bvid} 评论批次：扫描 {records_scanned}，新增 {records_appended}"),
        },
        CollectionEvent::DanmakuSegmentWritten {
            bvid,
            cid,
            page,
            segment_index,
            records_scanned,
            records_appended,
            segment_appended,
        } => EventLine {
            kind: EventKind::Danmaku,
            text: format!(
                "{bvid} 弹幕分段：cid={cid}，P{page}，段 {segment_index}，扫描 {records_scanned}，新增 {records_appended}，元数据新增 {}",
                yes_no(*segment_appended)
            ),
        },
        CollectionEvent::VideoFinished { bvid } => EventLine {
            kind: EventKind::Success,
            text: format!("视频处理完成：{bvid}"),
        },
    }
}

impl Render for BiliOpinionGui {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::default();
        let can_start = !self.task.phase.is_busy();

        if self.app_view == AppView::IdentityGate {
            return self.render_identity_gate(&palette, cx);
        }

        self.render_workbench(&palette, can_start, cx)
    }
}

impl BiliOpinionGui {
    fn render_workbench(
        &mut self,
        palette: &Palette,
        can_start: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        v_flex()
            .size_full()
            .bg(palette.app_bg)
            .text_color(palette.text)
            .font_family("Microsoft YaHei UI")
            .child(self.render_header(palette, can_start, cx))
            .child(
                h_flex()
                    .size_full()
                    .items_start()
                    .gap(px(16.))
                    .p(px(18.))
                    .child(self.render_collection_panel(palette, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .gap(px(14.))
                            .child(self.render_progress_panel(palette))
                            .child(self.render_event_panel(palette))
                            .child(self.render_result_panel(palette)),
                    ),
            )
    }

    fn render_identity_gate(&self, palette: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let motion_tick = self.visual.motion_tick;
        let can_login = !matches!(
            self.auth.phase,
            AuthPhase::QrWaitingForScan
                | AuthPhase::QrWaitingForConfirm
                | AuthPhase::QrSuccessChecking
                | AuthPhase::BootChecking
        );
        let can_enter = self.auth.session.collection_ready().is_some();
        let login_label = match self.auth.phase {
            AuthPhase::QrExpired => "刷新二维码",
            AuthPhase::QrWaitingForScan | AuthPhase::QrWaitingForConfirm => "轮询中",
            AuthPhase::QrSuccessChecking => "复核中",
            _ => "扫码登录",
        };
        let shell_wave = wave_between(motion_tick, 0.13, 0.08, 0.18);
        let enter_rebound = self
            .visual
            .button_rebound_amount(ButtonMotionId::EnterWorkbench);
        let qr_rebound = self.visual.button_rebound_amount(ButtonMotionId::QrLogin);
        let recheck_rebound = self
            .visual
            .button_rebound_amount(ButtonMotionId::RecheckAuth);
        let anonymous_rebound = self.visual.button_rebound_amount(ButtonMotionId::Anonymous);

        div()
            .size_full()
            .overflow_hidden()
            .bg(linear_gradient(
                135.,
                linear_color_stop(rgb(0xf7fbff), 0.0),
                linear_color_stop(rgb(0xf1fffd), 1.0),
            ))
            .text_color(palette.text)
            .font_family("Microsoft YaHei UI")
            .flex()
            .items_center()
            .justify_center()
            .p(px(30.))
            .child(
                v_flex()
                    .size_full()
                    .w(relative(0.96))
                    .max_w(px(1560.))
                    .min_w(px(0.))
                    .gap(px(20.))
                    .child(
                        div()
                            .relative()
                            .overflow_hidden()
                            .rounded(px(30.))
                            .border_1()
                            .border_color(palette.accent.opacity(0.18 + shell_wave * 0.18))
                            .bg(hsla(0., 0., 1., 0.68))
                            .shadow(vec![gpui::BoxShadow {
                                color: palette.accent.opacity(0.12 + shell_wave * 0.08),
                                offset: gpui::point(px(0.), px(18.)),
                                blur_radius: px(38.),
                                spread_radius: px(-22.),
                            }])
                            .child(
                                div()
                                    .absolute()
                                    .left(px(24. + shell_wave * 36.))
                                    .right(px(340.))
                                    .top(px(8.))
                                    .h(px(9.))
                                    .rounded(px(999.))
                                    .bg(hsla(0., 0., 1., 0.26 + shell_wave * 0.14)),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(18.))
                                    .p(px(18.))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap(px(14.))
                                            .min_w(px(0.))
                                            .child(product_mark(palette))
                                            .child(
                                                v_flex()
                                                    .gap(px(5.))
                                                    .min_w(px(0.))
                                                    .child(
                                                        div()
                                                            .text_size(px(24.))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .child("Bilibili 舆情采集"),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(palette.muted)
                                                            .child(
                                                                "身份入口会先真实调用 nav 校验登录态，再决定登录或匿名进入。",
                                                            ),
                                                    ),
                                            ),
                                    )
                                    .child(status_badge(
                                        self.auth.phase.label(),
                                        self.auth.status_kind(),
                                        palette,
                                    )),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .w_full()
                            .items_start()
                            .gap(px(22.))
                            .child(
                                glass_auth_panel(palette)
                                    .flex_1()
                                    .min_w(px(0.))
                                    .h_full()
                                    .gap(px(18.))
                                    .child(auth_summary_block(&self.auth, palette))
                                    .child(auth_lifecycle_block(&self.auth, palette, motion_tick))
                                    .child(auth_risk_block(&self.auth.session, palette))
                                    .child(
                                        v_flex()
                                            .gap(px(10.))
                                            .mt(px(2.))
                                            .child(
                                                h_flex()
                                                    .gap(px(10.))
                                                    .child(
                                                        jelly_action_button(
                                                            if can_enter {
                                                                "进入工作台"
                                                            } else {
                                                                "等待身份确认"
                                                            },
                                                            palette,
                                                            JellyButtonConfig {
                                                                tone: JellyActionTone::Primary,
                                                                enabled: can_enter,
                                                                loading: false,
                                                                motion_tick,
                                                                group: "auth-enter-workbench-group",
                                                                motion_id:
                                                                    ButtonMotionId::EnterWorkbench,
                                                                size: JellyButtonSize::Standard,
                                                                rebound: enter_rebound,
                                                            },
                                                        )
                                                        .flex_1()
                                                        .id("auth-enter-workbench")
                                                        .focusable()
                                                        .focus(|this| {
                                                            this.border_color(
                                                                palette.accent.opacity(0.72),
                                                            )
                                                        })
                                                        .active(|this| {
                                                            this.opacity(0.92).border_color(
                                                                palette.accent.opacity(0.78),
                                                            )
                                                        })
                                                        .when(can_enter, |this| {
                                                            this.cursor_pointer().on_click(
                                                                cx.listener(Self::enter_workbench),
                                                            )
                                                        }),
                                                    )
                                                    .child(
                                                        jelly_action_button(
                                                            login_label,
                                                            palette,
                                                            JellyButtonConfig {
                                                                tone: JellyActionTone::Cyan,
                                                                enabled: can_login,
                                                                loading: self.auth.should_show_qr()
                                                                    || matches!(
                                                                        self.auth.phase,
                                                                        AuthPhase::BootChecking
                                                                    ),
                                                                motion_tick,
                                                                group: "auth-start-qr-login-group",
                                                                motion_id: ButtonMotionId::QrLogin,
                                                                size: JellyButtonSize::Standard,
                                                                rebound: qr_rebound,
                                                            },
                                                        )
                                                        .flex_1()
                                                        .id("auth-start-qr-login")
                                                        .focusable()
                                                        .focus(|this| {
                                                            this.border_color(
                                                                palette.accent.opacity(0.72),
                                                            )
                                                        })
                                                        .active(|this| {
                                                            this.opacity(0.92).border_color(
                                                                palette.accent.opacity(0.78),
                                                            )
                                                        })
                                                        .when(can_login, |this| {
                                                            this.cursor_pointer().on_click(
                                                                cx.listener(Self::start_qr_login),
                                                            )
                                                        }),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap(px(10.))
                                                    .child(
                                                        jelly_action_button(
                                                            "重新校验",
                                                            palette,
                                                            JellyButtonConfig {
                                                                tone: JellyActionTone::Neutral,
                                                                enabled: !self.auth.is_busy(),
                                                                loading: self.auth.phase
                                                                    == AuthPhase::BootChecking,
                                                                motion_tick,
                                                                group: "auth-recheck-group",
                                                                motion_id:
                                                                    ButtonMotionId::RecheckAuth,
                                                                size: JellyButtonSize::Standard,
                                                                rebound: recheck_rebound,
                                                            },
                                                        )
                                                        .flex_1()
                                                        .id("auth-recheck")
                                                        .focusable()
                                                        .focus(|this| {
                                                            this.border_color(
                                                                palette.accent.opacity(0.52),
                                                            )
                                                        })
                                                        .active(|this| {
                                                            this.opacity(0.9).border_color(
                                                                palette.accent.opacity(0.6),
                                                            )
                                                        })
                                                        .when(!self.auth.is_busy(), |this| {
                                                            this.cursor_pointer().on_click(
                                                                cx.listener(Self::recheck_auth),
                                                            )
                                                        }),
                                                    )
                                                    .child(
                                                        jelly_action_button(
                                                            "匿名进入",
                                                            palette,
                                                            JellyButtonConfig {
                                                                tone: JellyActionTone::Warning,
                                                                enabled: !matches!(
                                                                    self.auth.phase,
                                                                    AuthPhase::BootChecking
                                                                ),
                                                                loading: false,
                                                                motion_tick,
                                                                group: "auth-anonymous-group",
                                                                motion_id: ButtonMotionId::Anonymous,
                                                                size: JellyButtonSize::Standard,
                                                                rebound: anonymous_rebound,
                                                            },
                                                        )
                                                        .flex_1()
                                                        .id("auth-anonymous")
                                                        .focusable()
                                                        .focus(|this| {
                                                            this.border_color(
                                                                palette.warning.opacity(0.68),
                                                            )
                                                        })
                                                        .active(|this| {
                                                            this.opacity(0.9).border_color(
                                                                palette.warning.opacity(0.74),
                                                            )
                                                        })
                                                        .when(
                                                            !matches!(
                                                                self.auth.phase,
                                                                AuthPhase::BootChecking
                                                            ),
                                                            |this| {
                                                                this.cursor_pointer().on_click(
                                                                    cx.listener(
                                                                        Self::continue_anonymous,
                                                                    ),
                                                                )
                                                            },
                                                        ),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                glass_auth_panel(palette)
                                    .w(relative(0.34))
                                    .min_w(px(410.))
                                    .max_w(px(520.))
                                    .flex_shrink_0()
                                    .h_full()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(16.))
                                    .child(qr_stage(&self.auth, palette, motion_tick))
                                    .child(qr_lifecycle_card(&self.auth, palette, motion_tick))
                                    .child(
                                        div()
                                            .w_full()
                                            .text_size(px(12.))
                                            .text_color(palette.muted)
                                            .line_height(relative(1.4))
                                            .child(SharedString::from(qr_helper_copy(&self.auth))),
                                    ),
                            ),
                    ),
            )
    }

    fn render_header(
        &self,
        palette: &Palette,
        can_start: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .px(px(24.))
            .py(px(16.))
            .border_b_1()
            .border_color(palette.border)
            .bg(palette.surface)
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(12.))
                    .items_center()
                    .child(product_mark(palette))
                    .child(
                        v_flex()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Bilibili 舆情采集"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(palette.muted)
                                    .child("Rust-native GPUI 桌面端"),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap(px(8.))
                    .child(session_capsule(&self.auth, palette))
                    .child(
                        header_action_button(
                            "清空",
                            palette,
                            HeaderButtonConfig {
                                kind: HeaderActionKind::Ghost,
                                enabled: can_start,
                                motion_tick: self.visual.motion_tick,
                                group: "header-clear-group",
                                motion_id: ButtonMotionId::HeaderClear,
                                rebound: self
                                    .visual
                                    .button_rebound_amount(ButtonMotionId::HeaderClear),
                            },
                        )
                        .id("clear-log-action")
                        .active(|this| this.opacity(0.94))
                        .when(can_start, |this| {
                            this.cursor_pointer().on_click(cx.listener(Self::clear_log))
                        }),
                    )
                    .child(
                        header_action_button(
                            "取消",
                            palette,
                            HeaderButtonConfig {
                                kind: HeaderActionKind::Outline,
                                enabled: self.task.phase == TaskPhase::Running,
                                motion_tick: self.visual.motion_tick,
                                group: "header-cancel-group",
                                motion_id: ButtonMotionId::HeaderCancel,
                                rebound: self
                                    .visual
                                    .button_rebound_amount(ButtonMotionId::HeaderCancel),
                            },
                        )
                        .id("cancel-collection-action")
                        .active(|this| this.opacity(0.94))
                        .when(
                            self.task.phase == TaskPhase::Running,
                            |this| {
                                this.cursor_pointer()
                                    .on_click(cx.listener(Self::request_cancel))
                            },
                        ),
                    )
                    .child(
                        header_action_button(
                            if self.task.phase.is_busy() {
                                "运行中"
                            } else {
                                "开始采集"
                            },
                            palette,
                            HeaderButtonConfig {
                                kind: HeaderActionKind::Primary,
                                enabled: can_start,
                                motion_tick: self.visual.motion_tick,
                                group: "header-start-group",
                                motion_id: ButtonMotionId::HeaderStart,
                                rebound: self
                                    .visual
                                    .button_rebound_amount(ButtonMotionId::HeaderStart),
                            },
                        )
                        .id("start-collection-action")
                        .active(|this| this.opacity(0.94))
                        .when(can_start, |this| {
                            this.cursor_pointer()
                                .on_click(cx.listener(Self::start_collection))
                        }),
                    ),
            )
    }

    fn render_collection_panel(
        &self,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        panel(palette)
            .w(relative(0.3))
            .min_w(px(360.))
            .max_w(px(480.))
            .flex_shrink_0()
            .h_full()
            .gap(px(16.))
            .overflow_y_scrollbar()
            .child(panel_title("采集设置", "输入视频、凭据和输出选项", palette))
            .child(form_section(
                "视频列表",
                "支持 BVID 或完整视频链接，每行一个。",
                Input::new(&self.form.bvids_input).h(px(138.)),
                palette,
            ))
            .child(form_section(
                "Cookie 文件",
                "工作台默认继承身份入口状态；这里仍可填写显式 cookie 文件供采集使用。",
                Input::new(&self.form.cookie_input).cleanable(true),
                palette,
            ))
            .child(form_section(
                "输出目录",
                "当前写入本地目录，复跑会跳过已存在记录。",
                Input::new(&self.form.output_input).cleanable(true),
                palette,
            ))
            .child(self.render_auth_strip(palette))
            .child(option_group(
                "采集内容",
                h_flex()
                    .gap(px(14.))
                    .child(
                        Checkbox::new("collect-comments")
                            .label("评论")
                            .checked(self.form.collect_comments)
                            .on_click(cx.listener(Self::set_collect_comments)),
                    )
                    .child(
                        Checkbox::new("collect-danmaku")
                            .label("弹幕")
                            .checked(self.form.collect_danmaku)
                            .on_click(cx.listener(Self::set_collect_danmaku)),
                    ),
                palette,
            ))
            .child(option_group(
                "评论输出",
                h_flex()
                    .gap(px(14.))
                    .child(
                        Checkbox::new("write-csv")
                            .label("CSV")
                            .checked(self.form.write_csv)
                            .on_click(cx.listener(Self::set_write_csv)),
                    )
                    .child(
                        Checkbox::new("write-jsonl")
                            .label("JSONL")
                            .checked(self.form.write_jsonl)
                            .on_click(cx.listener(Self::set_write_jsonl)),
                    ),
                palette,
            ))
            .when_some(self.task.validation_error.clone(), |this, message| {
                this.child(validation_box(&message, palette))
            })
    }

    fn render_auth_strip(&self, palette: &Palette) -> impl IntoElement {
        let message = self
            .auth
            .message
            .clone()
            .unwrap_or_else(|| "等待身份状态。".to_string());
        let risk = self.auth.session.completeness_warning;

        v_flex()
            .w_full()
            .gap(px(8.))
            .p(px(12.))
            .rounded(px(10.))
            .border_1()
            .border_color(palette.border)
            .bg(palette.surface_soft)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(10.))
                            .child(status_dot(palette.accent))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("登录态"),
                            ),
                    )
                    .child(status_badge(
                        self.auth.phase.label(),
                        self.auth.status_kind(),
                        palette,
                    )),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .line_height(relative(1.25))
                    .child(self.auth.session.detail()),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(if risk { palette.warning } else { palette.muted })
                    .line_height(relative(1.25))
                    .child(SharedString::from(message)),
            )
    }

    fn render_progress_panel(&self, palette: &Palette) -> impl IntoElement {
        panel(palette)
            .gap(px(14.))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(panel_title(
                        "任务进度",
                        "以真实 collector 事件推进",
                        palette,
                    ))
                    .child(status_badge(
                        self.task.phase.label(),
                        phase_kind(self.task.phase),
                        palette,
                    )),
            )
            .child(jelly_progress(
                self.task.progress.display_percent,
                self.task.phase,
                self.visual.motion_tick,
                palette,
            ))
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(h_flex().gap(px(10.)).children([
                        metric_chip(
                            "评论扫描",
                            self.task.progress.comments_scanned,
                            palette.accent_2,
                            palette,
                        ),
                        metric_chip(
                            "评论新增",
                            self.task.progress.comments_appended,
                            palette.success,
                            palette,
                        ),
                    ]))
                    .child(h_flex().gap(px(10.)).children([
                        metric_chip(
                            "弹幕扫描",
                            self.task.progress.danmaku_scanned,
                            palette.accent,
                            palette,
                        ),
                        metric_chip(
                            "分段",
                            self.task.progress.danmaku_segments,
                            palette.warning,
                            palette,
                        ),
                    ])),
            )
            .child(self.render_run_summary(palette))
    }

    fn render_run_summary(&self, palette: &Palette) -> impl IntoElement {
        let Some(summary) = self.task.active_summary.as_ref() else {
            return div()
                .text_size(px(12.))
                .text_color(palette.muted)
                .child("等待新的采集任务。");
        };

        h_flex()
            .w_full()
            .gap(px(8.))
            .items_center()
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(palette.muted)
                    .child(format!(
                        "{} 个视频 · {} · 输出 {}",
                        summary.videos.len(),
                        collect_scope_label(summary.collect_comments, summary.collect_danmaku),
                        summary.output.display()
                    )),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(format!(
                        "{}/{} 单元",
                        self.task.progress.completed_units, self.task.progress.total_units
                    )),
            )
    }

    fn render_event_panel(&self, palette: &Palette) -> impl IntoElement {
        let dropped = self.events.dropped_count;
        panel(palette)
            .flex_1()
            .min_h(px(245.))
            .gap(px(10.))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(panel_title("事件流", "有边界、可滚动、按类型标记", palette))
                    .child(div().text_size(px(11.)).text_color(palette.muted).child(
                        if dropped == 0 {
                            format!("保留最近 {} 条", EVENT_LIMIT)
                        } else {
                            format!("已裁剪 {dropped} 条")
                        },
                    )),
            )
            .child(
                v_flex()
                    .id("gui-event-scroll")
                    .flex_1()
                    .min_h(px(190.))
                    .max_h(px(290.))
                    .gap(px(7.))
                    .p(px(10.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.event_bg)
                    .children(
                        self.events
                            .lines
                            .iter()
                            .rev()
                            .map(|line| event_row(line, palette)),
                    )
                    .overflow_y_scrollbar(),
            )
    }

    fn render_result_panel(&self, palette: &Palette) -> impl IntoElement {
        panel(palette)
            .gap(px(12.))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(panel_title(
                        "结果查看",
                        "采集后汇总输出文件与失败项",
                        palette,
                    ))
                    .child(status_badge(
                        if self.results.jobs.is_empty() {
                            "等待结果"
                        } else {
                            "可复核"
                        },
                        if self.results.failures.is_empty() {
                            EventKind::Output
                        } else {
                            EventKind::Failure
                        },
                        palette,
                    )),
            )
            .child(
                v_flex()
                    .gap(px(8.))
                    .children(
                        self.results
                            .jobs
                            .iter()
                            .map(|item| result_row(item, palette)),
                    )
                    .when(self.results.jobs.is_empty(), |this| {
                        this.child(empty_result_state(palette))
                    }),
            )
            .when(!self.results.failures.is_empty(), |this| {
                this.child(
                    v_flex().gap(px(6.)).children(
                        self.results
                            .failures
                            .iter()
                            .map(|failure| failure_row(failure, palette)),
                    ),
                )
            })
            .when_some(self.results.output_root.clone(), |this, output| {
                this.child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(palette.muted)
                        .child(format!("输出根目录：{}", output.display())),
                )
            })
    }
}

#[derive(Clone, Copy)]
struct Palette {
    app_bg: Hsla,
    surface: Hsla,
    surface_soft: Hsla,
    event_bg: Hsla,
    border: Hsla,
    text: Hsla,
    muted: Hsla,
    accent: Hsla,
    accent_2: Hsla,
    success: Hsla,
    warning: Hsla,
    error: Hsla,
}

#[derive(Clone, Copy)]
enum HeaderActionKind {
    Ghost,
    Outline,
    Primary,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            app_bg: rgb(0xf6f8fb).into(),
            surface: rgb(0xffffff).into(),
            surface_soft: rgb(0xf8fbff).into(),
            event_bg: rgb(0xfbfcfe).into(),
            border: rgb(0xd9e2ec).into(),
            text: rgb(0x172033).into(),
            muted: rgb(0x667085).into(),
            accent: rgb(0x15c8d8).into(),
            accent_2: rgb(0xfb7299).into(),
            success: rgb(0x18a66a).into(),
            warning: rgb(0xc47a10).into(),
            error: rgb(0xd92d20).into(),
        }
    }
}

#[derive(Clone, Copy)]
struct HeaderButtonConfig {
    kind: HeaderActionKind,
    enabled: bool,
    motion_tick: u64,
    group: &'static str,
    motion_id: ButtonMotionId,
    rebound: f32,
}

fn header_action_button(
    label: &'static str,
    palette: &Palette,
    config: HeaderButtonConfig,
) -> gpui::Div {
    let tone = match config.kind {
        HeaderActionKind::Ghost => JellyActionTone::Neutral,
        HeaderActionKind::Outline => JellyActionTone::Warning,
        HeaderActionKind::Primary => JellyActionTone::Primary,
    };

    jelly_action_button(
        label,
        palette,
        JellyButtonConfig {
            tone,
            enabled: config.enabled,
            loading: false,
            motion_tick: config.motion_tick,
            group: config.group,
            motion_id: config.motion_id,
            size: JellyButtonSize::Compact,
            rebound: config.rebound,
        },
    )
}

#[derive(Clone, Copy)]
enum JellyActionTone {
    Primary,
    Cyan,
    Warning,
    Neutral,
}

#[derive(Clone, Copy)]
enum JellyButtonSize {
    Standard,
    Compact,
}

#[derive(Clone, Copy)]
struct JellyButtonMetrics {
    height: f32,
    min_width: f32,
    outer_pad_x: f32,
    text_size: f32,
    inner_x: f32,
    inner_y: f32,
    highlight_h: f32,
    shell_top: f32,
    shell_bottom: f32,
}

impl JellyButtonSize {
    fn metrics(self) -> JellyButtonMetrics {
        match self {
            Self::Standard => JellyButtonMetrics {
                height: 52.,
                min_width: 140.,
                outer_pad_x: 18.,
                text_size: 12.,
                inner_x: 8.,
                inner_y: 7.,
                highlight_h: 8.,
                shell_top: 2.,
                shell_bottom: 4.,
            },
            Self::Compact => JellyButtonMetrics {
                height: 40.,
                min_width: 88.,
                outer_pad_x: 12.,
                text_size: 11.,
                inner_x: 6.,
                inner_y: 5.,
                highlight_h: 5.5,
                shell_top: 1.5,
                shell_bottom: 3.,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct JellyButtonMaterial {
    start: Hsla,
    end: Hsla,
    rim: Hsla,
    inner_top: Hsla,
    inner_bottom: Hsla,
    text: Hsla,
    aura: Hsla,
}

#[derive(Clone, Copy)]
struct JellyButtonMotion {
    shell_top: f32,
    shell_bottom: f32,
    shell_bleed_x: f32,
    inner_top: f32,
    inner_bottom: f32,
    gloss_top: f32,
    gloss_inset: f32,
    ridge_bottom: f32,
    ridge_height: f32,
    contact_y: f32,
    contact_blur: f32,
    breath: f32,
    pop: f32,
}

#[derive(Clone, Copy)]
struct JellyButtonConfig {
    tone: JellyActionTone,
    enabled: bool,
    loading: bool,
    motion_tick: u64,
    group: &'static str,
    motion_id: ButtonMotionId,
    size: JellyButtonSize,
    rebound: f32,
}

fn jelly_button_material(tone: JellyActionTone, palette: &Palette) -> JellyButtonMaterial {
    match tone {
        JellyActionTone::Primary => JellyButtonMaterial {
            start: palette.accent_2,
            end: palette.accent,
            rim: hsla(0., 0., 1., 0.64),
            inner_top: hsla(0., 0., 1., 0.95),
            inner_bottom: hsla(190., 0.82, 0.92, 0.78),
            text: rgb(0x0b6176).into(),
            aura: palette.accent_2,
        },
        JellyActionTone::Cyan => JellyButtonMaterial {
            start: palette.accent,
            end: rgb(0x77e6f1).into(),
            rim: hsla(0., 0., 1., 0.66),
            inner_top: hsla(0., 0., 1., 0.94),
            inner_bottom: hsla(188., 0.86, 0.91, 0.76),
            text: rgb(0x075c67).into(),
            aura: palette.accent,
        },
        JellyActionTone::Warning => JellyButtonMaterial {
            start: palette.warning,
            end: rgb(0xffbd78).into(),
            rim: hsla(0., 0., 1., 0.58),
            inner_top: hsla(0., 0., 1., 0.92),
            inner_bottom: hsla(35., 0.92, 0.88, 0.76),
            text: rgb(0x884900).into(),
            aura: palette.warning,
        },
        JellyActionTone::Neutral => JellyButtonMaterial {
            start: rgb(0xcaf7ff).into(),
            end: rgb(0xffe1ed).into(),
            rim: palette.accent.opacity(0.48),
            inner_top: hsla(0., 0., 1., 0.92),
            inner_bottom: hsla(332., 0.75, 0.95, 0.7),
            text: rgb(0x233348).into(),
            aura: palette.accent,
        },
    }
}

fn jelly_button_motion(
    metrics: JellyButtonMetrics,
    config: JellyButtonConfig,
) -> JellyButtonMotion {
    let pop = config.rebound.clamp(0.0, 1.0);
    let breath = if config.loading {
        wave_between(config.motion_tick, 0.18, 0.08, 0.2)
    } else {
        0.0
    };
    let size_factor = if matches!(config.size, JellyButtonSize::Standard) {
        1.0
    } else {
        0.68
    };

    JellyButtonMotion {
        shell_top: metrics.shell_top - pop * 2.4 * size_factor,
        shell_bottom: metrics.shell_bottom - pop * 1.3 * size_factor,
        shell_bleed_x: pop * 4.2 * size_factor,
        inner_top: metrics.inner_y - pop * 1.2 * size_factor,
        inner_bottom: metrics.inner_y + pop * 1.7 * size_factor,
        gloss_top: 5. + pop * 1.6 * size_factor,
        gloss_inset: 22. - pop * 3. * size_factor,
        ridge_bottom: 7. - pop * 1.2 * size_factor,
        ridge_height: if matches!(config.size, JellyButtonSize::Standard) {
            6. + pop * 1.6
        } else {
            4. + pop
        },
        contact_y: 13. - pop * 3.2 * size_factor,
        contact_blur: 26. + pop * 8. * size_factor,
        breath,
        pop,
    }
}

fn jelly_action_button(
    label: impl Into<String>,
    palette: &Palette,
    config: JellyButtonConfig,
) -> gpui::Div {
    let label = label.into();
    let material = jelly_button_material(config.tone, palette);
    let opacity = if config.enabled { 1.0 } else { 0.46 };
    let metrics = config.size.metrics();
    let motion = jelly_button_motion(metrics, config);
    let group_name = SharedString::from(config.group);
    let id_seed = config.motion_id as usize;

    div()
        .relative()
        .group(group_name.clone())
        .flex_shrink_0()
        .h(px(metrics.height))
        .min_w(px(metrics.min_width))
        .px(px(metrics.outer_pad_x))
        .rounded(px(999.))
        .child(
            div()
                .id(("jelly-button-shell", id_seed))
                .absolute()
                .top(px(motion.shell_top))
                .bottom(px(motion.shell_bottom))
                .left(px(-motion.shell_bleed_x))
                .right(px(-motion.shell_bleed_x))
                .rounded(px(999.))
                .overflow_hidden()
                .border_1()
                .border_color(material.rim.opacity((0.78 + motion.pop * 0.2) * opacity))
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(material.start.opacity(opacity), 0.0),
                    linear_color_stop(material.end.opacity(opacity), 1.0),
                ))
                .shadow(vec![
                    gpui::BoxShadow {
                        color: material.aura.opacity((0.24 + motion.pop * 0.14) * opacity),
                        offset: gpui::point(px(0.), px(motion.contact_y)),
                        blur_radius: px(motion.contact_blur),
                        spread_radius: px(-12.),
                    },
                    gpui::BoxShadow {
                        color: material.end.opacity(0.12 * opacity),
                        offset: gpui::point(px(0.), px(5.)),
                        blur_radius: px(12.),
                        spread_radius: px(-8.),
                    },
                    gpui::BoxShadow {
                        color: hsla(0., 0., 1., 0.46 * opacity),
                        offset: gpui::point(px(0.), px(1.)),
                        blur_radius: px(0.),
                        spread_radius: px(0.),
                    },
                ])
                .when(config.enabled, |this| {
                    this.hover(|this| {
                        this.border_color(material.rim.opacity((0.96 * opacity).min(1.0)))
                            .bg(linear_gradient(
                                135.,
                                linear_color_stop(
                                    material.start.opacity((opacity + 0.08).min(1.0)),
                                    0.0,
                                ),
                                linear_color_stop(
                                    material.end.opacity((opacity + 0.1).min(1.0)),
                                    1.0,
                                ),
                            ))
                    })
                })
                .group_active(group_name.clone(), |this| {
                    let active_top = if matches!(config.size, JellyButtonSize::Standard) {
                        6.
                    } else {
                        4.2
                    };
                    let active_bottom = if matches!(config.size, JellyButtonSize::Standard) {
                        1.4
                    } else {
                        1.
                    };
                    let active_bleed = if matches!(config.size, JellyButtonSize::Standard) {
                        -4.
                    } else {
                        -2.6
                    };

                    this.top(px(active_top))
                        .bottom(px(active_bottom))
                        .left(px(active_bleed))
                        .right(px(active_bleed))
                        .border_color(material.rim.opacity(0.7 * opacity))
                        .shadow(vec![gpui::BoxShadow {
                            color: material.aura.opacity(0.18 * opacity),
                            offset: gpui::point(px(0.), px(7.)),
                            blur_radius: px(16.),
                            spread_radius: px(-10.),
                        }])
                })
                .child(
                    div()
                        .absolute()
                        .left(px(12.))
                        .right(px(12.))
                        .bottom(px(2.))
                        .h(px(metrics.height * 0.24))
                        .rounded(px(999.))
                        .bg(hsla(0., 0., 0., 0.10 * opacity)),
                )
                .child(
                    div()
                        .id(("jelly-button-inner", id_seed))
                        .absolute()
                        .left(px(metrics.inner_x))
                        .right(px(metrics.inner_x))
                        .top(px(motion.inner_top.max(2.5)))
                        .bottom(px(motion.inner_bottom.max(2.5)))
                        .rounded(px(999.))
                        .border_1()
                        .border_color(hsla(
                            0.,
                            0.,
                            1.,
                            (0.56 + motion.breath + motion.pop * 0.22) * opacity,
                        ))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(
                                material.inner_top.opacity(
                                    (0.96 + motion.breath * 0.18 + motion.pop * 0.05) * opacity,
                                ),
                                0.0,
                            ),
                            linear_color_stop(
                                material
                                    .inner_bottom
                                    .opacity((0.86 + motion.pop * 0.1) * opacity),
                                1.0,
                            ),
                        ))
                        .shadow(vec![
                            gpui::BoxShadow {
                                color: hsla(0., 0., 1., (0.62 + motion.pop * 0.2) * opacity),
                                offset: gpui::point(px(0.), px(1.)),
                                blur_radius: px(0.),
                                spread_radius: px(0.),
                            },
                            gpui::BoxShadow {
                                color: material.start.opacity(0.16 * opacity),
                                offset: gpui::point(px(0.), px(7.)),
                                blur_radius: px(14.),
                                spread_radius: px(-10.),
                            },
                        ])
                        .group_active(group_name.clone(), |this| {
                            this.top(px(metrics.inner_y + 3.))
                                .bottom(px((metrics.inner_y - 2.).max(2.)))
                                .border_color(material.rim.opacity(0.52))
                                .bg(linear_gradient(
                                    180.,
                                    linear_color_stop(
                                        material.inner_top.opacity(0.78 * opacity),
                                        0.0,
                                    ),
                                    linear_color_stop(
                                        material.inner_bottom.opacity(0.68 * opacity),
                                        1.0,
                                    ),
                                ))
                        }),
                )
                .child(
                    div()
                        .id(("jelly-button-gloss", id_seed))
                        .absolute()
                        .top(px(motion.gloss_top))
                        .left(px(motion.gloss_inset))
                        .right(px(motion.gloss_inset))
                        .h(px(metrics.highlight_h))
                        .rounded(px(999.))
                        .bg(hsla(
                            0.,
                            0.,
                            1.,
                            (0.34 + motion.breath * 0.8 + motion.pop * 0.2) * opacity,
                        ))
                        .group_active(group_name.clone(), |this| {
                            this.top(px(8.))
                                .left(px(28.))
                                .right(px(28.))
                                .h(px((metrics.highlight_h - 1.5).max(3.)))
                                .bg(hsla(0., 0., 1., 0.22 * opacity))
                        }),
                )
                .child(
                    div()
                        .id(("jelly-button-ridge", id_seed))
                        .absolute()
                        .left(px(15.))
                        .right(px(15.))
                        .bottom(px(motion.ridge_bottom))
                        .h(px(motion.ridge_height))
                        .rounded(px(999.))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(
                                hsla(0., 0., 1., (0.06 + motion.breath * 0.22) * opacity),
                                0.0,
                            ),
                            linear_color_stop(
                                hsla(0., 0., 1., (0.22 + motion.pop * 0.18) * opacity),
                                1.0,
                            ),
                        ))
                        .group_active(group_name.clone(), |this| {
                            this.bottom(px(4.))
                                .h(px(if matches!(config.size, JellyButtonSize::Standard) {
                                    4.
                                } else {
                                    3.
                                }))
                                .bg(hsla(0., 0., 1., 0.1 * opacity))
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(18.))
                        .right(px(18.))
                        .bottom(px(0.))
                        .h(px(1.))
                        .bg(material.rim.opacity((0.28 + motion.pop * 0.24) * opacity)),
                ),
        )
        .child(
            div()
                .absolute()
                .left(px(12.))
                .right(px(12.))
                .bottom(px(2.))
                .h(px(8.))
                .rounded(px(999.))
                .bg(material.aura.opacity((0.05 + motion.pop * 0.05) * opacity)),
        )
        .child(
            div()
                .absolute()
                .left(px(0.))
                .right(px(0.))
                .bottom(px(0.))
                .h(px(2.))
                .rounded(px(999.))
                .bg(hsla(0., 0., 0., 0.06 * opacity)),
        )
        .child(
            div()
                .id(("jelly-button-label", id_seed))
                .absolute()
                .left(px(metrics.outer_pad_x * 0.5))
                .right(px(metrics.outer_pad_x * 0.5))
                .top(px(0.))
                .bottom(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .group_active(SharedString::from(config.group), |this| {
                    this.pt(px(if matches!(config.size, JellyButtonSize::Standard) {
                        2.
                    } else {
                        1.
                    }))
                })
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(
                            material
                                .text
                                .opacity(if config.enabled { 1.0 } else { 0.62 }),
                        )
                        .text_size(px(metrics.text_size))
                        .child(SharedString::from(label)),
                ),
        )
}

fn glass_auth_panel(palette: &Palette) -> gpui::Div {
    v_flex()
        .p(px(18.))
        .rounded(px(24.))
        .border_1()
        .border_color(hsla(187., 0.76, 0.62, 0.22))
        .bg(hsla(0., 0., 1., 0.72))
        .shadow(vec![
            gpui::BoxShadow {
                color: palette.accent.opacity(0.16),
                offset: gpui::point(px(0.), px(18.)),
                blur_radius: px(36.),
                spread_radius: px(-18.),
            },
            gpui::BoxShadow {
                color: palette.accent_2.opacity(0.1),
                offset: gpui::point(px(0.), px(8.)),
                blur_radius: px(18.),
                spread_radius: px(-12.),
            },
        ])
}

fn auth_summary_block(auth: &AuthState, palette: &Palette) -> impl IntoElement {
    let message = auth
        .message
        .clone()
        .unwrap_or_else(|| "等待登录态校验。".to_string());
    let detail = auth.session.detail();

    v_flex()
        .gap(px(10.))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    v_flex()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(auth.session.title()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(palette.muted)
                                .child(detail),
                        ),
                )
                .child(status_badge(
                    auth.phase.label(),
                    auth.status_kind(),
                    palette,
                )),
        )
        .child(
            div()
                .text_size(px(12.))
                .line_height(relative(1.35))
                .text_color(if auth.nav_error.is_some() {
                    palette.error
                } else {
                    palette.text
                })
                .child(SharedString::from(message)),
        )
}

fn auth_risk_block(session: &SessionMode, palette: &Palette) -> impl IntoElement {
    let (kind, title, body) = match session.kind {
        SessionKind::LoggedIn => (
            EventKind::Success,
            "已通过真实 nav 校验",
            "工作台将复用当前 cookie 来源；登录态失效时需要重新校验或扫码登录。",
        ),
        SessionKind::Anonymous => (
            EventKind::Warning,
            "匿名风险已确认",
            "匿名模式可能导致评论、弹幕或部分接口结果不完整；工作台会持续显示该风险。",
        ),
        SessionKind::Unknown => (
            EventKind::Warning,
            "未登录风险",
            "未登录时不要默认认为采集完整；请扫码登录，或明确选择匿名进入并接受完整性风险。",
        ),
    };
    let color = event_color(kind, palette);

    h_flex()
        .items_start()
        .gap(px(10.))
        .p(px(12.))
        .rounded(px(16.))
        .border_1()
        .border_color(color.opacity(0.24))
        .bg(color.opacity(0.075))
        .child(status_dot(color))
        .child(
            v_flex()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(relative(1.35))
                        .text_color(palette.text)
                        .child(body),
                ),
        )
}

fn auth_lifecycle_block(auth: &AuthState, palette: &Palette, motion_tick: u64) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap(px(10.))
        .child(auth_lifecycle_chip(
            "1",
            "nav 校验",
            matches!(
                auth.phase,
                AuthPhase::BootChecking
                    | AuthPhase::LoggedIn
                    | AuthPhase::CredentialMissing
                    | AuthPhase::CredentialInvalid
                    | AuthPhase::CredentialError
            ),
            auth.status_kind(),
            palette,
            motion_tick,
        ))
        .child(auth_lifecycle_chip(
            "2",
            "扫码 / 凭据",
            auth.should_show_qr(),
            auth.status_kind(),
            palette,
            motion_tick.wrapping_add(7),
        ))
        .child(auth_lifecycle_chip(
            "3",
            "进入工作台",
            auth.session.collection_ready().is_some(),
            if auth.session.collection_ready().is_some() {
                EventKind::Success
            } else {
                EventKind::System
            },
            palette,
            motion_tick.wrapping_add(14),
        ))
}

fn auth_lifecycle_chip(
    step: &'static str,
    label: &'static str,
    active: bool,
    kind: EventKind,
    palette: &Palette,
    motion_tick: u64,
) -> impl IntoElement {
    let color = event_color(kind, palette);
    let pulse = if active {
        wave_between(motion_tick, 0.18, 0.08, 0.2)
    } else {
        0.0
    };

    h_flex()
        .flex_1()
        .items_center()
        .gap(px(8.))
        .min_w(px(0.))
        .p(px(10.))
        .rounded(px(18.))
        .border_1()
        .border_color(color.opacity(if active { 0.24 + pulse } else { 0.12 }))
        .bg(linear_gradient(
            135.,
            linear_color_stop(
                color.opacity(if active { 0.08 + pulse * 0.4 } else { 0.035 }),
                0.0,
            ),
            linear_color_stop(hsla(0., 0., 1., if active { 0.66 } else { 0.48 }), 1.0),
        ))
        .child(
            div()
                .flex_shrink_0()
                .size(px(24.))
                .rounded(px(999.))
                .border_1()
                .border_color(color.opacity(if active { 0.38 + pulse } else { 0.18 }))
                .bg(color.opacity(if active { 0.15 + pulse } else { 0.06 }))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if active { color } else { palette.muted })
                        .child(step),
                ),
        )
        .child(
            div()
                .truncate()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if active { palette.text } else { palette.muted })
                .child(label),
        )
}

fn session_capsule(auth: &AuthState, palette: &Palette) -> impl IntoElement {
    let kind = auth.status_kind();
    let color = event_color(kind, palette);
    h_flex()
        .items_center()
        .gap(px(8.))
        .max_w(px(260.))
        .px(px(12.))
        .py(px(7.))
        .rounded(px(999.))
        .border_1()
        .border_color(color.opacity(0.22))
        .bg(linear_gradient(
            135.,
            linear_color_stop(color.opacity(0.09), 0.0),
            linear_color_stop(hsla(0., 0., 1., 0.7), 1.0),
        ))
        .child(status_dot(color))
        .child(
            v_flex()
                .min_w(px(0.))
                .gap(px(1.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(auth.phase.label()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(10.))
                        .text_color(palette.muted)
                        .child(auth.session.title()),
                ),
        )
}

fn qr_stage(auth: &AuthState, palette: &Palette, motion_tick: u64) -> impl IntoElement {
    if let Some(qr) = auth.qr.as_ref() {
        return qr_matrix_card(qr, auth.status_kind(), palette, motion_tick).into_any_element();
    }

    let pulse = wave_between(motion_tick, 0.16, 0.08, 0.2);
    div()
        .relative()
        .size(px(286.))
        .rounded(px(32.))
        .border_1()
        .border_color(palette.accent.opacity(0.16 + pulse))
        .bg(linear_gradient(
            145.,
            linear_color_stop(rgb(0xf6feff), 0.0),
            linear_color_stop(rgb(0xfff2f7), 1.0),
        ))
        .shadow_md()
        .flex()
        .items_center()
        .justify_center()
        .child(
            v_flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(64.))
                        .rounded(px(22.))
                        .border_1()
                        .border_color(palette.accent.opacity(0.26 + pulse))
                        .bg(palette.accent.opacity(0.08 + pulse * 0.35)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.muted)
                        .child(match auth.phase {
                            AuthPhase::BootChecking => "正在校验",
                            AuthPhase::LoggedIn => "登录已确认",
                            AuthPhase::AnonymousAvailable => "匿名已确认",
                            _ => "等待二维码",
                        }),
                ),
        )
        .into_any_element()
}

fn qr_matrix_card(qr: &QrState, kind: EventKind, palette: &Palette, motion_tick: u64) -> gpui::Div {
    let card = 286.;
    let well = 246.;
    let canvas = 220.;
    let quiet_modules = 4.;
    let module = canvas / (qr.matrix.width as f32 + quiet_modules * 2.);
    let qr_offset = quiet_modules * module;
    let color = event_color(kind, palette);
    let pulse = wave_between(motion_tick, 0.18, 0.08, 0.22);
    let scan = wave_01(motion_tick, 0.2);
    let mut cells = Vec::new();
    for y in 0..qr.matrix.width {
        for x in 0..qr.matrix.width {
            if qr.matrix.is_dark(x, y) {
                cells.push(
                    div()
                        .absolute()
                        .left(px(qr_offset + x as f32 * module))
                        .top(px(qr_offset + y as f32 * module))
                        .size(px((module + 0.05).max(1.0)))
                        .bg(rgb(0x101828)),
                );
            }
        }
    }

    div()
        .relative()
        .size(px(card))
        .rounded(px(32.))
        .border_1()
        .border_color(color.opacity(0.3 + pulse))
        .bg(hsla(0., 0., 1., 0.78))
        .shadow(vec![gpui::BoxShadow {
            color: color.opacity(0.18 + pulse * 0.5),
            offset: gpui::point(px(0.), px(14.)),
            blur_radius: px(30.),
            spread_radius: px(-16.),
        }])
        .child(
            div()
                .absolute()
                .top(px((card - well) / 2.))
                .left(px((card - well) / 2.))
                .size(px(well))
                .rounded(px(22.))
                .bg(rgb(0xffffff))
                .border_1()
                .border_color(hsla(0., 0., 0.98, 0.92))
                .child(
                    div()
                        .absolute()
                        .top(px((well - canvas) / 2.))
                        .left(px((well - canvas) / 2.))
                        .size(px(canvas))
                        .children(cells),
                ),
        )
        .child(
            div()
                .absolute()
                .left(px(28. + scan * 46.))
                .right(px(72. - scan * 24.))
                .top(px(10.))
                .h(px(9.))
                .rounded(px(999.))
                .bg(hsla(0., 0., 1., 0.26 + pulse * 0.7)),
        )
}

fn qr_lifecycle_card(auth: &AuthState, palette: &Palette, motion_tick: u64) -> impl IntoElement {
    let color = event_color(auth.status_kind(), palette);
    let pulse = wave_between(motion_tick, 0.16, 0.05, 0.18);
    let title = match auth.phase {
        AuthPhase::QrWaitingForScan => "等待扫码",
        AuthPhase::QrWaitingForConfirm => "手机端待确认",
        AuthPhase::QrExpired => "二维码已过期",
        AuthPhase::QrSuccessChecking => "正在保存并复核",
        AuthPhase::LoggedIn => "登录态已确认",
        AuthPhase::AnonymousAvailable => "匿名模式已确认",
        AuthPhase::BootChecking => "正在校验登录态",
        AuthPhase::CredentialMissing => "未发现可用凭据",
        AuthPhase::CredentialInvalid => "当前凭据不可用",
        AuthPhase::CredentialError => "校验遇到异常",
    };
    let elapsed = auth.qr.as_ref().map(qr_elapsed_seconds);

    v_flex()
        .w_full()
        .gap(px(7.))
        .p(px(12.))
        .rounded(px(18.))
        .border_1()
        .border_color(color.opacity(0.22 + pulse))
        .bg(linear_gradient(
            135.,
            linear_color_stop(color.opacity(0.07 + pulse * 0.36), 0.0),
            linear_color_stop(hsla(0., 0., 1., 0.68), 1.0),
        ))
        .child(
            h_flex()
                .items_center()
                .gap(px(8.))
                .child(status_dot(color))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(title),
                ),
        )
        .when_some(elapsed, |this, elapsed| {
            this.child(
                div()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(format!(
                        "已等待 {elapsed} 秒；过期状态以 Bilibili 服务端轮询结果为准。"
                    )),
            )
        })
        .when_some(auth.qr.as_ref(), |this, qr| {
            this.child(
                div()
                    .text_size(px(11.))
                    .line_height(relative(1.3))
                    .text_color(palette.text)
                    .child(SharedString::from(qr.last_status_text.clone())),
            )
        })
}

fn qr_helper_copy(auth: &AuthState) -> String {
    match auth.phase {
        AuthPhase::QrWaitingForScan => {
            "请用 Bilibili 客户端扫码。二维码有效期有限，页面会持续检测，过期后这里会提供刷新入口。"
                .to_string()
        }
        AuthPhase::QrWaitingForConfirm => {
            "已扫码，请在手机端确认登录。如果手机端退出或二维码过期，页面会切到“刷新二维码”。"
                .to_string()
        }
        AuthPhase::QrExpired => {
            "二维码已过期。点击“刷新二维码”重新生成，不需要关闭应用或重启登录流程。".to_string()
        }
        AuthPhase::QrSuccessChecking => "授权已收到，正在保存登录状态并再次确认账号。".to_string(),
        AuthPhase::LoggedIn => "登录态已通过 nav 确认，可以进入工作台。".to_string(),
        AuthPhase::AnonymousAvailable => {
            "你已选择匿名进入；后续采集会持续显示完整性风险。".to_string()
        }
        _ => "未登录时可扫码登录，也可明确选择匿名进入。".to_string(),
    }
}

fn qr_elapsed_seconds(qr: &QrState) -> u64 {
    SystemTime::now()
        .duration_since(qr.generated_at)
        .unwrap_or_default()
        .as_secs()
}

fn wave_01(motion_tick: u64, speed: f32) -> f32 {
    ((motion_tick as f32 * speed) % TAU).sin().mul_add(0.5, 0.5)
}

fn wave_between(motion_tick: u64, speed: f32, min: f32, max: f32) -> f32 {
    min + (max - min) * wave_01(motion_tick, speed)
}

fn panel(palette: &Palette) -> gpui::Div {
    v_flex()
        .p(px(16.))
        .rounded(px(14.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface)
        .shadow_sm()
}

fn panel_title(title: &'static str, subtitle: &'static str, palette: &Palette) -> impl IntoElement {
    v_flex()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .child(subtitle),
        )
}

fn form_section(
    label: &'static str,
    helper: &'static str,
    input: impl IntoElement,
    palette: &Palette,
) -> impl IntoElement {
    v_flex()
        .gap(px(8.))
        .child(section_label(label, palette))
        .child(input)
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .line_height(relative(1.25))
                .child(helper),
        )
}

fn option_group(
    label: &'static str,
    content: impl IntoElement,
    palette: &Palette,
) -> impl IntoElement {
    v_flex()
        .gap(px(8.))
        .child(section_label(label, palette))
        .child(content)
}

fn section_label(label: &'static str, palette: &Palette) -> impl IntoElement {
    div()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.text)
        .child(label)
}

fn validation_box(message: &str, palette: &Palette) -> impl IntoElement {
    h_flex()
        .gap(px(8.))
        .items_start()
        .p(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(palette.error.opacity(0.35))
        .bg(palette.error.opacity(0.07))
        .child(status_dot(palette.error))
        .child(
            div()
                .text_size(px(12.))
                .line_height(relative(1.3))
                .text_color(palette.error)
                .child(SharedString::from(message.to_string())),
        )
}

fn jelly_progress(
    percent: f32,
    phase: TaskPhase,
    motion_tick: u64,
    palette: &Palette,
) -> impl IntoElement {
    let percent = percent.clamp(0., 100.);
    let fill = (percent / 100.).clamp(0.02, 1.0);
    let wobble = match phase {
        TaskPhase::Running | TaskPhase::Cancelling => ((motion_tick % 5) as f32 - 2.) * 0.6,
        _ => 0.,
    };
    let bubble_opacity = if percent > 1. { 0.92 } else { 0.0 };

    v_flex()
        .gap(px(8.))
        .child(
            div()
                .relative()
                .h(px(30.))
                .w_full()
                .rounded(px(999.))
                .overflow_hidden()
                .border_1()
                .border_color(palette.accent.opacity(0.24))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(rgb(0xeafcff), 0.0),
                    linear_color_stop(rgb(0xffeff5), 1.0),
                ))
                .child(
                    div()
                        .absolute()
                        .top(px(4.))
                        .left(px(4.))
                        .bottom(px(4.))
                        .w(relative(fill))
                        .rounded(px(999.))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(palette.accent, 0.0),
                            linear_color_stop(palette.accent_2, 1.0),
                        ))
                        .shadow_md()
                        .child(
                            div()
                                .absolute()
                                .top(px(3.))
                                .left(px(14.))
                                .right(px(14.))
                                .h(px(7.))
                                .rounded(px(999.))
                                .bg(hsla(0., 0., 1., 0.38)),
                        )
                        .child(
                            div()
                                .absolute()
                                .right(px(-10. + wobble))
                                .top(px(-2.))
                                .size(px(28.))
                                .rounded(px(999.))
                                .bg(hsla(0., 0., 1., bubble_opacity))
                                .border_1()
                                .border_color(hsla(0., 0., 1., 0.72)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(14.))
                        .top(px(7.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text)
                        .child(format!("{percent:.0}%")),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .child(progress_microcopy(phase)),
        )
}

fn metric_chip(
    label: &'static str,
    value: usize,
    color: Hsla,
    palette: &Palette,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .gap(px(5.))
        .p(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(color.opacity(0.2))
        .bg(color.opacity(0.07))
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(17.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .child(value.to_string()),
        )
}

fn event_row(line: &EventLine, palette: &Palette) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap(px(8.))
        .items_start()
        .p(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(event_color(line.kind, palette).opacity(0.14))
        .bg(event_color(line.kind, palette).opacity(0.055))
        .child(status_dot(event_color(line.kind, palette)))
        .child(
            div()
                .flex_1()
                .text_size(px(12.))
                .line_height(relative(1.3))
                .text_color(palette.text)
                .child(SharedString::from(line.text.clone())),
        )
}

fn result_row(item: &ResultItem, palette: &Palette) -> impl IntoElement {
    let kind_label = match item.kind {
        ResultKind::Comments => "评论",
        ResultKind::Danmaku => "弹幕",
    };
    let kind = match item.kind {
        ResultKind::Comments => EventKind::Comments,
        ResultKind::Danmaku => EventKind::Danmaku,
    };
    h_flex()
        .w_full()
        .gap(px(10.))
        .items_start()
        .p(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface_soft)
        .child(status_badge(kind_label, kind, palette))
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap(px(5.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!(
                            "{} · 扫描 {} · 新增 {}",
                            item.bvid, item.scanned, item.appended
                        )),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(palette.muted)
                        .child(SharedString::from(item.extra.clone())),
                )
                .children(item.outputs.iter().map(|path| {
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(palette.muted)
                        .child(format!("文件：{}", path.display()))
                })),
        )
}

fn failure_row(failure: &FailureItem, palette: &Palette) -> impl IntoElement {
    h_flex()
        .gap(px(8.))
        .p(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(palette.error.opacity(0.22))
        .bg(palette.error.opacity(0.06))
        .child(status_dot(palette.error))
        .child(
            div()
                .flex_1()
                .text_size(px(12.))
                .line_height(relative(1.3))
                .text_color(palette.error)
                .child(format!(
                    "{} · {} · {}",
                    failure.kind, failure.bvid, failure.error
                )),
        )
}

fn empty_result_state(palette: &Palette) -> impl IntoElement {
    v_flex()
        .gap(px(6.))
        .p(px(12.))
        .rounded(px(10.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface_soft)
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("暂无结果"),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .line_height(relative(1.3))
                .child("完成后这里会显示评论、弹幕、失败项和输出文件。"),
        )
}

fn status_badge(label: &'static str, kind: EventKind, palette: &Palette) -> impl IntoElement {
    let color = event_color(kind, palette);
    h_flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.))
        .px(px(9.))
        .py(px(5.))
        .rounded(px(999.))
        .border_1()
        .border_color(color.opacity(0.25))
        .bg(color.opacity(0.09))
        .child(status_dot(color))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(label),
        )
}

fn product_mark(palette: &Palette) -> impl IntoElement {
    div()
        .relative()
        .size(px(40.))
        .rounded(px(13.))
        .overflow_hidden()
        .bg(linear_gradient(
            135.,
            linear_color_stop(palette.accent_2, 0.0),
            linear_color_stop(palette.accent, 1.0),
        ))
        .shadow_md()
        .child(
            div()
                .absolute()
                .top(px(7.))
                .left(px(8.))
                .size(px(14.))
                .rounded(px(999.))
                .bg(hsla(0., 0., 1., 0.55)),
        )
        .child(
            div()
                .absolute()
                .right(px(7.))
                .bottom(px(6.))
                .size(px(12.))
                .rounded(px(999.))
                .bg(hsla(0., 0., 1., 0.72)),
        )
}

fn status_dot(color: Hsla) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .mt(px(2.))
        .size(px(8.))
        .rounded(px(999.))
        .bg(color)
        .shadow_sm()
}

fn event_color(kind: EventKind, palette: &Palette) -> Hsla {
    match kind {
        EventKind::System => palette.muted,
        EventKind::Video => palette.accent_2,
        EventKind::Comments => palette.accent_2,
        EventKind::Danmaku => palette.accent,
        EventKind::Output => rgb(0x5865f2).into(),
        EventKind::Warning => palette.warning,
        EventKind::Success => palette.success,
        EventKind::Failure => palette.error,
    }
}

fn phase_kind(phase: TaskPhase) -> EventKind {
    match phase {
        TaskPhase::Idle => EventKind::System,
        TaskPhase::Validating => EventKind::Warning,
        TaskPhase::Running => EventKind::Danmaku,
        TaskPhase::Cancelling => EventKind::Warning,
        TaskPhase::Completed => EventKind::Success,
        TaskPhase::Failed => EventKind::Failure,
    }
}

fn progress_microcopy(phase: TaskPhase) -> &'static str {
    match phase {
        TaskPhase::Idle => "等待任务开始。",
        TaskPhase::Validating => "正在检查输入和采集选项。",
        TaskPhase::Running => "评论批次和弹幕分段会实时推动进度。",
        TaskPhase::Cancelling => "正在等待当前采集任务退出。",
        TaskPhase::Completed => "采集完成，可以复核输出结果。",
        TaskPhase::Failed => "任务失败，请查看事件流中的错误。",
    }
}

fn ease_towards(current: f32, target: f32) -> f32 {
    if target <= current {
        target
    } else {
        current + (target - current) * 0.42
    }
}

fn trimmed_input(input: &Entity<InputState>, cx: &App) -> Option<String> {
    let value = input.read(cx).value().trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn trimmed_cookie_input(input: &Entity<InputState>, cx: &App) -> Option<String> {
    let value = input.read(cx).value().trim().to_string();
    if value.is_empty() || value == DEFAULT_COOKIE_PATH {
        None
    } else {
        Some(value)
    }
}

fn format_bvid_preview(bvids: &[String]) -> String {
    const PREVIEW_LIMIT: usize = 5;

    let mut preview = bvids
        .iter()
        .take(PREVIEW_LIMIT)
        .cloned()
        .collect::<Vec<_>>()
        .join("、");

    if bvids.len() > PREVIEW_LIMIT {
        preview.push_str(&format!("，另有 {} 个", bvids.len() - PREVIEW_LIMIT));
    }

    preview
}

fn yes_no(value: bool) -> &'static str {
    if value { "是" } else { "否" }
}

fn collect_scope_label(comments: bool, danmaku: bool) -> &'static str {
    match (comments, danmaku) {
        (true, true) => "评论 + 弹幕",
        (true, false) => "仅评论",
        (false, true) => "仅弹幕",
        (false, false) => "未启用",
    }
}

fn auth_event_kind(session: &SessionMode) -> EventKind {
    match session.kind {
        SessionKind::LoggedIn => EventKind::Success,
        SessionKind::Anonymous => EventKind::Warning,
        SessionKind::Unknown => EventKind::Failure,
    }
}

fn credential_source_label(source: CredentialSource) -> &'static str {
    match source {
        CredentialSource::None => "无凭据",
        CredentialSource::DefaultCookie => "默认 cookie",
        CredentialSource::ExplicitCookie => "显式 cookie",
        CredentialSource::QrLogin => "二维码登录",
        CredentialSource::Anonymous => "匿名",
    }
}
