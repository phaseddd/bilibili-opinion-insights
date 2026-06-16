use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::Result;

use crate::app::collection::{
    CollectionRequest, CredentialOptions, DEFAULT_REQUEST_DELAY, run_collection_with_events,
};
use crate::app::comments::CommentOutputFormat;
use crate::gui::messages::GuiMessage;
use crate::gui::state::auth::{CredentialSource, SessionKind, SessionMode};
use crate::gui::state::results::FailureItem;

#[derive(Debug)]
pub(crate) struct CollectionDraft {
    pub(crate) bvids: Vec<String>,
    pub(crate) session: SessionMode,
    pub(crate) cookie: Option<PathBuf>,
    pub(crate) output: PathBuf,
    pub(crate) collect_comments: bool,
    pub(crate) collect_danmaku: bool,
    pub(crate) write_csv: bool,
    pub(crate) write_jsonl: bool,
}

pub(crate) fn spawn_collection_worker(draft: CollectionDraft, sender: mpsc::Sender<GuiMessage>) {
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
