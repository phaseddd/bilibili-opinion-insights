use std::path::Path;

use anyhow::Result;

use crate::app::jsonl::read_jsonl_records;
use crate::bili::comment::CommentRecord;
use crate::bili::danmaku::{DanmakuRecord, DanmakuSegmentMetadata};

pub fn read_comment_records(path: &Path) -> Result<Vec<CommentRecord>> {
    read_jsonl_records(path)
}

pub fn read_danmaku_records(path: &Path) -> Result<Vec<DanmakuRecord>> {
    read_jsonl_records(path)
}

pub fn read_danmaku_segment_metadata(path: &Path) -> Result<Vec<DanmakuSegmentMetadata>> {
    read_jsonl_records(path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn reads_comment_jsonl_records_for_viewing() {
        let path = std::env::temp_dir().join(format!(
            "bili-opinion-comment-view-{}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"{"Uname":"tester","Sex":"保密","Content":"hello","Pictures":"","Picture_count":0,"Emotes":"","Emote_urls":"","At_users":"","Jump_url_keys":"","Jump_urls":"","Video_time_seconds":"","Video_time_links":"","Rpid":1,"Oid":2,"Bvid":"BV1xx411c7mD","Mid":3,"Parent":0,"Fansgrade":false,"Ctime":1710000000,"Like":4,"Following":false,"Current_level":6,"Location":"IP属地：上海"}"#,
        )
        .expect("write comment JSONL");

        let records = read_comment_records(&path).expect("read comments");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].rpid, 1);
        assert_eq!(records[0].content, "hello");
        fs::remove_file(path).expect("remove comment JSONL");
    }

    #[test]
    fn reads_danmaku_jsonl_records_for_viewing() {
        let path = std::env::temp_dir().join(format!(
            "bili-opinion-danmaku-view-{}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"{"bvid":"BV1xx411c7mD","aid":10,"cid":20,"page":1,"part":"P1","segment_index":1,"id":100,"progress_ms":1234,"mode":1,"font_size":25,"color":16777215,"mid_hash":"hash","content":"弹幕","ctime":1710000000,"weight":0,"action":"","pool":0,"id_str":"100","attr":0,"animation":"","colorful":0}"#,
        )
        .expect("write danmaku JSONL");

        let records = read_danmaku_records(&path).expect("read danmaku");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id_str, "100");
        assert_eq!(records[0].content, "弹幕");
        fs::remove_file(path).expect("remove danmaku JSONL");
    }

    #[test]
    fn reads_danmaku_segment_metadata_for_viewing() {
        let path = std::env::temp_dir().join(format!(
            "bili-opinion-danmaku-segments-view-{}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"{"bvid":"BV1xx411c7mD","aid":10,"cid":20,"page":1,"part":"P1","segment_index":1,"record_count":2,"state":0,"ai_flags":[{"dmid":100,"flag":8}],"colorful_sources":[{"colorful_type":60001,"src":"https://example.com/a.png"}]}"#,
        )
        .expect("write danmaku segment JSONL");

        let records = read_danmaku_segment_metadata(&path).expect("read segment metadata");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_count, 2);
        assert_eq!(records[0].ai_flags[0].flag, 8);
        fs::remove_file(path).expect("remove danmaku segment JSONL");
    }
}
