use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use gpui::{
    App, AppContext as _, Application, Bounds, ClickEvent, Context, Entity, IntoElement,
    ParentElement, Render, SharedString, Styled as _, Window, WindowBounds, WindowOptions, div, px,
    rgb, size,
};
use gpui_component::{
    Disableable as _, Root,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputState},
    progress::Progress,
    v_flex,
};

use crate::app::collection::{
    CollectionJobOutcome, CollectionRequest, CredentialOptions, DEFAULT_COOKIE_PATH,
    DEFAULT_OUTPUT_ROOT, DEFAULT_REQUEST_DELAY, run_collection_with_events,
};
use crate::app::comments::CommentOutputFormat;
use crate::app::events::CollectionEvent;
use crate::bili::video::normalize_bvid_input;

pub fn run() {
    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(1080.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
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
    bvids_input: Entity<InputState>,
    cookie_input: Entity<InputState>,
    output_input: Entity<InputState>,
    collect_comments: bool,
    collect_danmaku: bool,
    write_csv: bool,
    write_jsonl: bool,
    running: bool,
    progress_percent: f32,
    total_units: usize,
    completed_units: usize,
    log_lines: Vec<String>,
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
    Log(String),
    UnitFinished,
    Finished { success: bool, message: String },
}

impl BiliOpinionGui {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let bvids_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(4)
                .placeholder("BVID or video URL list")
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
            running: false,
            progress_percent: 0.,
            total_units: 0,
            completed_units: 0,
            log_lines: vec!["ready".to_string()],
        }
    }

    fn build_draft(&self, cx: &App) -> Result<CollectionDraft, String> {
        let mut bvids = Vec::new();
        for value in self.bvids_input.read(cx).value().lines() {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }

            let bvid = normalize_bvid_input(value)
                .ok_or_else(|| format!("could not find a BVID in input: {value}"))?;
            bvids.push(bvid);
        }

        if bvids.is_empty() {
            return Err("provide at least one BVID".to_string());
        }
        if !self.collect_comments && !self.collect_danmaku {
            return Err("enable comments, danmaku, or both".to_string());
        }
        if self.collect_comments && !self.write_csv && !self.write_jsonl {
            return Err("enable at least one comment output format".to_string());
        }

        let cookie = trimmed_input(&self.cookie_input, cx).map(PathBuf::from);
        let output = trimmed_input(&self.output_input, cx)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT_ROOT));

        Ok(CollectionDraft {
            bvids,
            cookie,
            output,
            collect_comments: self.collect_comments,
            collect_danmaku: self.collect_danmaku,
            write_csv: self.write_csv,
            write_jsonl: self.write_jsonl,
        })
    }

    fn start_collection(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        match self.build_draft(cx) {
            Ok(draft) => {
                let total_units = draft.bvids.len() * usize::from(draft.collect_comments)
                    + draft.bvids.len() * usize::from(draft.collect_danmaku);
                let (sender, receiver) = mpsc::channel();

                self.running = true;
                self.progress_percent = 0.;
                self.total_units = total_units;
                self.completed_units = 0;
                self.log_lines.clear();
                self.log_lines
                    .push(format!("videos queued: {}", draft.bvids.len()));
                self.log_lines
                    .push(format!("videos: {}", format_bvid_preview(&draft.bvids)));
                self.log_lines.push(format!(
                    "collect: comments={}, danmaku={}",
                    draft.collect_comments, draft.collect_danmaku
                ));
                self.log_lines.push(format!(
                    "comment formats: csv={}, jsonl={}",
                    draft.write_csv, draft.write_jsonl
                ));
                self.log_lines
                    .push(format!("output: {}", draft.output.display()));
                self.log_lines.push(format!(
                    "cookie: {}",
                    draft
                        .cookie
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "<anonymous>".to_string())
                ));
                self.log_lines.push("collector started".to_string());
                spawn_collection_worker(draft, sender);
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
                            .timer(Duration::from_millis(100))
                            .await;
                    }
                })
                .detach();
            }
            Err(message) => {
                self.progress_percent = 0.;
                self.log_lines.push(format!("validation: {message}"));
            }
        }
        cx.notify();
    }

    fn apply_message(&mut self, message: GuiMessage, cx: &mut Context<Self>) {
        match message {
            GuiMessage::Event(event) => self.log_lines.push(format_collection_event(&event)),
            GuiMessage::Log(line) => self.log_lines.push(line),
            GuiMessage::UnitFinished => {
                self.completed_units = self.completed_units.saturating_add(1);
                if self.total_units > 0 {
                    self.progress_percent =
                        (self.completed_units as f32 / self.total_units as f32 * 100.).min(100.);
                }
            }
            GuiMessage::Finished { success, message } => {
                self.running = false;
                if success {
                    self.progress_percent = 100.;
                }
                self.log_lines.push(message);
            }
        }
        cx.notify();
    }

    fn set_collect_comments(&mut self, checked: &bool, _: &mut Window, cx: &mut Context<Self>) {
        self.collect_comments = *checked;
        cx.notify();
    }

    fn set_collect_danmaku(&mut self, checked: &bool, _: &mut Window, cx: &mut Context<Self>) {
        self.collect_danmaku = *checked;
        cx.notify();
    }

    fn set_write_csv(&mut self, checked: &bool, _: &mut Window, cx: &mut Context<Self>) {
        self.write_csv = *checked;
        cx.notify();
    }

    fn set_write_jsonl(&mut self, checked: &bool, _: &mut Window, cx: &mut Context<Self>) {
        self.write_jsonl = *checked;
        cx.notify();
    }

    fn clear_log(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.log_lines.clear();
        self.progress_percent = 0.;
        cx.notify();
    }
}

