use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;

use crate::app::events::CollectionEvent;
use crate::bili::client::BiliClient;
use crate::bili::danmaku::{
    DanmakuRecord, DanmakuSegment, DanmakuSegmentContext, DanmakuSegmentMetadata, segment_count,
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
    pub records_scanned: usize,
    pub records_appended: usize,
    pub segments_scanned: u64,
    pub segments_appended: usize,
    pub record_path: PathBuf,
    pub segment_metadata_path: PathBuf,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct DanmakuWriteCounts {
    records_scanned: usize,
    records_appended: usize,
    segments_appended: usize,
}

pub async fn collect_video_danmaku(
    client: &BiliClient,
    bvid: &str,
    options: &DanmakuCollectionOptions,
) -> Result<DanmakuCollectionOutcome> {
    collect_video_danmaku_with_events(client, bvid, options, |_| Ok(())).await
}

pub async fn collect_video_danmaku_with_events<F>(
    client: &BiliClient,
    bvid: &str,
    options: &DanmakuCollectionOptions,
    mut on_event: F,
) -> Result<DanmakuCollectionOutcome>
where
    F: FnMut(CollectionEvent) -> Result<()>,
{
    on_event(CollectionEvent::VideoStarted {
        bvid: bvid.to_string(),
    })?;
    tracing::info!(bvid, "collecting danmaku");
    let video = client.video_info(bvid).await?;
    let mut writer = DanmakuOutputWriter::create(&options.output, &video.bvid)?;
    for path in writer.paths() {
        on_event(CollectionEvent::OutputInitialized {
            bvid: video.bvid.clone(),
            path,
        })?;
    }
    let mut records_scanned = 0;
    let mut records_appended = 0;
    let mut segments_scanned = 0;
    let mut segments_appended = 0;
    let total_segments = video
        .pages
        .iter()
        .map(|page| {
            let segments = segment_count(page.duration);
            options
                .max_segments
                .map_or(segments, |limit| segments.min(limit))
        })
        .sum();
    on_event(CollectionEvent::DanmakuScanPlanned {
        bvid: video.bvid.clone(),
        total_segments,
    })?;

    for page in &video.pages {
        let mut segments = segment_count(page.duration);
        if let Some(limit) = options.max_segments {
            segments = segments.min(limit);
        }

        for segment_index in 1..=segments {
            if segments_scanned > 0 {
                sleep_request_delay(options.request_delay).await;
            }

            let segment = client
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

            let counts = writer.write_segment(&segment)?;
            records_scanned += counts.records_scanned;
            records_appended += counts.records_appended;
            segments_appended += counts.segments_appended;
            on_event(CollectionEvent::DanmakuSegmentWritten {
                bvid: video.bvid.clone(),
                cid: page.cid,
                page: page.page,
                segment_index,
                records_scanned: counts.records_scanned,
                records_appended: counts.records_appended,
                segment_appended: counts.segments_appended > 0,
            })?;
        }
    }

    let output = writer.finish()?;
    on_event(CollectionEvent::DanmakuScanFinished {
        bvid: video.bvid.clone(),
    })?;
    on_event(CollectionEvent::VideoFinished {
        bvid: video.bvid.clone(),
    })?;

    Ok(DanmakuCollectionOutcome {
        bvid: video.bvid,
        records_scanned,
        records_appended,
        segments_scanned,
        segments_appended,
        record_path: output.record_path,
        segment_metadata_path: output.segment_metadata_path,
    })
}

async fn sleep_request_delay(request_delay: Option<Duration>) {
    if let Some(delay) = request_delay
        && !delay.is_zero()
    {
        tokio::time::sleep(delay).await;
    }
}

struct DanmakuOutputWriter {
    record_path: PathBuf,
    segment_metadata_path: PathBuf,
    record_writer: BufWriter<File>,
    segment_metadata_writer: BufWriter<File>,
    seen_record_keys: HashSet<String>,
    seen_segment_keys: HashSet<String>,
}

struct DanmakuOutputPaths {
    record_path: PathBuf,
    segment_metadata_path: PathBuf,
}

impl DanmakuOutputWriter {
    fn create(output_root: &Path, bvid: &str) -> Result<Self> {
        let video_dir = output_root.join(bvid);
        fs::create_dir_all(&video_dir)?;

        let record_path = video_dir.join("danmaku.jsonl");
        let segment_metadata_path = video_dir.join("danmaku_segments.jsonl");
        let seen_record_keys = read_existing_danmaku_record_keys(&record_path)?;
        let seen_segment_keys = read_existing_danmaku_segment_keys(&segment_metadata_path)?;
        let record_writer = append_jsonl_writer(&record_path)?;
        let segment_metadata_writer = append_jsonl_writer(&segment_metadata_path)?;

        Ok(Self {
            record_path,
            segment_metadata_path,
            record_writer,
            segment_metadata_writer,
            seen_record_keys,
            seen_segment_keys,
        })
    }

    fn write_segment(&mut self, segment: &DanmakuSegment) -> Result<DanmakuWriteCounts> {
        let mut records_appended = 0;

        for record in &segment.records {
            if self.seen_record_keys.insert(danmaku_record_key(record)) {
                write_jsonl_record(&mut self.record_writer, record)?;
                records_appended += 1;
            }
        }
        flush_jsonl_writer(&mut self.record_writer)?;

        let mut segments_appended = 0;
        if self
            .seen_segment_keys
            .insert(danmaku_segment_key(&segment.metadata))
        {
            write_jsonl_record(&mut self.segment_metadata_writer, &segment.metadata)?;
            flush_jsonl_writer(&mut self.segment_metadata_writer)?;
            segments_appended = 1;
        }

        Ok(DanmakuWriteCounts {
            records_scanned: segment.records.len(),
            records_appended,
            segments_appended,
        })
    }

    fn paths(&self) -> [PathBuf; 2] {
        [self.record_path.clone(), self.segment_metadata_path.clone()]
    }

    fn finish(mut self) -> Result<DanmakuOutputPaths> {
        flush_jsonl_writer(&mut self.record_writer)?;
        flush_jsonl_writer(&mut self.segment_metadata_writer)?;
        Ok(DanmakuOutputPaths {
            record_path: self.record_path,
            segment_metadata_path: self.segment_metadata_path,
        })
    }
}

fn append_jsonl_writer(path: &Path) -> Result<BufWriter<File>> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    Ok(BufWriter::new(file))
}

