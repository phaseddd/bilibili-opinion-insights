use std::path::PathBuf;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CollectionEvent {
    VideoStarted {
        bvid: String,
    },
    OutputInitialized {
        bvid: String,
        path: PathBuf,
    },
    CommentScanPlanned {
        bvid: String,
        expected_total: u64,
    },
    CommentBatchWritten {
        bvid: String,
        records_scanned: usize,
        records_appended: usize,
    },
    CommentScanFinished {
        bvid: String,
    },
    DanmakuScanPlanned {
        bvid: String,
        total_segments: u64,
    },
    DanmakuSegmentWritten {
        bvid: String,
        cid: u64,
        page: u64,
        segment_index: u64,
        records_scanned: usize,
        records_appended: usize,
        segment_appended: bool,
    },
    DanmakuScanFinished {
        bvid: String,
    },
    VideoFinished {
        bvid: String,
    },
}