fn spawn_collection_worker(draft: CollectionDraft, sender: mpsc::Sender<GuiMessage>) {
    std::thread::spawn(move || {
        let result = run_collection_blocking(draft, sender.clone());
        match result {
            Ok(()) => {
                let _ = sender.send(GuiMessage::Finished {
                    success: true,
                    message: "collector finished".to_string(),
                });
            }
            Err(error) => {
                let _ = sender.send(GuiMessage::Finished {
                    success: false,
                    message: format!("collector failed: {error}"),
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
        let _ = sender.send(GuiMessage::Log(format_collection_job(job)));
        let _ = sender.send(GuiMessage::UnitFinished);
    }

    for failure in &outcome.failures {
        let _ = sender.send(GuiMessage::Log(format!(
            "{} failed: {}: {}",
            failure.kind.as_str(),
            failure.bvid,
            failure.error
        )));
        let _ = sender.send(GuiMessage::UnitFinished);
    }

    outcome.ensure_success()
}

fn format_collection_job(job: &CollectionJobOutcome) -> String {
    match job {
        CollectionJobOutcome::Comments(outcome) => {
            format!(
                "comments done: {} scanned={}, appended={}",
                outcome.bvid, outcome.summary.comments_scanned, outcome.appended_count
            )
        }
        CollectionJobOutcome::Danmaku(outcome) => format!(
            "danmaku done: {} scanned={}, appended={}, segments={}",
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

fn format_collection_event(event: &CollectionEvent) -> String {
    match event {
        CollectionEvent::VideoStarted { bvid } => format!("video started: {bvid}"),
        CollectionEvent::OutputInitialized { bvid, path } => {
            format!("output initialized: {bvid}: {}", path.display())
        }
        CollectionEvent::CommentBatchWritten {
            bvid,
            records_scanned,
            records_appended,
        } => {
            format!("comments batch: {bvid} scanned={records_scanned}, appended={records_appended}")
        }
        CollectionEvent::DanmakuSegmentWritten {
            bvid,
            cid,
            page,
            segment_index,
            records_scanned,
            records_appended,
            segment_appended,
        } => format!(
            "danmaku segment: {bvid} cid={cid}, page={page}, segment={segment_index}, scanned={records_scanned}, appended={records_appended}, metadata={segment_appended}"
        ),
        CollectionEvent::VideoFinished { bvid } => format!("video finished: {bvid}"),
    }
}

impl Render for BiliOpinionGui {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_start = !self.running;

        v_flex()
            .size_full()
            .bg(rgb(0xf8fafc))
            .text_color(rgb(0x111827))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .px(px(24.))
                    .py(px(18.))
                    .border_b_1()
                    .border_color(rgb(0xd8dee9))
                    .bg(rgb(0xffffff))
                    .child(
                        v_flex()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Bilibili Opinion Insights"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(0x4b5563))
                                    .child("Rust-native GPUI collector"),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(
                                Button::new("clear-log")
                                    .label("Clear")
                                    .disabled(self.running)
                                    .on_click(cx.listener(Self::clear_log)),
                            )
                            .child(
                                Button::new("start-collection")
                                    .primary()
                                    .label(if self.running { "Running" } else { "Start" })
                                    .loading(self.running)
                                    .disabled(!can_start)
                                    .on_click(cx.listener(Self::start_collection)),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .size_full()
                    .items_start()
                    .gap(px(18.))
                    .p(px(18.))
                    .child(
                        v_flex()
                            .w(px(420.))
                            .gap(px(14.))
                            .child(form_section(
                                "Videos",
                                Input::new(&self.bvids_input).h(px(116.)),
                            ))
                            .child(form_section(
                                "Cookie file",
                                Input::new(&self.cookie_input).cleanable(true),
                            ))
                            .child(form_section(
                                "Output",
                                Input::new(&self.output_input).cleanable(true),
                            ))
                            .child(
                                v_flex().gap(px(8.)).child(section_label("Collect")).child(
                                    h_flex()
                                        .gap(px(14.))
                                        .child(
                                            Checkbox::new("collect-comments")
                                                .label("Comments")
                                                .checked(self.collect_comments)
                                                .on_click(cx.listener(Self::set_collect_comments)),
                                        )
                                        .child(
                                            Checkbox::new("collect-danmaku")
                                                .label("Danmaku")
                                                .checked(self.collect_danmaku)
                                                .on_click(cx.listener(Self::set_collect_danmaku)),
                                        ),
                                ),
                            )
                            .child(
                                v_flex()
                                    .gap(px(8.))
                                    .child(section_label("Comment output"))
                                    .child(
                                        h_flex()
                                            .gap(px(14.))
                                            .child(
                                                Checkbox::new("write-csv")
                                                    .label("CSV")
                                                    .checked(self.write_csv)
                                                    .on_click(cx.listener(Self::set_write_csv)),
                                            )
                                            .child(
                                                Checkbox::new("write-jsonl")
                                                    .label("JSONL")
                                                    .checked(self.write_jsonl)
                                                    .on_click(cx.listener(Self::set_write_jsonl)),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(360.))
                            .gap(px(12.))
                            .child(
                                v_flex()
                                    .gap(px(8.))
                                    .child(section_label("Progress"))
                                    .child(Progress::new().value(self.progress_percent)),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap(px(8.))
                                    .child(section_label("Events"))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_h(px(420.))
                                            .gap(px(6.))
                                            .p(px(12.))
                                            .rounded(px(6.))
                                            .border_1()
                                            .border_color(rgb(0xd8dee9))
                                            .bg(rgb(0xffffff))
                                            .children(self.log_lines.iter().rev().map(|line| {
                                                div()
                                                    .w_full()
                                                    .text_size(px(12.))
                                                    .text_color(rgb(0x111827))
                                                    .child(SharedString::from(line.clone()))
                                            })),
                                    ),
                            ),
                    ),
            )
    }
}

fn trimmed_input(input: &Entity<InputState>, cx: &App) -> Option<String> {
    let value = input.read(cx).value().trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn format_bvid_preview(bvids: &[String]) -> String {
    const PREVIEW_LIMIT: usize = 5;

    let mut preview = bvids
        .iter()
        .take(PREVIEW_LIMIT)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");

    if bvids.len() > PREVIEW_LIMIT {
        preview.push_str(&format!(", ... +{} more", bvids.len() - PREVIEW_LIMIT));
    }

    preview
}

fn form_section(label: &'static str, input: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap(px(8.))
        .child(section_label(label))
        .child(input)
}

fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(0x374151))
        .child(label)
}
