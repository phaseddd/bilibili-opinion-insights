use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    App, AppContext as _, Application, Bounds, ClickEvent, Context, Entity, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, WindowBounds, WindowOptions, div, hsla,
    linear_color_stop, linear_gradient, prelude::FluentBuilder as _, px, relative, rgb, size,
};
use gpui_component::{
    Root, h_flex,
    input::{Input, InputState},
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::app::collection::{DEFAULT_COOKIE_PATH, DEFAULT_OUTPUT_ROOT};
use crate::app::events::CollectionEvent;
use crate::bili::auth::QrLoginStatus;
use crate::bili::video::normalize_bvid_input;
use crate::gui::components::jelly_button::{
    HeaderActionKind, HeaderButtonConfig, JellyActionTone, JellyButtonConfig, JellyButtonSize,
    header_action_button, jelly_action_button,
};
use crate::gui::components::jelly_progress::jelly_progress as jelly_progress_component;
use crate::gui::components::jelly_switch::{
    JellySwitchConfig, JellySwitchSize, JellySwitchTone, jelly_switch,
};
use crate::gui::components::jelly_task_lane::{jelly_task_lane, jelly_task_lane_tone};
use crate::gui::materials::{JellyMaterialToken, JellyTone};
use crate::gui::messages::{AuthMessage, GuiMessage};
use crate::gui::motion::{
    JellyMotionSnapshot, JellySwitchMotionSnapshot, VISUAL_MOTION_TICK_MS, wave_between,
};
use crate::gui::rendering::jelly_image_cache::{
    JellyButtonImage, JellyButtonImageRequest, JellyCapsuleImage, JellyCapsuleImageRequest,
    JellyProgressImagePhase, JellyProgressImageQuality, JellyProgressImageRequest,
    JellySurfaceImage, JellySurfaceImageRequest, JellySwitchImage, JellySwitchImageRequest,
};
use crate::gui::rendering::jelly_surface_bitmap::JellySurfaceDensity;
use crate::gui::state::auth::{
    AuthPhase, AuthState, CredentialSource, QrState, SessionKind, SessionMode,
};
use crate::gui::state::events::{EVENT_LIMIT, EventKind, EventLine, EventState};
use crate::gui::state::results::{
    FailureItem, ResultItem, ResultKind, ResultState, format_collection_job,
};
use crate::gui::state::task::{RunSummary, TaskLanePhase, TaskPhase, TaskState};
use crate::gui::state::visual::{ButtonMotionId, VisualState};
use crate::gui::theme::Palette;
use crate::gui::views::auth_gate::{
    auth_lifecycle_block, auth_risk_block, auth_summary_block, glass_auth_panel, qr_helper_copy,
    qr_lifecycle_card, qr_stage, session_capsule,
};
use crate::gui::views::primitives::{
    form_section, jelly_form_field, metric_chip, option_group, panel, panel_title, phase_kind,
    product_mark, progress_visual_phase, status_capsule, status_dot, validation_box,
};
use crate::gui::views::workbench_widgets::{empty_result_state, failure_row, result_row};
use crate::gui::workers::auth_worker::{
    spawn_auth_bootstrap_worker, spawn_qr_generate_worker, spawn_qr_poll_worker,
};
use crate::gui::workers::collection_worker::{CollectionDraft, spawn_collection_worker};

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

#[derive(Clone, Copy)]
struct SwitchImageConfig {
    tone: JellySwitchTone,
    checked: bool,
    enabled: bool,
    active: bool,
    size: JellySwitchSize,
    motion: JellySwitchMotionSnapshot,
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
                format!(
                    "正在读取手动指定的 Cookie 并校验登录状态：{}",
                    path.display()
                )
            } else {
                "正在读取默认 Cookie 文件并校验登录状态。".to_string()
            },
        );
        self.events
            .push(EventKind::System, "身份入口：开始登录状态校验。");

        let (sender, receiver) = mpsc::channel();
        spawn_auth_bootstrap_worker(sender, cancel, explicit_cookie);
        spawn_message_pump(receiver, cx);
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn start_qr_login(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_motion(ButtonMotionId::QrLogin, cx);
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
        self.trigger_button_motion(ButtonMotionId::RecheckAuth, cx);
        self.start_auth_bootstrap(cx);
    }

    fn continue_anonymous(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_motion(ButtonMotionId::Anonymous, cx);
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
        self.trigger_button_motion(ButtonMotionId::EnterWorkbench, cx);
        let Some(session) = self.auth.session.collection_ready() else {
            self.task.validation_error = Some("请先登录，或明确选择匿名进入。".to_string());
            cx.notify();
            return;
        };

        self.cancel_auth_worker();
        self.auth.qr = None;
        self.auth.phase = match session.kind {
            SessionKind::LoggedIn => AuthPhase::LoggedIn,
            SessionKind::Anonymous => AuthPhase::AnonymousAvailable,
            SessionKind::Unknown => self.auth.phase,
        };
        self.auth.message = Some(format!("工作台已继承身份入口状态：{}", session.detail()));
        self.auth.nav_error = None;
        self.auth.last_checked_at = Some(SystemTime::now());
        self.app_view = AppView::Workbench;
        self.events.push(
            EventKind::System,
            format!("进入工作台：{}", session.detail()),
        );
        cx.notify();
    }

    fn trigger_button_motion(&mut self, id: ButtonMotionId, cx: &mut Context<Self>) {
        self.visual.trigger_button(id);
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn ensure_motion_loop(&mut self, cx: &mut Context<Self>) {
        if self.visual.motion_loop_running || !self.should_animate_visuals() {
            return;
        }

        self.visual.motion_loop_running = true;
        cx.spawn(async move |view, cx| {
            let mut last_frame = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(VISUAL_MOTION_TICK_MS))
                    .await;

                let now = Instant::now();
                let dt = now.duration_since(last_frame).as_secs_f32();
                last_frame = now;

                let keep_running = match view.update(cx, |view, cx| {
                    if view.should_animate_visuals() {
                        view.visual.motion_tick = view.visual.motion_tick.wrapping_add(1);
                        view.task.tick_visual_motion(dt);
                        view.visual.tick_switch_motion(dt);
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

        identity_motion
            || task_motion
            || self.task.has_active_visual_motion()
            || self.visual.has_active_control_motion()
    }

    fn button_image(
        &mut self,
        palette: &Palette,
        tone: JellyActionTone,
        enabled: bool,
        loading: bool,
        size: JellyButtonSize,
        motion: JellyMotionSnapshot,
    ) -> Option<JellyButtonImage> {
        let jelly_tone = button_tone(tone);
        let (width, height) = match size {
            JellyButtonSize::Standard => (360., 66.),
            JellyButtonSize::Compact => (148., 48.),
        };

        self.visual
            .image_cache
            .button_image(JellyButtonImageRequest {
                width,
                height,
                motion,
                tone: jelly_tone,
                material: JellyMaterialToken::for_tone(jelly_tone, palette),
                enabled,
                loading,
            })
    }

    fn switch_image(
        &mut self,
        palette: &Palette,
        request: SwitchImageConfig,
    ) -> Option<JellySwitchImage> {
        let jelly_tone = switch_tone(request.tone);
        let (width, height) = match request.size {
            JellySwitchSize::Standard => (118., 42.),
            JellySwitchSize::Compact => (100., 36.),
        };

        self.visual
            .image_cache
            .switch_image(JellySwitchImageRequest {
                width,
                height,
                motion: request.motion,
                tone: jelly_tone,
                material: JellyMaterialToken::for_tone(jelly_tone, palette),
                checked: request.checked,
                enabled: request.enabled,
                active: request.active,
            })
    }

    fn capsule_image(
        &mut self,
        palette: &Palette,
        kind: EventKind,
        width: f32,
        active: bool,
    ) -> Option<JellyCapsuleImage> {
        let tone = event_tone(kind);
        let motion = self.capsule_motion(active);

        self.visual
            .image_cache
            .capsule_image(JellyCapsuleImageRequest {
                width,
                height: 34.,
                motion,
                tone,
                material: JellyMaterialToken::for_tone(tone, palette),
                enabled: true,
                active,
            })
    }

    fn capsule_motion(&self, active: bool) -> JellyMotionSnapshot {
        let breath = if active {
            wave_between(self.visual.motion_tick, 0.18, 0.08, 0.32)
        } else {
            0.
        };

        JellyMotionSnapshot {
            pressure: breath * 0.36,
            rebound: breath * 0.14,
            squash_x: breath * 0.18,
            squash_y: breath * 0.12,
            rim_pressure: 0.18 + breath * 0.46,
            gloss_phase: (self.visual.motion_tick as f32 * 0.13)
                .sin()
                .mul_add(0.5, 0.5),
            inner_lag: breath * 0.12,
            contact: 0.18 + breath * 0.34,
            aura: 0.2 + breath * 0.42,
            error_shake: 0.,
        }
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
        self.trigger_button_motion(ButtonMotionId::HeaderStart, cx);
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

        self.task.begin_run(
            RunSummary {
                videos: draft.bvids.clone(),
                output: draft.output.clone(),
                collect_comments: draft.collect_comments,
                collect_danmaku: draft.collect_danmaku,
            },
            total_units,
        );
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
        spawn_collection_worker(draft, sender);
        spawn_message_pump(receiver, cx);
        cx.notify();
    }

    fn apply_message(&mut self, message: GuiMessage, cx: &mut Context<Self>) {
        match message {
            GuiMessage::Auth(message) => self.apply_auth_message(message, cx),
            GuiMessage::Event(event) => self.apply_collection_event(event),
            GuiMessage::Outcome(job) => {
                self.results.jobs.push(ResultItem::from_job(&job));
                self.events
                    .push(EventKind::Success, format_collection_job(&job));
            }
            GuiMessage::Failure(failure) => {
                self.task.mark_failure(&failure.bvid, &failure.kind);
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
                self.task.finish_run(success);
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
        if self.app_view == AppView::Workbench {
            return;
        }

        match message {
            AuthMessage::BootChecking => {
                self.auth.set_phase(
                    AuthPhase::BootChecking,
                    "正在读取默认 Cookie 文件并校验登录状态。",
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
                        "二维码 Cookie 已保存，但登录状态未确认，请刷新二维码重试。",
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
        self.task.apply_collection_event(&event);
        self.events
            .push_line(EventLine::from_collection_event(&event));
    }

    fn finish_progress_unit(&mut self) {
        self.task.finish_progress_unit();
    }

    fn toggle_collect_comments(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.task.phase.is_busy() {
            return;
        }
        self.form.collect_comments = !self.form.collect_comments;
        self.visual.toggle_switch(101, self.form.collect_comments);
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn toggle_collect_danmaku(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.task.phase.is_busy() {
            return;
        }
        self.form.collect_danmaku = !self.form.collect_danmaku;
        self.visual.toggle_switch(102, self.form.collect_danmaku);
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn toggle_write_csv(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.task.phase.is_busy() {
            return;
        }
        self.form.write_csv = !self.form.write_csv;
        self.visual.toggle_switch(103, self.form.write_csv);
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn toggle_write_jsonl(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.task.phase.is_busy() {
            return;
        }
        self.form.write_jsonl = !self.form.write_jsonl;
        self.visual.toggle_switch(104, self.form.write_jsonl);
        self.ensure_motion_loop(cx);
        cx.notify();
    }

    fn clear_log(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_motion(ButtonMotionId::HeaderClear, cx);
        if self.task.phase.is_busy() {
            return;
        }

        self.events.clear();
        self.events.push(EventKind::System, "事件已清空。");
        self.task.clear_idle_progress();
        self.results = ResultState::default();
        cx.notify();
    }

    fn request_cancel(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.trigger_button_motion(ButtonMotionId::HeaderCancel, cx);
        if self.task.phase == TaskPhase::Running {
            self.task.request_cancel_visual();
            self.events.push(
                EventKind::Warning,
                "正在请求取消；当前采集任务会在安全位置停止。",
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

    fn render_identity_gate(&mut self, palette: &Palette, cx: &mut Context<Self>) -> gpui::Div {
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
        let enter_motion =
            self.visual
                .button_motion(ButtonMotionId::EnterWorkbench, false, !can_enter);
        let qr_loading =
            self.auth.should_show_qr() || matches!(self.auth.phase, AuthPhase::BootChecking);
        let qr_motion = self
            .visual
            .button_motion(ButtonMotionId::QrLogin, qr_loading, false);
        let recheck_motion = self.visual.button_motion(
            ButtonMotionId::RecheckAuth,
            self.auth.phase == AuthPhase::BootChecking,
            false,
        );
        let anonymous_motion = self
            .visual
            .button_motion(ButtonMotionId::Anonymous, false, false);
        let enter_image = self.button_image(
            palette,
            JellyActionTone::Primary,
            can_enter,
            false,
            JellyButtonSize::Standard,
            enter_motion,
        );
        let qr_image = self.button_image(
            palette,
            JellyActionTone::Cyan,
            can_login,
            qr_loading,
            JellyButtonSize::Standard,
            qr_motion,
        );
        let recheck_image = self.button_image(
            palette,
            JellyActionTone::Neutral,
            !self.auth.is_busy(),
            self.auth.phase == AuthPhase::BootChecking,
            JellyButtonSize::Standard,
            recheck_motion,
        );
        let anonymous_image = self.button_image(
            palette,
            JellyActionTone::Warning,
            !matches!(self.auth.phase, AuthPhase::BootChecking),
            false,
            JellyButtonSize::Standard,
            anonymous_motion,
        );
        let auth_capsule_kind = self.auth.status_kind();
        let auth_capsule_image =
            self.capsule_image(palette, auth_capsule_kind, 156., self.auth.is_busy());

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
                                                                "身份入口会先真实校验登录状态，再决定登录或匿名进入。",
                                                            ),
                                                    ),
                                            ),
                                    )
                                    .child(status_capsule(
                                        self.auth.phase.label(),
                                        auth_capsule_kind,
                                        palette,
                                        auth_capsule_image,
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
                                                                id_seed:
                                                                    ButtonMotionId::EnterWorkbench
                                                                        as usize,
                                                                size: JellyButtonSize::Standard,
                                                                motion: enter_motion,
                                                                image: enter_image,
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
                                                                loading: qr_loading,
                                                                motion_tick,
                                                                group: "auth-start-qr-login-group",
                                                                id_seed: ButtonMotionId::QrLogin
                                                                    as usize,
                                                                size: JellyButtonSize::Standard,
                                                                motion: qr_motion,
                                                                image: qr_image,
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
                                                                id_seed:
                                                                    ButtonMotionId::RecheckAuth
                                                                        as usize,
                                                                size: JellyButtonSize::Standard,
                                                                motion: recheck_motion,
                                                                image: recheck_image,
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
                                                                id_seed: ButtonMotionId::Anonymous
                                                                    as usize,
                                                                size: JellyButtonSize::Standard,
                                                                motion: anonymous_motion,
                                                                image: anonymous_image,
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
        &mut self,
        palette: &Palette,
        can_start: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let motion_tick = self.visual.motion_tick;
        let clear_motion = self
            .visual
            .button_motion(ButtonMotionId::HeaderClear, false, false);
        let cancel_loading = self.task.phase == TaskPhase::Cancelling;
        let cancel_motion =
            self.visual
                .button_motion(ButtonMotionId::HeaderCancel, cancel_loading, false);
        let start_loading = self.task.phase.is_busy();
        let start_error = self.task.phase == TaskPhase::Failed;
        let start_motion =
            self.visual
                .button_motion(ButtonMotionId::HeaderStart, start_loading, start_error);
        let clear_image = self.button_image(
            palette,
            JellyActionTone::Neutral,
            can_start,
            false,
            JellyButtonSize::Compact,
            clear_motion,
        );
        let cancel_image = self.button_image(
            palette,
            JellyActionTone::Warning,
            self.task.phase == TaskPhase::Running,
            cancel_loading,
            JellyButtonSize::Compact,
            cancel_motion,
        );
        let start_image = self.button_image(
            palette,
            JellyActionTone::Primary,
            can_start,
            start_loading,
            JellyButtonSize::Compact,
            start_motion,
        );
        let session_capsule_image = self.capsule_image(
            palette,
            self.auth.status_kind(),
            260.,
            self.auth.is_busy() || self.task.phase.is_busy(),
        );

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
                    .child(session_capsule(&self.auth, palette, session_capsule_image))
                    .child(
                        header_action_button(
                            "清空",
                            palette,
                            HeaderButtonConfig {
                                kind: HeaderActionKind::Ghost,
                                enabled: can_start,
                                motion_tick,
                                group: "header-clear-group",
                                id_seed: ButtonMotionId::HeaderClear as usize,
                                motion: clear_motion,
                                image: clear_image,
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
                                motion_tick,
                                group: "header-cancel-group",
                                id_seed: ButtonMotionId::HeaderCancel as usize,
                                motion: cancel_motion,
                                image: cancel_image,
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
                                motion_tick,
                                group: "header-start-group",
                                id_seed: ButtonMotionId::HeaderStart as usize,
                                motion: start_motion,
                                image: start_image,
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
        &mut self,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let options_enabled = !self.task.phase.is_busy();
        let comments_active = self.task.phase.is_busy() && self.form.collect_comments;
        let danmaku_active = self.task.phase.is_busy() && self.form.collect_danmaku;
        let csv_active = self.task.phase.is_busy() && self.form.write_csv;
        let jsonl_active = self.task.phase.is_busy() && self.form.write_jsonl;
        let comments_motion =
            self.visual
                .switch_motion(101, self.form.collect_comments, comments_active);
        let danmaku_motion =
            self.visual
                .switch_motion(102, self.form.collect_danmaku, danmaku_active);
        let csv_motion = self
            .visual
            .switch_motion(103, self.form.write_csv, csv_active);
        let jsonl_motion = self
            .visual
            .switch_motion(104, self.form.write_jsonl, jsonl_active);
        let comments_image = self.switch_image(
            palette,
            SwitchImageConfig {
                tone: JellySwitchTone::Primary,
                checked: self.form.collect_comments,
                enabled: options_enabled,
                active: comments_active,
                size: JellySwitchSize::Standard,
                motion: comments_motion,
            },
        );
        let danmaku_image = self.switch_image(
            palette,
            SwitchImageConfig {
                tone: JellySwitchTone::Cyan,
                checked: self.form.collect_danmaku,
                enabled: options_enabled,
                active: danmaku_active,
                size: JellySwitchSize::Standard,
                motion: danmaku_motion,
            },
        );
        let csv_image = self.switch_image(
            palette,
            SwitchImageConfig {
                tone: JellySwitchTone::Output,
                checked: self.form.write_csv,
                enabled: options_enabled,
                active: csv_active,
                size: JellySwitchSize::Standard,
                motion: csv_motion,
            },
        );
        let jsonl_image = self.switch_image(
            palette,
            SwitchImageConfig {
                tone: JellySwitchTone::Output,
                checked: self.form.write_jsonl,
                enabled: options_enabled,
                active: jsonl_active,
                size: JellySwitchSize::Standard,
                motion: jsonl_motion,
            },
        );
        let task_active = self.task.phase.is_busy();
        let bvids_field_image =
            self.form_surface_image(palette, JellyTone::Primary, 420., 150., task_active);
        let cookie_field_image =
            self.form_surface_image(palette, JellyTone::Neutral, 420., 50., false);
        let output_field_image =
            self.form_surface_image(palette, JellyTone::Output, 420., 50., task_active);

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
                jelly_form_field(
                    Input::new(&self.form.bvids_input)
                        .h_full()
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false),
                    palette,
                    bvids_field_image,
                    150.,
                    JellyTone::Primary,
                ),
                palette,
            ))
            .child(form_section(
                "Cookie 文件",
                "工作台默认继承身份入口状态；这里仍可填写手动指定的 Cookie 文件供采集使用。",
                jelly_form_field(
                    Input::new(&self.form.cookie_input)
                        .cleanable(true)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false),
                    palette,
                    cookie_field_image,
                    50.,
                    JellyTone::Neutral,
                ),
                palette,
            ))
            .child(form_section(
                "输出目录",
                "当前写入本地目录，复跑会跳过已存在记录。",
                jelly_form_field(
                    Input::new(&self.form.output_input)
                        .cleanable(true)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false),
                    palette,
                    output_field_image,
                    50.,
                    JellyTone::Output,
                ),
                palette,
            ))
            .child(self.render_auth_strip(palette))
            .child(option_group(
                "采集内容",
                h_flex()
                    .gap(px(12.))
                    .child(
                        jelly_switch(
                            JellySwitchConfig {
                                label: "评论",
                                checked: self.form.collect_comments,
                                enabled: options_enabled,
                                tone: JellySwitchTone::Primary,
                                size: JellySwitchSize::Standard,
                                motion_tick: self.visual.motion_tick,
                                group: "collect-comments-switch",
                                id_seed: 101,
                                active: self.task.phase.is_busy() && self.form.collect_comments,
                                motion: comments_motion,
                                image: comments_image,
                            },
                            palette,
                        )
                        .id("collect-comments")
                        .when(options_enabled, |this| {
                            this.cursor_pointer()
                                .on_click(cx.listener(Self::toggle_collect_comments))
                        }),
                    )
                    .child(
                        jelly_switch(
                            JellySwitchConfig {
                                label: "弹幕",
                                checked: self.form.collect_danmaku,
                                enabled: options_enabled,
                                tone: JellySwitchTone::Cyan,
                                size: JellySwitchSize::Standard,
                                motion_tick: self.visual.motion_tick,
                                group: "collect-danmaku-switch",
                                id_seed: 102,
                                active: self.task.phase.is_busy() && self.form.collect_danmaku,
                                motion: danmaku_motion,
                                image: danmaku_image,
                            },
                            palette,
                        )
                        .id("collect-danmaku")
                        .when(options_enabled, |this| {
                            this.cursor_pointer()
                                .on_click(cx.listener(Self::toggle_collect_danmaku))
                        }),
                    ),
                palette,
            ))
            .child(option_group(
                "评论输出",
                h_flex()
                    .gap(px(12.))
                    .child(
                        jelly_switch(
                            JellySwitchConfig {
                                label: "CSV",
                                checked: self.form.write_csv,
                                enabled: options_enabled,
                                tone: JellySwitchTone::Output,
                                size: JellySwitchSize::Standard,
                                motion_tick: self.visual.motion_tick,
                                group: "write-csv-switch",
                                id_seed: 103,
                                active: self.task.phase.is_busy() && self.form.write_csv,
                                motion: csv_motion,
                                image: csv_image,
                            },
                            palette,
                        )
                        .id("write-csv")
                        .when(options_enabled, |this| {
                            this.cursor_pointer()
                                .on_click(cx.listener(Self::toggle_write_csv))
                        }),
                    )
                    .child(
                        jelly_switch(
                            JellySwitchConfig {
                                label: "JSONL",
                                checked: self.form.write_jsonl,
                                enabled: options_enabled,
                                tone: JellySwitchTone::Output,
                                size: JellySwitchSize::Standard,
                                motion_tick: self.visual.motion_tick,
                                group: "write-jsonl-switch",
                                id_seed: 104,
                                active: self.task.phase.is_busy() && self.form.write_jsonl,
                                motion: jsonl_motion,
                                image: jsonl_image,
                            },
                            palette,
                        )
                        .id("write-jsonl")
                        .when(options_enabled, |this| {
                            this.cursor_pointer()
                                .on_click(cx.listener(Self::toggle_write_jsonl))
                        }),
                    ),
                palette,
            ))
            .when_some(self.task.validation_error.clone(), |this, message| {
                this.child(validation_box(&message, palette))
            })
    }

    fn render_auth_strip(&mut self, palette: &Palette) -> impl IntoElement {
        let message = self
            .auth
            .message
            .clone()
            .unwrap_or_else(|| "等待身份状态。".to_string());
        let risk = self.auth.session.completeness_warning;
        let auth_kind = self.auth.status_kind();
        let capsule_image = self.capsule_image(palette, auth_kind, 156., self.auth.is_busy());

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
                    .child(status_capsule(
                        self.auth.phase.label(),
                        auth_kind,
                        palette,
                        capsule_image,
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

    fn render_progress_panel(&mut self, palette: &Palette) -> impl IntoElement {
        let progress_motion = self
            .task
            .progress
            .motion_snapshot(self.task.phase, self.visual.motion_tick);
        let progress_tone = progress_tone(self.task.phase);
        let progress_bitmap = self
            .visual
            .image_cache
            .progress_image(JellyProgressImageRequest {
                width: 920.,
                height: 46.,
                quality: JellyProgressImageQuality::Main,
                motion: progress_motion,
                phase: progress_image_phase(self.task.phase),
                tone: progress_tone,
                material: crate::gui::materials::JellyMaterialToken::for_tone(
                    progress_tone,
                    palette,
                ),
            });
        let phase_kind = phase_kind(self.task.phase);
        let status_capsule_image =
            self.capsule_image(palette, phase_kind, 136., self.task.phase.is_busy());
        let comments_scanned_image = self.metric_surface_image(
            palette,
            EventKind::Comments,
            self.task.progress.comments_scanned,
        );
        let comments_appended_image = self.metric_surface_image(
            palette,
            EventKind::Success,
            self.task.progress.comments_appended,
        );
        let danmaku_scanned_image = self.metric_surface_image(
            palette,
            EventKind::Danmaku,
            self.task.progress.danmaku_scanned,
        );
        let danmaku_segments_image = self.metric_surface_image(
            palette,
            EventKind::Warning,
            self.task.progress.danmaku_segments,
        );

        panel(palette)
            .flex_shrink_0()
            .max_h(px(430.))
            .gap(px(14.))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(panel_title("任务进度", "根据实际采集进度更新", palette))
                    .child(status_capsule(
                        self.task.phase.label(),
                        phase_kind,
                        palette,
                        status_capsule_image,
                    )),
            )
            .child(jelly_progress_component(
                progress_motion,
                progress_visual_phase(self.task.phase),
                palette,
                progress_bitmap,
            ))
            .when(!self.task.lanes.is_empty(), |this| {
                this.child(self.render_task_lanes(palette))
            })
            .child(
                v_flex()
                    .gap(px(10.))
                    .child(h_flex().gap(px(10.)).children([
                        metric_chip(
                            "评论扫描",
                            self.task.progress.comments_scanned,
                            palette.accent_2,
                            palette,
                            comments_scanned_image,
                        ),
                        metric_chip(
                            "评论新增",
                            self.task.progress.comments_appended,
                            palette.success,
                            palette,
                            comments_appended_image,
                        ),
                    ]))
                    .child(h_flex().gap(px(10.)).children([
                        metric_chip(
                            "弹幕扫描",
                            self.task.progress.danmaku_scanned,
                            palette.accent,
                            palette,
                            danmaku_scanned_image,
                        ),
                        metric_chip(
                            "分段",
                            self.task.progress.danmaku_segments,
                            palette.warning,
                            palette,
                            danmaku_segments_image,
                        ),
                    ])),
            )
            .child(self.render_run_summary(palette))
    }

    fn render_task_lanes(&mut self, palette: &Palette) -> impl IntoElement {
        v_flex()
            .id("gui-task-lanes-scroll")
            .max_h(px(178.))
            .gap(px(8.))
            .p(px(8.))
            .rounded(px(10.))
            .border_1()
            .border_color(palette.border)
            .bg(palette.event_bg)
            .children(self.task.lanes.iter().map(|lane| {
                let motion = lane.motion_snapshot(self.visual.motion_tick);
                let tone = jelly_task_lane_tone(lane);
                let bitmap = self
                    .visual
                    .image_cache
                    .progress_image(JellyProgressImageRequest {
                        width: 360.,
                        height: 26.,
                        quality: JellyProgressImageQuality::Lane,
                        motion,
                        phase: lane_image_phase(lane.phase),
                        tone,
                        material: crate::gui::materials::JellyMaterialToken::for_tone(
                            tone, palette,
                        ),
                    });

                jelly_task_lane(lane, self.visual.motion_tick, palette, bitmap)
            }))
            .overflow_y_scrollbar()
    }

    fn event_surface_image(
        &mut self,
        palette: &Palette,
        line: &EventLine,
    ) -> Option<JellySurfaceImage> {
        let density = match line.kind {
            EventKind::Output => JellySurfaceDensity::Result,
            _ => JellySurfaceDensity::Event,
        };
        let active = matches!(
            line.kind,
            EventKind::Video | EventKind::Comments | EventKind::Danmaku | EventKind::Output
        );

        self.surface_image_for_kind(
            palette,
            line.kind,
            density,
            980.,
            if matches!(density, JellySurfaceDensity::Result) {
                48.
            } else {
                42.
            },
            active,
        )
    }

    fn result_surface_image(
        &mut self,
        palette: &Palette,
        item: &ResultItem,
    ) -> Option<JellySurfaceImage> {
        let kind = match item.kind {
            ResultKind::Comments => EventKind::Comments,
            ResultKind::Danmaku => EventKind::Danmaku,
        };
        let active = item.scanned > 0 || item.appended > 0;
        let height = (62. + item.outputs.len() as f32 * 14.).clamp(64., 118.);

        self.surface_image_for_kind(
            palette,
            kind,
            JellySurfaceDensity::Result,
            980.,
            height,
            active,
        )
    }

    fn failure_surface_image(
        &mut self,
        palette: &Palette,
        _failure: &FailureItem,
    ) -> Option<JellySurfaceImage> {
        self.surface_image_for_kind(
            palette,
            EventKind::Failure,
            JellySurfaceDensity::Result,
            980.,
            58.,
            true,
        )
    }

    fn empty_result_surface_image(&mut self, palette: &Palette) -> Option<JellySurfaceImage> {
        self.surface_image_for_kind(
            palette,
            EventKind::Output,
            JellySurfaceDensity::Result,
            980.,
            78.,
            false,
        )
    }

    fn metric_surface_image(
        &mut self,
        palette: &Palette,
        kind: EventKind,
        value: usize,
    ) -> Option<JellySurfaceImage> {
        self.surface_image_for_kind(
            palette,
            kind,
            JellySurfaceDensity::Result,
            460.,
            84.,
            value > 0,
        )
    }

    fn form_surface_image(
        &mut self,
        palette: &Palette,
        tone: JellyTone,
        width: f32,
        height: f32,
        active: bool,
    ) -> Option<JellySurfaceImage> {
        self.surface_image_for_tone(
            palette,
            tone,
            JellySurfaceDensity::Result,
            width,
            height,
            active,
        )
    }

    fn surface_image_for_kind(
        &mut self,
        palette: &Palette,
        kind: EventKind,
        density: JellySurfaceDensity,
        width: f32,
        height: f32,
        active: bool,
    ) -> Option<JellySurfaceImage> {
        self.surface_image_for_tone(palette, event_tone(kind), density, width, height, active)
    }

    fn surface_image_for_tone(
        &mut self,
        palette: &Palette,
        tone: JellyTone,
        density: JellySurfaceDensity,
        width: f32,
        height: f32,
        active: bool,
    ) -> Option<JellySurfaceImage> {
        self.visual
            .image_cache
            .surface_image(JellySurfaceImageRequest {
                width,
                height,
                motion: JellyMotionSnapshot {
                    pressure: if active { 0.18 } else { 0.08 },
                    rebound: if active { 0.08 } else { 0. },
                    squash_x: 0.,
                    squash_y: 0.,
                    rim_pressure: if active { 0.24 } else { 0.12 },
                    gloss_phase: (self.visual.motion_tick as f32 * 0.09)
                        .sin()
                        .mul_add(0.5, 0.5),
                    inner_lag: 0.,
                    contact: if active { 0.18 } else { 0.08 },
                    aura: if active { 0.18 } else { 0.08 },
                    error_shake: 0.,
                },
                tone,
                material: JellyMaterialToken::for_tone(tone, palette),
                density,
                active,
            })
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
                        "{}/{} 项工作",
                        self.task.progress.completed_units, self.task.progress.total_units
                    )),
            )
    }

    fn render_event_panel(&mut self, palette: &Palette) -> impl IntoElement {
        let dropped = self.events.dropped_count;
        let event_lines: Vec<_> = self.events.lines.iter().rev().cloned().collect();
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
                    .children(event_lines.iter().map(|line| {
                        let image = self.event_surface_image(palette, line);
                        crate::gui::components::jelly_event_row::jelly_event_row(
                            line, palette, image,
                        )
                    }))
                    .overflow_y_scrollbar(),
            )
    }

    fn render_result_panel(&mut self, palette: &Palette) -> impl IntoElement {
        let result_items = self.results.jobs.to_vec();
        let failure_items = self.results.failures.to_vec();
        let result_label = if !self.results.failures.is_empty() {
            "有失败项"
        } else if self.results.jobs.is_empty() {
            "等待运行"
        } else {
            "有输出"
        };
        let result_kind = if self.results.failures.is_empty() {
            EventKind::Output
        } else {
            EventKind::Failure
        };
        let result_capsule_image = self.capsule_image(
            palette,
            result_kind,
            136.,
            !self.results.failures.is_empty(),
        );
        let empty_result_image = if self.results.jobs.is_empty() {
            self.empty_result_surface_image(palette)
        } else {
            None
        };

        panel(palette)
            .flex_shrink_0()
            .max_h(px(250.))
            .gap(px(12.))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(panel_title(
                        "本次运行",
                        "输出摘要、失败项和完整性提示",
                        palette,
                    ))
                    .child(status_capsule(
                        result_label,
                        result_kind,
                        palette,
                        result_capsule_image,
                    )),
            )
            .child(
                v_flex()
                    .max_h(px(132.))
                    .gap(px(8.))
                    .children(result_items.iter().map(|item| {
                        let image = self.result_surface_image(palette, item);
                        result_row(item, palette, image)
                    }))
                    .when(self.results.jobs.is_empty(), |this| {
                        this.child(empty_result_state(palette, empty_result_image))
                    })
                    .overflow_y_scrollbar(),
            )
            .when(!self.results.failures.is_empty(), |this| {
                this.child(
                    v_flex()
                        .max_h(px(78.))
                        .gap(px(6.))
                        .children(failure_items.iter().map(|failure| {
                            let image = self.failure_surface_image(palette, failure);
                            failure_row(failure, palette, image)
                        }))
                        .overflow_y_scrollbar(),
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

fn progress_image_phase(phase: TaskPhase) -> JellyProgressImagePhase {
    match phase {
        TaskPhase::Idle => JellyProgressImagePhase::Idle,
        TaskPhase::Validating => JellyProgressImagePhase::Validating,
        TaskPhase::Running => JellyProgressImagePhase::Running,
        TaskPhase::Cancelling => JellyProgressImagePhase::Cancelling,
        TaskPhase::Completed => JellyProgressImagePhase::Completed,
        TaskPhase::Failed => JellyProgressImagePhase::Failed,
    }
}

fn lane_image_phase(phase: TaskLanePhase) -> JellyProgressImagePhase {
    match phase {
        TaskLanePhase::Pending => JellyProgressImagePhase::Idle,
        TaskLanePhase::Discovering => JellyProgressImagePhase::Validating,
        TaskLanePhase::Running => JellyProgressImagePhase::Running,
        TaskLanePhase::Cancelling => JellyProgressImagePhase::Cancelling,
        TaskLanePhase::Completed => JellyProgressImagePhase::Completed,
        TaskLanePhase::Failed => JellyProgressImagePhase::Failed,
    }
}

fn progress_tone(phase: TaskPhase) -> crate::gui::materials::JellyTone {
    match phase {
        TaskPhase::Idle => crate::gui::materials::JellyTone::Neutral,
        TaskPhase::Validating | TaskPhase::Cancelling => crate::gui::materials::JellyTone::Warning,
        TaskPhase::Running => crate::gui::materials::JellyTone::Primary,
        TaskPhase::Completed => crate::gui::materials::JellyTone::Success,
        TaskPhase::Failed => crate::gui::materials::JellyTone::Error,
    }
}

fn button_tone(tone: JellyActionTone) -> JellyTone {
    match tone {
        JellyActionTone::Primary => JellyTone::Primary,
        JellyActionTone::Cyan => JellyTone::Cyan,
        JellyActionTone::Warning => JellyTone::Warning,
        JellyActionTone::Neutral => JellyTone::Neutral,
    }
}

fn switch_tone(tone: JellySwitchTone) -> JellyTone {
    match tone {
        JellySwitchTone::Primary => JellyTone::Primary,
        JellySwitchTone::Cyan => JellyTone::Cyan,
        JellySwitchTone::Output => JellyTone::Output,
    }
}

fn event_tone(kind: EventKind) -> JellyTone {
    match kind {
        EventKind::System => JellyTone::Neutral,
        EventKind::Video | EventKind::Comments => JellyTone::Primary,
        EventKind::Danmaku => JellyTone::Cyan,
        EventKind::Output => JellyTone::Output,
        EventKind::Warning => JellyTone::Warning,
        EventKind::Success => JellyTone::Success,
        EventKind::Failure => JellyTone::Error,
    }
}
