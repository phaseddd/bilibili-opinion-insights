use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;

use crate::bili::client::BiliClient;
use crate::bili::danmaku::{
    DanmakuRecord, DanmakuSegmentContext, DanmakuSegmentMetadata, segment_count,
};

#[derive(Debug, Clone)]
pub struct DanmakuCollectionOptions {
    pub output: PathBuf,
    pub max_segments: Option<u64>,
    pub request_delay: Option<Duration>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DanmakuCollectionOutcome {
    pub bvid: String,
    pub record_count: usize,
    pub segments_scanned: u64,
    pub record_path: PathBuf,
    pub segment_metadata_path: PathBuf,
}

pub async fn collect_video_danmaku(
    client: &BiliClient,
    bvid: &str,
    options: &DanmakuCollectionOptions,
) -> Result<DanmakuCollectionOutcome> {
    tracing::info!(bvid, "collecting danmaku");
    let video = client.video_info(bvid).await?;
    let mut records = Vec::new();
    let mut segment_metadata = Vec::new();
    let mut segments_scanned = 0;

    for page in &video.pages {
        let mut segments = segment_count(page.duration);
        if let Some(limit) = options.max_segments {
            segments = segments.min(limit);
        }

        for segment_index in 1..=segments {
            if segments_scanned > 0 {
                sleep_request_delay(options.request_delay).await;
            }

            let mut segment = client
                .danmaku_segment(DanmakuSegmentContext {
                    bvid: &video.bvid,
                    aid: video.aid,
                    cid: page.cid,
                    page: page.page,
                    part: &page.part,
                    segment_index,
                })
                .await?;
            segments_scanned += 1;
            records.append(&mut segment.records);
            segment_metadata.push(segment.metadata);
        }
    }

    let record_path = write_danmaku_jsonl(&options.output, &video.bvid, &records)?;
    let segment_metadata_path =
        write_danmaku_segment_metadata_jsonl(&options.output, &video.bvid, &segment_metadata)?;

    Ok(DanmakuCollectionOutcome {
        bvid: video.bvid,
        record_count: records.len(),
        segments_scanned,
        record_path,
        segment_metadata_path,
    })
}

async fn sleep_request_delay(request_delay: Option<Duration>) {
    if let Some(delay) = request_delay
        && !delay.is_zero()
    {
        tokio::time::sleep(delay).await;
    }
}

fn write_danmaku_jsonl(
    output_root: &Path,
    bvid: &str,
    records: &[DanmakuRecord],
) -> Result<PathBuf> {
    write_jsonl_output(output_root, bvid, "danmaku.jsonl", records)
}

fn write_danmaku_segment_metadata_jsonl(
    output_root: &Path,
    bvid: &str,
    segments: &[DanmakuSegmentMetadata],
) -> Result<PathBuf> {
    write_jsonl_output(output_root, bvid, "danmaku_segments.jsonl", segments)
}

fn write_jsonl_output<T: Serialize>(
    output_root: &Path,
    bvid: &str,
    file_name: &str,
    records: &[T],
) -> Result<PathBuf> {
    let video_dir = output_root.join(bvid);
    fs::create_dir_all(&video_dir)?;
    let path = video_dir.join(file_name);
    let file = File::create(&path)?;
    let mut writer = BufWriter::new(file);
    for record in records {
        serde_json::to_writer(&mut writer, record)?;
        writeln!(writer)?;
    }
    writer.flush()?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct SampleRecord {
        id: u64,
        content: String,
    }

    #[test]
    fn writes_jsonl_output_under_video_directory() {
        let output_root =
            std::env::temp_dir().join(format!("bili-opinion-app-output-{}", std::process::id()));
        if output_root.exists() {
            fs::remove_dir_all(&output_root).expect("remove old output");
        }

        let path = write_jsonl_output(
            &output_root,
            "BV1xx411c7mD",
            "sample.jsonl",
            &[
                SampleRecord {
                    id: 1,
                    content: "first".to_string(),
                },
                SampleRecord {
                    id: 2,
                    content: "second".to_string(),
                },
            ],
        )
        .expect("write jsonl");

        assert_eq!(path, output_root.join("BV1xx411c7mD").join("sample.jsonl"));
        let content = fs::read_to_string(&path).expect("jsonl content");
        assert_eq!(content.lines().count(), 2);
        assert!(content.contains(r#""content":"first""#));
        assert!(content.contains(r#""content":"second""#));

        fs::remove_dir_all(output_root).expect("remove output");
    }
}
