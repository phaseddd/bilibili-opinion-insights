use std::collections::VecDeque;

use crate::app::events::CollectionEvent;

pub(crate) const EVENT_LIMIT: usize = 240;

#[derive(Default)]
pub(crate) struct EventState {
    pub(crate) lines: VecDeque<EventLine>,
    pub(crate) dropped_count: usize,
}

#[derive(Clone)]
pub(crate) struct EventLine {
    pub(crate) kind: EventKind,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EventKind {
    System,
    Video,
    Comments,
    Danmaku,
    Output,
    Warning,
    Success,
    Failure,
}

impl EventState {
    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.dropped_count = 0;
    }

    pub(crate) fn push(&mut self, kind: EventKind, text: impl Into<String>) {
        self.push_line(EventLine {
            kind,
            text: text.into(),
        });
    }

    pub(crate) fn push_line(&mut self, line: EventLine) {
        if self.lines.len() >= EVENT_LIMIT {
            self.lines.pop_front();
            self.dropped_count += 1;
        }
        self.lines.push_back(line);
    }
}

impl EventLine {
    pub(crate) fn from_collection_event(event: &CollectionEvent) -> Self {
        match event {
            CollectionEvent::VideoStarted { bvid } => Self {
                kind: EventKind::Video,
                text: format!("开始处理视频：{bvid}"),
            },
            CollectionEvent::OutputInitialized { bvid, path } => Self {
                kind: EventKind::Output,
                text: format!("{bvid} 输出已就绪：{}", path.display()),
            },
            CollectionEvent::CommentScanPlanned {
                bvid,
                expected_total,
            } => Self {
                kind: EventKind::Comments,
                text: format!("{bvid} 评论分母：预计 {expected_total} 条"),
            },
            CollectionEvent::CommentBatchWritten {
                bvid,
                records_scanned,
                records_appended,
            } => Self {
                kind: EventKind::Comments,
                text: format!("{bvid} 评论批次：扫描 {records_scanned}，新增 {records_appended}"),
            },
            CollectionEvent::CommentScanFinished { bvid } => Self {
                kind: EventKind::Success,
                text: format!("{bvid} 评论扫描完成"),
            },
            CollectionEvent::DanmakuScanPlanned {
                bvid,
                total_segments,
            } => Self {
                kind: EventKind::Danmaku,
                text: format!("{bvid} 弹幕分母：预计 {total_segments} 个分段"),
            },
            CollectionEvent::DanmakuSegmentWritten {
                bvid,
                cid,
                page,
                segment_index,
                records_scanned,
                records_appended,
                segment_appended,
            } => Self {
                kind: EventKind::Danmaku,
                text: format!(
                    "{bvid} 弹幕分段：cid={cid}，P{page}，段 {segment_index}，扫描 {records_scanned}，新增 {records_appended}，元数据新增 {}",
                    yes_no(*segment_appended)
                ),
            },
            CollectionEvent::DanmakuScanFinished { bvid } => Self {
                kind: EventKind::Success,
                text: format!("{bvid} 弹幕扫描完成"),
            },
            CollectionEvent::VideoFinished { bvid } => Self {
                kind: EventKind::Success,
                text: format!("视频处理完成：{bvid}"),
            },
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "是" } else { "否" }
}
