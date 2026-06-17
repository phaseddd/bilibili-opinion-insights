use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::app::events::CollectionEvent;
use crate::gui::motion::{
    JellyProgressMotionSnapshot, JellyProgressMotionState, ProgressMotionPhase, VISUAL_MOTION_DT,
};

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
    pub(crate) lanes: Vec<TaskLane>,
    pub(crate) active_summary: Option<RunSummary>,
    pub(crate) validation_error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ProgressState {
    pub(crate) target_percent: f32,
    pub(crate) display_percent: f32,
    pub(crate) motion: JellyProgressMotionState,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskLaneKind {
    Comments,
    Danmaku,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskLanePhase {
    Pending,
    Discovering,
    Running,
    Completed,
    Failed,
    Cancelling,
}

pub(crate) struct TaskLane {
    pub(crate) bvid: String,
    pub(crate) kind: TaskLaneKind,
    pub(crate) phase: TaskLanePhase,
    pub(crate) target_percent: f32,
    pub(crate) display_percent: f32,
    pub(crate) expected_total: Option<u64>,
    pub(crate) scanned: u64,
    pub(crate) appended: u64,
    pub(crate) output_ready: bool,
    motion: JellyProgressMotionState,
}

impl ProgressState {
    pub(crate) fn with_total_units(total_units: usize) -> Self {
        let mut progress = Self {
            total_units,
            ..Self::default()
        };
        progress.reset_visual_percent(0.);
        progress
    }

    pub(crate) fn set_target_percent(&mut self, target_percent: f32) {
        let target_percent = target_percent.clamp(0., 100.);
        self.target_percent = target_percent;
        self.motion.set_target_percent(target_percent);
    }

    pub(crate) fn reset_visual_percent(&mut self, percent: f32) {
        let percent = percent.clamp(0., 100.);
        self.target_percent = percent;
        self.display_percent = percent;
        self.motion.reset_to(percent);
    }

    pub(crate) fn tick_visual_motion(&mut self, phase: TaskPhase) -> bool {
        self.motion.set_target_percent(self.target_percent);
        let active = self
            .motion
            .tick(progress_motion_phase(phase), VISUAL_MOTION_DT);
        self.display_percent = self.motion.display_percent();
        active || self.has_active_visual_motion(phase)
    }

    pub(crate) fn has_active_visual_motion(&self, phase: TaskPhase) -> bool {
        self.motion.is_active(progress_motion_phase(phase))
            || (self.display_percent - self.target_percent).abs() > 0.02
    }

    pub(crate) fn motion_snapshot(
        &self,
        phase: TaskPhase,
        motion_tick: u64,
    ) -> JellyProgressMotionSnapshot {
        self.motion
            .snapshot(motion_tick, progress_motion_phase(phase))
    }
}

impl TaskState {
    pub(crate) fn begin_run(&mut self, summary: RunSummary, total_units: usize) {
        let lanes = TaskLane::from_summary(&summary);
        self.phase = TaskPhase::Running;
        self.progress = ProgressState::with_total_units(total_units);
        self.lanes = lanes;
        self.active_summary = Some(summary);
        self.validation_error = None;
    }

    pub(crate) fn apply_collection_event(&mut self, event: &CollectionEvent) {
        match event {
            CollectionEvent::VideoStarted { bvid } => {
                self.update_lanes_for_bvid(bvid, |lane| {
                    if lane.phase == TaskLanePhase::Pending {
                        lane.mark_discovering();
                    }
                });
            }
            CollectionEvent::OutputInitialized { bvid, .. } => {
                self.update_lanes_for_bvid(bvid, |lane| {
                    lane.output_ready = true;
                    if lane.phase == TaskLanePhase::Pending {
                        lane.mark_discovering();
                    }
                });
            }
            CollectionEvent::CommentScanPlanned {
                bvid,
                expected_total,
            } => {
                self.progress
                    .comment_expected_by_bvid
                    .insert(bvid.clone(), *expected_total);
                self.update_lane(bvid, TaskLaneKind::Comments, |lane| {
                    lane.expected_total = Some(*expected_total);
                    lane.mark_running();
                    lane.set_target_percent(if *expected_total == 0 { 3. } else { 1.5 });
                });
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
                let expected = self.progress.comment_expected_by_bvid.get(bvid).copied();
                self.update_lane(bvid, TaskLaneKind::Comments, |lane| {
                    lane.mark_running();
                    lane.scanned = lane.scanned.saturating_add(*records_scanned as u64);
                    lane.appended = lane.appended.saturating_add(*records_appended as u64);
                    lane.expected_total = expected.or(lane.expected_total);
                    lane.set_target_percent(
                        (progress_fraction(lane.scanned, lane.expected_total) * 100.).min(99.),
                    );
                });
                self.recalculate_progress_from_facts();
            }
            CollectionEvent::CommentScanFinished { bvid } => {
                self.progress.comment_finished_bvids.insert(bvid.clone());
                self.update_lane(bvid, TaskLaneKind::Comments, |lane| {
                    lane.mark_completed();
                });
                self.recalculate_progress_from_facts();
            }
            CollectionEvent::DanmakuScanPlanned {
                bvid,
                total_segments,
            } => {
                self.progress
                    .danmaku_segments_by_bvid
                    .insert(bvid.clone(), *total_segments);
                self.update_lane(bvid, TaskLaneKind::Danmaku, |lane| {
                    lane.expected_total = Some(*total_segments);
                    lane.mark_running();
                    lane.set_target_percent(if *total_segments == 0 { 3. } else { 1.5 });
                });
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
                let expected = self.progress.danmaku_segments_by_bvid.get(bvid).copied();
                self.update_lane(bvid, TaskLaneKind::Danmaku, |lane| {
                    lane.mark_running();
                    lane.scanned = lane.scanned.saturating_add(1);
                    lane.appended = lane.appended.saturating_add(*records_appended as u64);
                    lane.expected_total = expected.or(lane.expected_total);
                    lane.set_target_percent(
                        (progress_fraction(lane.scanned, lane.expected_total) * 100.).min(99.),
                    );
                });
                self.recalculate_progress_from_facts();
            }
            CollectionEvent::DanmakuScanFinished { bvid } => {
                self.progress.danmaku_finished_bvids.insert(bvid.clone());
                self.update_lane(bvid, TaskLaneKind::Danmaku, |lane| {
                    lane.mark_completed();
                });
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
            self.progress.set_target_percent(
                (self.progress.completed_units as f32 / self.progress.total_units as f32 * 100.)
                    .min(99.),
            );
            self.progress
                .motion
                .trigger_phase_pulse(progress_motion_phase(self.phase));
        }
    }

    pub(crate) fn finish_run(&mut self, success: bool) {
        self.phase = if success {
            self.progress.set_target_percent(100.);
            self.progress
                .motion
                .trigger_phase_pulse(ProgressMotionPhase::Completed);
            for lane in &mut self.lanes {
                if !matches!(lane.phase, TaskLanePhase::Completed | TaskLanePhase::Failed) {
                    lane.mark_completed();
                }
            }
            TaskPhase::Completed
        } else {
            self.progress
                .motion
                .trigger_phase_pulse(ProgressMotionPhase::Failed);
            for lane in &mut self.lanes {
                if !matches!(lane.phase, TaskLanePhase::Completed | TaskLanePhase::Failed) {
                    lane.mark_failed();
                }
            }
            TaskPhase::Failed
        };
    }

    pub(crate) fn request_cancel_visual(&mut self) {
        self.phase = TaskPhase::Cancelling;
        self.progress
            .motion
            .trigger_phase_pulse(ProgressMotionPhase::Cancelling);
        for lane in &mut self.lanes {
            if matches!(
                lane.phase,
                TaskLanePhase::Pending | TaskLanePhase::Running | TaskLanePhase::Discovering
            ) {
                lane.mark_cancelling();
            }
        }
    }

    pub(crate) fn clear_idle_progress(&mut self) {
        self.phase = TaskPhase::Idle;
        self.progress = ProgressState::default();
        self.lanes.clear();
        self.active_summary = None;
        self.validation_error = None;
    }

    pub(crate) fn mark_failure(&mut self, bvid: &str, kind: &str) {
        let lane_kind = match kind {
            "comments" => Some(TaskLaneKind::Comments),
            "danmaku" => Some(TaskLaneKind::Danmaku),
            _ => None,
        };

        if let Some(lane_kind) = lane_kind {
            self.update_lane(bvid, lane_kind, |lane| {
                lane.mark_failed();
            });
        }
    }

    pub(crate) fn tick_visual_motion(&mut self) -> bool {
        let mut active = self.progress.tick_visual_motion(self.phase);
        for lane in &mut self.lanes {
            active |= lane.tick_visual_motion();
        }
        active
    }

    pub(crate) fn has_active_visual_motion(&self) -> bool {
        self.progress.has_active_visual_motion(self.phase)
            || self.lanes.iter().any(TaskLane::has_active_visual_motion)
    }

    fn update_lane(
        &mut self,
        bvid: &str,
        kind: TaskLaneKind,
        mut update: impl FnMut(&mut TaskLane),
    ) {
        if let Some(lane) = self
            .lanes
            .iter_mut()
            .find(|lane| lane.bvid == bvid && lane.kind == kind)
        {
            update(lane);
        }
    }

    fn update_lanes_for_bvid(&mut self, bvid: &str, mut update: impl FnMut(&mut TaskLane)) {
        for lane in self.lanes.iter_mut().filter(|lane| lane.bvid == bvid) {
            update(lane);
        }
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
        self.progress
            .set_target_percent(self.progress.target_percent.max(target));
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

impl TaskLane {
    fn from_summary(summary: &RunSummary) -> Vec<Self> {
        let mut lanes = Vec::new();
        for bvid in &summary.videos {
            if summary.collect_comments {
                lanes.push(Self::new(bvid.clone(), TaskLaneKind::Comments));
            }
            if summary.collect_danmaku {
                lanes.push(Self::new(bvid.clone(), TaskLaneKind::Danmaku));
            }
        }
        lanes
    }

    fn new(bvid: String, kind: TaskLaneKind) -> Self {
        let mut motion = JellyProgressMotionState::default();
        motion.reset_to(0.);
        Self {
            bvid,
            kind,
            phase: TaskLanePhase::Pending,
            target_percent: 0.,
            display_percent: 0.,
            expected_total: None,
            scanned: 0,
            appended: 0,
            output_ready: false,
            motion,
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self.kind {
            TaskLaneKind::Comments => "评论",
            TaskLaneKind::Danmaku => "弹幕",
        }
    }

    pub(crate) fn phase_label(&self) -> &'static str {
        match self.phase {
            TaskLanePhase::Pending => "等待",
            TaskLanePhase::Discovering => "发现中",
            TaskLanePhase::Running => "运行中",
            TaskLanePhase::Completed => "完成",
            TaskLanePhase::Failed => "失败",
            TaskLanePhase::Cancelling => "取消中",
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self.kind {
            TaskLaneKind::Comments => format!(
                "已处理 {} 条 · 新增 {} 条 · 预计总数 {}{}",
                self.scanned,
                self.appended,
                expected_label(self.expected_total, "条"),
                output_label(self.output_ready)
            ),
            TaskLaneKind::Danmaku => format!(
                "弹幕分包 {} / {} · 新增 {} 条{}",
                self.scanned,
                expected_label(self.expected_total, "段"),
                self.appended,
                output_label(self.output_ready)
            ),
        }
    }

    pub(crate) fn motion_snapshot(&self, motion_tick: u64) -> JellyProgressMotionSnapshot {
        self.motion
            .snapshot(motion_tick, task_lane_motion_phase(self.phase))
    }

    fn set_target_percent(&mut self, percent: f32) {
        let percent = percent.clamp(0., 100.);
        self.target_percent = percent;
        self.motion.set_target_percent(percent);
    }

    fn mark_discovering(&mut self) {
        if self.phase == TaskLanePhase::Pending {
            self.phase = TaskLanePhase::Discovering;
            self.motion
                .trigger_phase_pulse(ProgressMotionPhase::Validating);
        }
    }

    fn mark_running(&mut self) {
        if !matches!(
            self.phase,
            TaskLanePhase::Running | TaskLanePhase::Completed
        ) {
            self.phase = TaskLanePhase::Running;
        }
        self.motion
            .trigger_phase_pulse(ProgressMotionPhase::Running);
    }

    fn mark_completed(&mut self) {
        self.phase = TaskLanePhase::Completed;
        self.set_target_percent(100.);
        self.motion
            .trigger_phase_pulse(ProgressMotionPhase::Completed);
    }

    fn mark_failed(&mut self) {
        self.phase = TaskLanePhase::Failed;
        self.motion.trigger_phase_pulse(ProgressMotionPhase::Failed);
    }

    fn mark_cancelling(&mut self) {
        self.phase = TaskLanePhase::Cancelling;
        self.motion
            .trigger_phase_pulse(ProgressMotionPhase::Cancelling);
    }

    fn tick_visual_motion(&mut self) -> bool {
        self.motion.set_target_percent(self.target_percent);
        let active = self
            .motion
            .tick(task_lane_motion_phase(self.phase), VISUAL_MOTION_DT);
        self.display_percent = self.motion.display_percent();
        active || self.has_active_visual_motion()
    }

    fn has_active_visual_motion(&self) -> bool {
        self.motion.is_active(task_lane_motion_phase(self.phase))
            || (self.display_percent - self.target_percent).abs() > 0.02
    }
}

impl TaskLaneKind {
    pub(crate) fn event_kind(self) -> crate::gui::state::events::EventKind {
        match self {
            Self::Comments => crate::gui::state::events::EventKind::Comments,
            Self::Danmaku => crate::gui::state::events::EventKind::Danmaku,
        }
    }
}

fn progress_motion_phase(phase: TaskPhase) -> ProgressMotionPhase {
    match phase {
        TaskPhase::Idle => ProgressMotionPhase::Idle,
        TaskPhase::Validating => ProgressMotionPhase::Validating,
        TaskPhase::Running => ProgressMotionPhase::Running,
        TaskPhase::Cancelling => ProgressMotionPhase::Cancelling,
        TaskPhase::Completed => ProgressMotionPhase::Completed,
        TaskPhase::Failed => ProgressMotionPhase::Failed,
    }
}

fn task_lane_motion_phase(phase: TaskLanePhase) -> ProgressMotionPhase {
    match phase {
        TaskLanePhase::Pending => ProgressMotionPhase::Idle,
        TaskLanePhase::Discovering => ProgressMotionPhase::Validating,
        TaskLanePhase::Running => ProgressMotionPhase::Running,
        TaskLanePhase::Completed => ProgressMotionPhase::Completed,
        TaskLanePhase::Failed => ProgressMotionPhase::Failed,
        TaskLanePhase::Cancelling => ProgressMotionPhase::Cancelling,
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

fn expected_label(expected: Option<u64>, unit: &str) -> String {
    expected
        .map(|value| format!("{value}{unit}"))
        .unwrap_or_else(|| "发现中".to_string())
}

fn output_label(output_ready: bool) -> &'static str {
    if output_ready {
        " · 输出已就绪"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use crate::app::events::CollectionEvent;

    use super::{
        ProgressState, RunSummary, TaskLaneKind, TaskLanePhase, TaskPhase, TaskState,
        progress_fraction,
    };

    fn one_video_summary() -> RunSummary {
        RunSummary {
            videos: vec!["BVTEST".to_string()],
            output: "output".into(),
            collect_comments: true,
            collect_danmaku: true,
        }
    }

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

    #[test]
    fn progress_visual_motion_follows_target_instead_of_jumping() {
        let mut progress = ProgressState::with_total_units(1);

        progress.set_target_percent(100.);
        assert_eq!(progress.display_percent, 0.);

        progress.tick_visual_motion(TaskPhase::Running);

        assert!(progress.display_percent > 0.);
        assert!(progress.display_percent < 100.);
    }

    #[test]
    fn failed_run_preserves_real_progress_target() {
        let mut task = TaskState {
            progress: ProgressState::with_total_units(1),
            ..TaskState::default()
        };

        task.progress.set_target_percent(42.);
        task.finish_run(false);

        assert_eq!(task.phase, TaskPhase::Failed);
        assert_eq!(task.progress.target_percent, 42.);
    }

    #[test]
    fn begin_run_creates_comment_and_danmaku_lanes() {
        let mut task = TaskState::default();

        task.begin_run(one_video_summary(), 2);

        assert_eq!(task.lanes.len(), 2);
        assert!(
            task.lanes
                .iter()
                .any(|lane| lane.kind == TaskLaneKind::Comments)
        );
        assert!(
            task.lanes
                .iter()
                .any(|lane| lane.kind == TaskLaneKind::Danmaku)
        );
    }

    #[test]
    fn lane_progress_uses_real_event_facts_without_jumping_display() {
        let mut task = TaskState::default();
        task.begin_run(one_video_summary(), 2);

        task.apply_collection_event(&CollectionEvent::CommentScanPlanned {
            bvid: "BVTEST".to_string(),
            expected_total: 100,
        });
        task.apply_collection_event(&CollectionEvent::CommentBatchWritten {
            bvid: "BVTEST".to_string(),
            records_scanned: 40,
            records_appended: 12,
        });
        task.tick_visual_motion();

        let lane = task
            .lanes
            .iter()
            .find(|lane| lane.kind == TaskLaneKind::Comments)
            .expect("comment lane");
        assert_eq!(lane.phase, TaskLanePhase::Running);
        assert_eq!(lane.target_percent, 40.);
        assert!(lane.display_percent > 0.);
        assert!(lane.display_percent < 40.);
    }

    #[test]
    fn lane_failure_preserves_real_target() {
        let mut task = TaskState::default();
        task.begin_run(one_video_summary(), 2);
        task.apply_collection_event(&CollectionEvent::CommentBatchWritten {
            bvid: "BVTEST".to_string(),
            records_scanned: 10,
            records_appended: 3,
        });

        task.mark_failure("BVTEST", "comments");

        let lane = task
            .lanes
            .iter()
            .find(|lane| lane.kind == TaskLaneKind::Comments)
            .expect("comment lane");
        assert_eq!(lane.phase, TaskLanePhase::Failed);
        assert!(lane.target_percent < 100.);
    }

    #[test]
    fn clear_idle_progress_resets_completed_phase_and_validation() {
        let mut task = TaskState::default();
        task.begin_run(one_video_summary(), 2);
        task.validation_error = Some("旧错误".to_string());
        task.finish_run(true);

        task.clear_idle_progress();

        assert_eq!(task.phase, TaskPhase::Idle);
        assert_eq!(task.progress.target_percent, 0.);
        assert!(task.lanes.is_empty());
        assert!(task.active_summary.is_none());
        assert!(task.validation_error.is_none());
    }
}
