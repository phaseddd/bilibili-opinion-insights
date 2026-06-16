use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::app::events::CollectionEvent;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TaskPhase {
    #[default]
    Idle,
    Validating,
    Running,
    Cancelling,
    Completed,
    Failed,
}

#[derive(Default)]
pub(crate) struct TaskState {
    pub(crate) phase: TaskPhase,
    pub(crate) progress: ProgressState,
    pub(crate) active_summary: Option<RunSummary>,
    pub(crate) validation_error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ProgressState {
    pub(crate) target_percent: f32,
    pub(crate) display_percent: f32,
    pub(crate) total_units: usize,
    pub(crate) completed_units: usize,
    pub(crate) comments_scanned: usize,
    pub(crate) comments_appended: usize,
    pub(crate) danmaku_scanned: usize,
    pub(crate) danmaku_appended: usize,
    pub(crate) danmaku_segments: usize,
    pub(crate) pulses: u64,
    comment_expected_by_bvid: HashMap<String, u64>,
    comment_scanned_by_bvid: HashMap<String, u64>,
    comment_finished_bvids: HashSet<String>,
    danmaku_segments_by_bvid: HashMap<String, u64>,
    danmaku_scanned_segments_by_bvid: HashMap<String, u64>,
    danmaku_finished_bvids: HashSet<String>,
}

pub(crate) struct RunSummary {
    pub(crate) videos: Vec<String>,
    pub(crate) output: PathBuf,
    pub(crate) collect_comments: bool,
    pub(crate) collect_danmaku: bool,
}

impl ProgressState {
    pub(crate) fn with_total_units(total_units: usize) -> Self {
        Self {
            total_units,
            ..Self::default()
        }
    }
}

impl TaskState {
    pub(crate) fn apply_collection_event(&mut self, event: &CollectionEvent) {
        match event {
            CollectionEvent::CommentScanPlanned {
                bvid,
                expected_total,
            } => {
                self.progress
                    .comment_expected_by_bvid
                    .insert(bvid.clone(), *expected_total);
                self.recalculate_progress_from_facts();
            }
            CollectionEvent::CommentBatchWritten {
                bvid,
                records_scanned,
                records_appended,
            } => {
                self.progress.comments_scanned += records_scanned;
                self.progress.comments_appended += records_appended;
                *self
                    .progress
                    .comment_scanned_by_bvid
                    .entry(bvid.clone())
                    .or_default() += *records_scanned as u64;
                self.recalculate_progress_from_facts();
            }
            CollectionEvent::CommentScanFinished { bvid } => {
                self.progress.comment_finished_bvids.insert(bvid.clone());
                self.recalculate_progress_from_facts();
            }
            CollectionEvent::DanmakuScanPlanned {
                bvid,
                total_segments,
            } => {
                self.progress
                    .danmaku_segments_by_bvid
                    .insert(bvid.clone(), *total_segments);
                self.recalculate_progress_from_facts();
            }
            CollectionEvent::DanmakuSegmentWritten {
                bvid,
                records_scanned,
                records_appended,
                ..
            } => {
                self.progress.danmaku_scanned += records_scanned;
                self.progress.danmaku_appended += records_appended;
                self.progress.danmaku_segments += 1;
                *self
                    .progress
                    .danmaku_scanned_segments_by_bvid
                    .entry(bvid.clone())
                    .or_default() += 1;
                self.recalculate_progress_from_facts();
            }
            CollectionEvent::DanmakuScanFinished { bvid } => {
                self.progress.danmaku_finished_bvids.insert(bvid.clone());
                self.recalculate_progress_from_facts();
            }
            _ => {}
        }
    }

    pub(crate) fn finish_progress_unit(&mut self) {
        self.progress.completed_units = self
            .progress
            .completed_units
            .saturating_add(1)
            .min(self.progress.total_units);
        if self.progress.total_units > 0 {
            self.progress.target_percent =
                (self.progress.completed_units as f32 / self.progress.total_units as f32 * 100.)
                    .min(99.);
            self.progress.display_percent =
                ease_towards(self.progress.display_percent, self.progress.target_percent);
        }
    }