fn write_jsonl_record<T: Serialize>(writer: &mut BufWriter<File>, record: &T) -> Result<()> {
    serde_json::to_writer(&mut *writer, record)?;
    writeln!(writer)?;
    Ok(())
}

fn flush_jsonl_writer(writer: &mut BufWriter<File>) -> Result<()> {
    writer.flush()?;
    writer.get_mut().sync_data()?;
    Ok(())
}

fn danmaku_record_key(record: &DanmakuRecord) -> String {
    if record.id_str.trim().is_empty() {
        record.id.to_string()
    } else {
        record.id_str.clone()
    }
}

fn danmaku_segment_key(metadata: &DanmakuSegmentMetadata) -> String {
    format!(
        "{}:{}:{}",
        metadata.cid, metadata.page, metadata.segment_index
    )
}

fn read_existing_danmaku_record_keys(path: &Path) -> Result<HashSet<String>> {
    read_existing_jsonl_keys(path, |value| {
        value
            .get("id_str")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| value.get("id").map(serde_json::Value::to_string))
    })
}

fn read_existing_danmaku_segment_keys(path: &Path) -> Result<HashSet<String>> {
    read_existing_jsonl_keys(path, |value| {
        let cid = value.get("cid")?.as_u64()?;
        let page = value.get("page")?.as_u64()?;
        let segment_index = value.get("segment_index")?.as_u64()?;
        Some(format!("{cid}:{page}:{segment_index}"))
    })
}

