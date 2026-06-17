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
                text: format!("{bvid} 评论预计总数：{expected_total} 条"),
            },
            CollectionEvent::CommentBatchWritten {
                bvid,
                records_scanned,
                records_appended,
            } => Self {
                kind: EventKind::Comments,
                text: format!(
                    "{bvid} 评论进度：已处理 {records_scanned} 条，新增 {records_appended} 条"
                ),
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
                text: format!("{bvid} 弹幕预计分包：{total_segments} 个"),
            },
            CollectionEvent::DanmakuSegmentWritten {
                bvid,
                cid,
                page,
                segment_index,
                records_scanned,
                records_appended,
                segment_appended,
            } => {
                let segment_state = if *segment_appended {
                    "分段记录已保存"
                } else {
                    "分段记录已存在"
                };
                Self {
                    kind: EventKind::Danmaku,
                    text: format!(
                        "{bvid} 弹幕分包：cid={cid}，P{page}，第 {segment_index} 包，已处理 {records_scanned} 条，新增 {records_appended} 条，{segment_state}"
                    ),
                }
            }
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

#[cfg(test)]
mod tests {
    use crate::app::events::CollectionEvent;

    use super::EventLine;

    #[test]
    fn collection_event_copy_uses_user_facing_progress_terms() {
        let events = [
            CollectionEvent::CommentScanPlanned {
                bvid: "BVTEST".to_string(),
                expected_total: 42,
            },
            CollectionEvent::CommentBatchWritten {
                bvid: "BVTEST".to_string(),
                records_scanned: 12,
                records_appended: 3,
            },
            CollectionEvent::DanmakuScanPlanned {
                bvid: "BVTEST".to_string(),
                total_segments: 7,
            },
            CollectionEvent::DanmakuSegmentWritten {
                bvid: "BVTEST".to_string(),
                cid: 1,
                page: 1,
                segment_index: 2,
                records_scanned: 120,
                records_appended: 8,
                segment_appended: true,
            },
        ];

        let text = events
            .iter()
            .map(EventLine::from_collection_event)
            .map(|line| line.text)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("评论预计总数"));
        assert!(text.contains("评论进度：已处理"));
        assert!(text.contains("弹幕预计分包"));
        assert!(text.contains("分段记录已保存"));
        assert!(!text.contains("分母"));
        assert!(!text.contains("元数据新增"));
    }
}
