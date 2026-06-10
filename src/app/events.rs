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
    CommentBatchWritten {
        bvid: String,
        records_scanned: usize,
        records_appended: usize,
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
    VideoFinished {
        bvid: String,
    },
}
