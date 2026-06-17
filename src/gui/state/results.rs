use std::path::PathBuf;

use crate::app::collection::CollectionJobOutcome;

#[derive(Default)]
pub(crate) struct ResultState {
    pub(crate) jobs: Vec<ResultItem>,
    pub(crate) failures: Vec<FailureItem>,
    pub(crate) output_root: Option<PathBuf>,
}

#[derive(Clone)]
pub(crate) struct ResultItem {
    pub(crate) kind: ResultKind,
    pub(crate) bvid: String,
    pub(crate) scanned: usize,
    pub(crate) appended: usize,
    pub(crate) extra: String,
    pub(crate) outputs: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResultKind {
    Comments,
    Danmaku,
}

#[derive(Clone)]
pub(crate) struct FailureItem {
    pub(crate) kind: String,
    pub(crate) bvid: String,
    pub(crate) error: String,
}

impl ResultItem {
    pub(crate) fn from_job(job: &CollectionJobOutcome) -> Self {
        match job {
            CollectionJobOutcome::Comments(outcome) => Self {
                kind: ResultKind::Comments,
                bvid: outcome.bvid.clone(),
                scanned: outcome.summary.comments_scanned,
                appended: outcome.appended_count,
                extra: format!(
                    "主评论页 {}，二级评论页 {}，预计总数 {}",
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
            CollectionJobOutcome::Danmaku(outcome) => Self {
                kind: ResultKind::Danmaku,
                bvid: outcome.bvid.clone(),
                scanned: outcome.records_scanned,
                appended: outcome.records_appended,
                extra: format!(
                    "弹幕分包 {}，新增分包 {}",
                    outcome.segments_scanned, outcome.segments_appended
                ),
                outputs: vec![
                    outcome.record_path.clone(),
                    outcome.segment_metadata_path.clone(),
                ],
            },
        }
    }
}

pub(crate) fn format_collection_job(job: &CollectionJobOutcome) -> String {
    match job {
        CollectionJobOutcome::Comments(outcome) => {
            format!(
                "评论完成：{}，已处理 {}，新增 {}",
                outcome.bvid, outcome.summary.comments_scanned, outcome.appended_count
            )
        }
        CollectionJobOutcome::Danmaku(outcome) => format!(
            "弹幕完成：{}，已处理 {}，新增 {}，分包 {}",
            outcome.bvid,
            outcome.records_scanned,
            outcome.records_appended,
            outcome.segments_scanned
        ),
    }
}
