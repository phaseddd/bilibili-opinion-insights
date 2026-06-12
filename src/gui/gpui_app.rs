use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

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

use crate::app::collection::{
    CollectionJobOutcome, CollectionRequest, CredentialOptions, DEFAULT_COOKIE_PATH,
    DEFAULT_OUTPUT_ROOT, DEFAULT_REQUEST_DELAY, run_collection_with_events,
};
use crate::app::comments::CommentOutputFormat;
use crate::app::events::CollectionEvent;
use crate::bili::video::normalize_bvid_input;

const EVENT_LIMIT: usize = 240;

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
    form: FormState,
    auth: AuthState,
    task: TaskState,
    events: EventState,
    results: ResultState,
    visual: VisualState,
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
    mode: AuthMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AuthMode {
    #[default]
    DefaultCookie,
    ExplicitCookie,
    Anonymous,
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
}

#[derive(Debug)]
struct CollectionDraft {
    bvids: Vec<String>,
    cookie: Option<PathBuf>,
    output: PathBuf,
    collect_comments: bool,
    collect_danmaku: bool,
    write_csv: bool,
    write_jsonl: bool,
}

enum GuiMessage {
    Event(CollectionEvent),
    Outcome(CollectionJobOutcome),
    Failure(FailureItem),
    UnitFinished,
    Finished { success: bool, message: String },
}

impl BiliOpinionGui {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let form = FormState::new(window, cx);
        let mut events = EventState::default();
        events.push(
            EventKind::System,
            "准备就绪。输入 BVID 或视频链接后开始采集。",
        );

        Self {
            form,
            auth: AuthState::default(),
            task: TaskState::default(),
            events,
            results: ResultState::default(),
            visual: VisualState::default(),
        }
    }

    fn build_draft(&self, cx: &App) -> Result<CollectionDraft, String> {
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
            cookie,
            output,
            collect_comments: self.form.collect_comments,
            collect_danmaku: self.form.collect_danmaku,
            write_csv: self.form.write_csv,
            write_jsonl: self.form.write_jsonl,
        })
    }

    fn start_collection(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
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
            auth_event_kind(draft.cookie.as_ref()),
            format!("登录凭据：{}", auth_label(draft.cookie.as_ref())),
        );
        self.auth.mode = auth_mode_for_draft(draft.cookie.as_ref());

        spawn_collection_worker(draft, sender);
        spawn_message_pump(receiver, cx);
        cx.notify();
    }

    fn apply_message(&mut self, message: GuiMessage, cx: &mut Context<Self>) {
        match message {
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
        cx.notify();
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
            while let Ok(message) = receiver.try_recv() {
                finished = matches!(message, GuiMessage::Finished { .. });
                if view
                    .update(cx, |view, cx| view.apply_message(message, cx))
                    .is_err()
                {
                    return;
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

async fn run_collection(draft: CollectionDraft, sender: mpsc::Sender<GuiMessage>) -> Result<()> {
    let comment_formats = comment_formats(&draft);
    let request = CollectionRequest {
        bvids: draft.bvids,
        credentials: CredentialOptions {
            cookie: draft.cookie,
            sessdata: None,
            anonymous: false,
        },
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

        v_flex()
            .size_full()
            .bg(palette.app_bg)
            .text_color(palette.text)
            .font_family("Microsoft YaHei UI")
            .child(self.render_header(&palette, can_start, cx))
            .child(
                h_flex()
                    .size_full()
                    .items_start()
                    .gap(px(16.))
                    .p(px(18.))
                    .child(self.render_collection_panel(&palette, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .gap(px(14.))
                            .child(self.render_progress_panel(&palette))
                            .child(self.render_event_panel(&palette))
                            .child(self.render_result_panel(&palette)),
                    ),
            )
    }
}

impl BiliOpinionGui {
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
                    .child(
                        header_action_button("清空", HeaderActionKind::Ghost, can_start, palette)
                            .id("clear-log-action")
                            .when(can_start, |this| {
                                this.cursor_pointer().on_click(cx.listener(Self::clear_log))
                            }),
                    )
                    .child(
                        header_action_button(
                            "取消",
                            HeaderActionKind::Outline,
                            self.task.phase == TaskPhase::Running,
                            palette,
                        )
                        .id("cancel-collection-action")
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
                            HeaderActionKind::Primary,
                            can_start,
                            palette,
                        )
                        .id("start-collection-action")
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
                "后续会接入原生文件选择器和扫码登录。",
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
                    .child(status_badge("扫码待接入", EventKind::Warning, palette)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .line_height(relative(1.25))
                    .child(auth_mode_copy(self.auth.mode)),
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

fn header_action_button(
    label: &'static str,
    kind: HeaderActionKind,
    enabled: bool,
    palette: &Palette,
) -> gpui::Div {
    let (bg, border, text) = match kind {
        HeaderActionKind::Ghost => (
            palette.surface_soft,
            palette.border,
            if enabled { palette.text } else { palette.muted },
        ),
        HeaderActionKind::Outline => (
            palette.warning.opacity(0.06),
            palette.warning.opacity(0.28),
            palette.warning,
        ),
        HeaderActionKind::Primary => (
            palette.accent,
            palette.accent,
            hsla(0., 0., 1., if enabled { 1.0 } else { 0.7 }),
        ),
    };

    div()
        .flex_shrink_0()
        .min_w(px(78.))
        .h(px(36.))
        .px(px(14.))
        .rounded(px(999.))
        .border_1()
        .border_color(border.opacity(if enabled { 1.0 } else { 0.45 }))
        .bg(bg.opacity(if enabled { 1.0 } else { 0.45 }))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text)
                .child(label),
        )
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

fn auth_event_kind(cookie: Option<&PathBuf>) -> EventKind {
    if cookie.is_some() {
        EventKind::Output
    } else {
        EventKind::Warning
    }
}

fn auth_mode_for_draft(cookie: Option<&PathBuf>) -> AuthMode {
    match cookie {
        Some(_) => AuthMode::ExplicitCookie,
        None if Path::new(DEFAULT_COOKIE_PATH).exists() => AuthMode::DefaultCookie,
        None => AuthMode::Anonymous,
    }
}

fn auth_label(cookie: Option<&PathBuf>) -> String {
    cookie
        .map(|path| format!("显式 cookie 文件 {}", path.display()))
        .unwrap_or_else(|| format!("默认 cookie 文件或匿名模式（{DEFAULT_COOKIE_PATH}）"))
}

fn auth_mode_copy(mode: AuthMode) -> &'static str {
    match mode {
        AuthMode::DefaultCookie => "默认读取 config/bilibili-cookie.txt；不存在时按匿名请求。",
        AuthMode::ExplicitCookie => "使用当前填写的 Cookie 文件。",
        AuthMode::Anonymous => "匿名请求；登录态功能后续接入。",
    }
}