fn read_existing_jsonl_keys<F>(path: &Path, mut key_from_value: F) -> Result<HashSet<String>>
where
    F: FnMut(&serde_json::Value) -> Option<String>,
{
    if !path.exists() {
        return Ok(HashSet::new());
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut keys = HashSet::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)?;
        if let Some(key) = key_from_value(&value) {
            keys.insert(key);
        }
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_only_new_danmaku_records_and_segments() {
        let output_root = std::env::temp_dir().join(format!(
            "bili-opinion-danmaku-output-{}",
            std::process::id()
        ));
        if output_root.exists() {
            fs::remove_dir_all(&output_root).expect("remove old output");
        }

        let segment = sample_segment(1, 1, &[1, 2]);
        let mut first_writer =
            DanmakuOutputWriter::create(&output_root, "BV1xx411c7mD").expect("first writer");
        let first = first_writer.write_segment(&segment).expect("first write");
        first_writer.finish().expect("finish first writer");

        let mut second_writer =
            DanmakuOutputWriter::create(&output_root, "BV1xx411c7mD").expect("second writer");
        let second = second_writer.write_segment(&segment).expect("second write");
        second_writer.finish().expect("finish second writer");

        assert_eq!(
            first,
            DanmakuWriteCounts {
                records_scanned: 2,
                records_appended: 2,
                segments_appended: 1,
            }
        );
        assert_eq!(
            second,
            DanmakuWriteCounts {
                records_scanned: 2,
                records_appended: 0,
                segments_appended: 0,
            }
        );

        let video_dir = output_root.join("BV1xx411c7mD");
        let records = fs::read_to_string(video_dir.join("danmaku.jsonl")).expect("danmaku records");
        let segments =
            fs::read_to_string(video_dir.join("danmaku_segments.jsonl")).expect("segment metadata");

        assert_eq!(records.lines().count(), 2);
        assert_eq!(segments.lines().count(), 1);

        fs::remove_dir_all(output_root).expect("remove output");
    }

    #[test]
    fn creates_danmaku_files_before_first_segment_has_records() {
        let output_root = std::env::temp_dir().join(format!(
            "bili-opinion-empty-danmaku-output-{}",
            std::process::id()
        ));
        if output_root.exists() {
            fs::remove_dir_all(&output_root).expect("remove old output");
        }

        let mut writer = DanmakuOutputWriter::create(&output_root, "BV1xx411c7mD").expect("writer");
        let counts = writer
            .write_segment(&sample_segment(1, 1, &[]))
            .expect("write empty segment");
        writer.finish().expect("finish writer");

        let video_dir = output_root.join("BV1xx411c7mD");
        assert!(video_dir.join("danmaku.jsonl").exists());
        assert!(video_dir.join("danmaku_segments.jsonl").exists());
        assert_eq!(
            counts,
            DanmakuWriteCounts {
                records_scanned: 0,
                records_appended: 0,
                segments_appended: 1,
            }
        );
        assert_eq!(
            fs::read_to_string(video_dir.join("danmaku.jsonl"))
                .expect("danmaku records")
                .lines()
                .count(),
            0
        );
        assert_eq!(
            fs::read_to_string(video_dir.join("danmaku_segments.jsonl"))
                .expect("segment metadata")
                .lines()
                .count(),
            1
        );

        fs::remove_dir_all(output_root).expect("remove output");
    }

    fn sample_segment(page: u64, segment_index: u64, ids: &[i64]) -> DanmakuSegment {
        let records = ids
            .iter()
            .map(|id| DanmakuRecord {
                bvid: "BV1xx411c7mD".to_string(),
                aid: 10,
                cid: 20,
                page,
                part: "P1".to_string(),
                segment_index,
                id: *id,
                progress_ms: 1000,
                mode: 1,
                font_size: 25,
                color: 16_777_215,
                mid_hash: "hash".to_string(),
                content: format!("danmaku {id}"),
                ctime: 1710000000,
                weight: 0,
                action: String::new(),
                pool: 0,
                id_str: id.to_string(),
                attr: 0,
                animation: String::new(),
                colorful: 0,
            })
            .collect::<Vec<_>>();

        DanmakuSegment {
            metadata: DanmakuSegmentMetadata {
                bvid: "BV1xx411c7mD".to_string(),
                aid: 10,
                cid: 20,
                page,
                part: "P1".to_string(),
                segment_index,
                record_count: records.len(),
                state: 0,
                ai_flags: Vec::new(),
                colorful_sources: Vec::new(),
            },
            records,
        }
    }
}