    pub(crate) fn finish_run(&mut self, success: bool) {
        self.phase = if success {
            self.progress.target_percent = 100.;
            self.progress.display_percent = 100.;
            TaskPhase::Completed
        } else {
            TaskPhase::Failed
        };
    }

    fn recalculate_progress_from_facts(&mut self) {
        self.progress.pulses = self.progress.pulses.wrapping_add(1);

        let Some(summary) = self.active_summary.as_ref() else {
            return;
        };

        let mut job_count = 0usize;
        let mut completed_count = 0usize;
        let mut progress_sum = 0.;

        for bvid in &summary.videos {
            if summary.collect_comments {
                job_count += 1;
                if self.progress.comment_finished_bvids.contains(bvid) {
                    completed_count += 1;
                    progress_sum += 1.;
                } else {
                    let scanned = self
                        .progress
                        .comment_scanned_by_bvid
                        .get(bvid)
                        .copied()
                        .unwrap_or_default();
                    let expected = self.progress.comment_expected_by_bvid.get(bvid).copied();
                    progress_sum += progress_fraction(scanned, expected);
                }
            }

            if summary.collect_danmaku {
                job_count += 1;
                if self.progress.danmaku_finished_bvids.contains(bvid) {
                    completed_count += 1;
                    progress_sum += 1.;
                } else {
                    let scanned = self
                        .progress
                        .danmaku_scanned_segments_by_bvid
                        .get(bvid)
                        .copied()
                        .unwrap_or_default();
                    let expected = self.progress.danmaku_segments_by_bvid.get(bvid).copied();
                    progress_sum += progress_fraction(scanned, expected);
                }
            }
        }

        if job_count == 0 {
            return;
        }

        self.progress.total_units = job_count;
        self.progress.completed_units = self
            .progress
            .completed_units
            .max(completed_count)
            .min(job_count);

        let live_ceiling = if self.phase == TaskPhase::Running {
            99.
        } else {
            100.
        };
        let target = (progress_sum / job_count as f32 * 100.).min(live_ceiling);
        self.progress.target_percent = self.progress.target_percent.max(target);
        self.progress.display_percent =
            ease_towards(self.progress.display_percent, self.progress.target_percent);
    }
}

impl TaskPhase {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Idle => "待命",
            Self::Validating => "校验中",
            Self::Running => "采集中",
            Self::Cancelling => "取消中",
            Self::Completed => "已完成",
            Self::Failed => "失败",
        }
    }

    pub(crate) fn is_busy(self) -> bool {
        matches!(self, Self::Validating | Self::Running | Self::Cancelling)
    }
}

fn ease_towards(current: f32, target: f32) -> f32 {
    if target <= current {
        target
    } else {
        current + (target - current) * 0.42
    }
}

fn progress_fraction(scanned: u64, expected: Option<u64>) -> f32 {
    match expected {
        Some(0) => {
            if scanned == 0 {
                0.
            } else {
                0.95
            }
        }
        // The opening count is only a baseline. New comments may arrive while scanning.
        Some(expected) => (scanned as f32 / expected as f32).clamp(0., 0.995),
        None => {
            if scanned == 0 {
                0.
            } else {
                0.05
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::progress_fraction;

    #[test]
    fn progress_fraction_caps_when_scan_exceeds_opening_count() {
        let fraction = progress_fraction(10_050, Some(10_000));

        assert!(fraction < 1.);
        assert_eq!(fraction, 0.995);
    }

    #[test]
    fn progress_fraction_uses_opening_count_before_finish() {
        let fraction = progress_fraction(5_000, Some(10_000));

        assert_eq!(fraction, 0.5);
    }
}
