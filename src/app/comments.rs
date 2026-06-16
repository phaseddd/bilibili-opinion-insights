use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, bail};

use crate::app::events::CollectionEvent;
use crate::app::jsonl::{flush_jsonl_writer, read_jsonl_keys, write_jsonl_record};
use crate::bili::client::BiliClient;
use crate::bili::comment::{CommentRecord, CommentScanSummary};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommentOutputFormat {
    Csv,
    Jsonl,
}

#[derive(Debug, Clone)]
pub struct CommentCollectionOptions {
    pub output: PathBuf,
    pub formats: Vec<CommentOutputFormat>,
    pub max_pages: Option<usize>,
    pub max_reply_pages: Option<usize>,
    pub request_delay: Option<Duration>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommentCollectionOutcome {
    pub bvid: String,
    pub expected_total: u64,
    pub summary: CommentScanSummary,
    pub appended_count: usize,
    pub outputs: Vec<CommentOutputWriteResult>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommentOutputWriteResult {
    pub path: PathBuf,
    pub appended_count: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct CommentWriteCounts {
    records_scanned: usize,
    records_appended: usize,
}

pub async fn collect_video_comments(
    client: &BiliClient,
    bvid: &str,
    options: &CommentCollectionOptions,
) -> Result<CommentCollectionOutcome> {
    collect_video_comments_with_events(client, bvid, options, |_| Ok(())).await
}

pub async fn collect_video_comments_with_events<F>(
    client: &BiliClient,
    bvid: &str,
    options: &CommentCollectionOptions,
    mut on_event: F,
) -> Result<CommentCollectionOutcome>
where
    F: FnMut(CollectionEvent) -> Result<()>,
{
    on_event(CollectionEvent::VideoStarted {
        bvid: bvid.to_string(),
    })?;
    tracing::info!(bvid, "collecting comments");
    let video = client.video_info(bvid).await?;
    let expected_total = match client.comment_count(video.aid).await {
        Ok(count) => count,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "failed to fetch reply/count; falling back to video stat.reply"
            );
            video.comment_count
        }
    };
    on_event(CollectionEvent::CommentScanPlanned {
        bvid: video.bvid.clone(),
        expected_total,
    })?;
    let mut writer = CommentOutputWriter::create(&options.output, &video.bvid, &options.formats)?;
    for path in writer.paths() {
        tracing::info!(path = %path.display(), "initialized comment output");
        on_event(CollectionEvent::OutputInitialized {
            bvid: video.bvid.clone(),
            path: path.to_path_buf(),
        })?;
    }

    let event_bvid = video.bvid.clone();
    let summary = client
        .stream_comments(
            &video.bvid,
            video.aid,
            options.max_pages,
            options.max_reply_pages,
            options.request_delay,
            |comments| {
                let counts = writer.write_comments(comments)?;
                on_event(CollectionEvent::CommentBatchWritten {
                    bvid: event_bvid.clone(),
                    records_scanned: counts.records_scanned,
                    records_appended: counts.records_appended,
                })?;
                Ok(())
            },
        )
        .await?;
    let outputs = writer.finish()?;
    let appended_count = outputs
        .iter()
        .map(|output| output.appended_count)
        .max()
        .unwrap_or(0);
    on_event(CollectionEvent::CommentScanFinished {
        bvid: video.bvid.clone(),
    })?;
    on_event(CollectionEvent::VideoFinished {
        bvid: video.bvid.clone(),
    })?;

    Ok(CommentCollectionOutcome {
        bvid: video.bvid,
        expected_total,
        summary,
        appended_count,
        outputs,
    })
}

#[cfg(test)]
fn write_comment_outputs(
    output_root: &Path,
    bvid: &str,
    comments: &[CommentRecord],
    formats: &[CommentOutputFormat],
) -> Result<Vec<CommentOutputWriteResult>> {
    let mut writer = CommentOutputWriter::create(output_root, bvid, formats)?;
    writer.write_comments(comments)?;
    writer.finish()
}

const COMMENT_CSV_HEADER: &[&str] = &[
    "Uname",
    "Sex",
    "Content",
    "Pictures",
    "Picture_count",
    "Emotes",
    "Emote_urls",
    "At_users",
    "Jump_url_keys",
    "Jump_urls",
    "Video_time_seconds",
    "Video_time_links",
    "Rpid",
    "Oid",
    "Bvid",
    "Mid",
    "Parent",
    "Fansgrade",
    "Ctime",
    "Like",
    "Following",
    "Current_level",
    "Location",
];

struct CommentOutputWriter {
    outputs: Vec<CommentFormatWriter>,
}

impl CommentOutputWriter {
    fn create(
        output_root: &Path,
        bvid: &str,
        formats: &[CommentOutputFormat],
    ) -> Result<CommentOutputWriter> {
        let video_dir = output_root.join(bvid);
        fs::create_dir_all(&video_dir)?;

        let mut outputs = Vec::new();
        for format in formats {
            match format {
                CommentOutputFormat::Csv => {
                    let path = video_dir.join("comments.csv");
                    outputs.push(CommentFormatWriter::csv(path)?);
                }
                CommentOutputFormat::Jsonl => {
                    let path = video_dir.join("comments.jsonl");
                    outputs.push(CommentFormatWriter::jsonl(path)?);
                }
            }
        }

        Ok(Self { outputs })
    }

    fn paths(&self) -> Vec<&Path> {
        self.outputs.iter().map(CommentFormatWriter::path).collect()
    }

    fn write_comments(&mut self, comments: &[CommentRecord]) -> Result<CommentWriteCounts> {
        let mut records_appended = 0;
        for output in &mut self.outputs {
            records_appended = records_appended.max(output.write_comments(comments)?);
        }
        Ok(CommentWriteCounts {
            records_scanned: comments.len(),
            records_appended,
        })
    }

    fn finish(mut self) -> Result<Vec<CommentOutputWriteResult>> {
        let mut results = Vec::new();
        for output in &mut self.outputs {
            output.flush()?;
            results.push(output.result());
        }
        Ok(results)
    }
}

enum CommentFormatWriter {
    Csv {
        path: PathBuf,
        writer: Box<csv::Writer<File>>,
        seen: HashSet<u64>,
        appended_count: usize,
    },
    Jsonl {
        path: PathBuf,
        writer: BufWriter<File>,
        seen: HashSet<u64>,
        appended_count: usize,
    },
}

impl CommentFormatWriter {
    fn csv(path: PathBuf) -> Result<Self> {
        let append_without_header = path.exists() && path.metadata()?.len() > 0;
        if append_without_header {
            validate_existing_csv_header(&path)?;
        }
        let seen = read_existing_csv_rpids(&path)?;
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let writer = csv::WriterBuilder::new()
            .has_headers(!append_without_header)
            .from_writer(file);

        Ok(Self::Csv {
            path,
            writer: Box::new(writer),
            seen,
            appended_count: 0,
        })
    }

    fn jsonl(path: PathBuf) -> Result<Self> {
        let seen = read_existing_jsonl_rpids(&path)?;
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let writer = BufWriter::new(file);

        Ok(Self::Jsonl {
            path,
            writer,
            seen,
            appended_count: 0,
        })
    }

    fn path(&self) -> &Path {
        match self {
            Self::Csv { path, .. } | Self::Jsonl { path, .. } => path,
        }
    }

    fn write_comments(&mut self, comments: &[CommentRecord]) -> Result<usize> {
        match self {
            Self::Csv {
                writer,
                seen,
                appended_count,
                ..
            } => {
                let mut batch_appended = 0;
                for comment in comments {
                    if seen.insert(comment.rpid) {
                        writer.serialize(comment)?;
                        *appended_count += 1;
                        batch_appended += 1;
                    }
                }
                flush_csv_writer(writer.as_mut())?;
                Ok(batch_appended)
            }
            Self::Jsonl {
                writer,
                seen,
                appended_count,
                ..
            } => {
                let mut batch_appended = 0;
                for comment in comments {
                    if seen.insert(comment.rpid) {
                        write_jsonl_record(writer, comment)?;
                        *appended_count += 1;
                        batch_appended += 1;
                    }
                }
                flush_jsonl_writer(writer)?;
                Ok(batch_appended)
            }
        }
    }

    fn flush(&mut self) -> Result<()> {
        match self {
            Self::Csv { writer, .. } => flush_csv_writer(writer.as_mut())?,
            Self::Jsonl { writer, .. } => flush_jsonl_writer(writer)?,
        }
        Ok(())
    }

    fn result(&self) -> CommentOutputWriteResult {
        match self {
            Self::Csv {
                path,
                appended_count,
                ..
            }
            | Self::Jsonl {
                path,
                appended_count,
                ..
            } => CommentOutputWriteResult {
                path: path.clone(),
                appended_count: *appended_count,
            },
        }
    }
}

fn flush_csv_writer(writer: &mut csv::Writer<File>) -> Result<()> {
    writer.flush()?;
    writer.get_ref().sync_data()?;
    Ok(())
}

fn validate_existing_csv_header(path: &Path) -> Result<()> {
    let mut reader = csv::Reader::from_path(path)?;
    let headers = reader.headers()?;
    if headers.iter().collect::<Vec<_>>() != COMMENT_CSV_HEADER {
        bail!(
            "existing CSV header does not match current comment schema: {}",
            path.display()
        );
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct ExistingCommentRow {
    #[serde(rename = "Rpid")]
    rpid: u64,
}

fn read_existing_csv_rpids(path: &Path) -> Result<HashSet<u64>> {
    if !path.exists() {
        return Ok(HashSet::new());
    }

    let mut reader = csv::Reader::from_path(path)?;
    let mut rpids = HashSet::new();
    for record in reader.deserialize::<ExistingCommentRow>() {
        rpids.insert(record?.rpid);
    }
    Ok(rpids)
}

fn read_existing_jsonl_rpids(path: &Path) -> Result<HashSet<u64>> {
    read_jsonl_keys(path, |value| {
        value.get("Rpid").and_then(serde_json::Value::as_u64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_only_new_comments_to_existing_outputs() {
        let output_root =
            std::env::temp_dir().join(format!("bili-opinion-output-{}", std::process::id()));
        if output_root.exists() {
            fs::remove_dir_all(&output_root).expect("remove old output");
        }

        let comments = vec![sample_comment(1), sample_comment(2)];
        let first = write_comment_outputs(
            &output_root,
            "BV1xx411c7mD",
            &comments,
            &[CommentOutputFormat::Csv, CommentOutputFormat::Jsonl],
        )
        .expect("first write");
        let second = write_comment_outputs(
            &output_root,
            "BV1xx411c7mD",
            &comments,
            &[CommentOutputFormat::Csv, CommentOutputFormat::Jsonl],
        )
        .expect("second write");

        assert_eq!(
            first
                .iter()
                .map(|output| output.appended_count)
                .collect::<Vec<_>>(),
            [2, 2]
        );
        assert_eq!(
            second
                .iter()
                .map(|output| output.appended_count)
                .collect::<Vec<_>>(),
            [0, 0]
        );

        let csv =
            fs::read_to_string(output_root.join("BV1xx411c7mD").join("comments.csv")).expect("csv");
        let jsonl = fs::read_to_string(output_root.join("BV1xx411c7mD").join("comments.jsonl"))
            .expect("jsonl");

        assert_eq!(csv.lines().count(), 3);
        assert_eq!(jsonl.lines().count(), 2);

        fs::remove_dir_all(output_root).expect("remove output");
    }

    #[test]
    fn comment_output_writer_reports_per_batch_counts() {
        let output_root =
            std::env::temp_dir().join(format!("bili-opinion-count-output-{}", std::process::id()));
        if output_root.exists() {
            fs::remove_dir_all(&output_root).expect("remove old output");
        }

        let mut writer = CommentOutputWriter::create(
            &output_root,
            "BV1xx411c7mD",
            &[CommentOutputFormat::Csv, CommentOutputFormat::Jsonl],
        )
        .expect("create writer");

        let first = writer
            .write_comments(&[sample_comment(1), sample_comment(2)])
            .expect("write first batch");
        let second = writer
            .write_comments(&[sample_comment(2), sample_comment(3)])
            .expect("write second batch");
        writer.finish().expect("finish writer");

        assert_eq!(
            first,
            CommentWriteCounts {
                records_scanned: 2,
                records_appended: 2,
            }
        );
        assert_eq!(
            second,
            CommentWriteCounts {
                records_scanned: 2,
                records_appended: 1,
            }
        );

        fs::remove_dir_all(output_root).expect("remove output");
    }

    #[test]
    fn comment_output_writer_creates_files_and_flushes_each_batch() {
        let output_root =
            std::env::temp_dir().join(format!("bili-opinion-stream-output-{}", std::process::id()));
        if output_root.exists() {
            fs::remove_dir_all(&output_root).expect("remove old output");
        }

        let csv_path = output_root.join("BV1xx411c7mD").join("comments.csv");
        let jsonl_path = output_root.join("BV1xx411c7mD").join("comments.jsonl");
        let mut writer = CommentOutputWriter::create(
            &output_root,
            "BV1xx411c7mD",
            &[CommentOutputFormat::Csv, CommentOutputFormat::Jsonl],
        )
        .expect("create writer");

        assert!(csv_path.exists());
        assert!(jsonl_path.exists());

        writer
            .write_comments(&[sample_comment(1)])
            .expect("write first batch");
        assert_eq!(
            fs::read_to_string(&csv_path)
                .expect("csv after first batch")
                .lines()
                .count(),
            2
        );
        assert_eq!(
            fs::read_to_string(&jsonl_path)
                .expect("jsonl after first batch")
                .lines()
                .count(),
            1
        );

        writer
            .write_comments(&[sample_comment(1), sample_comment(2)])
            .expect("write second batch");
        let outputs = writer.finish().expect("finish writer");
        assert_eq!(
            outputs
                .iter()
                .map(|output| output.appended_count)
                .collect::<Vec<_>>(),
            [2, 2]
        );
        assert_eq!(
            fs::read_to_string(&csv_path)
                .expect("csv after second batch")
                .lines()
                .count(),
            3
        );
        assert_eq!(
            fs::read_to_string(&jsonl_path)
                .expect("jsonl after second batch")
                .lines()
                .count(),
            2
        );

        fs::remove_dir_all(output_root).expect("remove output");
    }

    fn sample_comment(rpid: u64) -> CommentRecord {
        CommentRecord {
            uname: "tester".to_string(),
            sex: "保密".to_string(),
            content: format!("comment {rpid}"),
            pictures: String::new(),
            picture_count: 0,
            emotes: String::new(),
            emote_urls: String::new(),
            at_users: String::new(),
            jump_url_keys: String::new(),
            jump_urls: String::new(),
            video_time_seconds: String::new(),
            video_time_links: String::new(),
            rpid,
            oid: 100,
            bvid: "BV1xx411c7mD".to_string(),
            mid: 200,
            parent: 0,
            fans_grade: false,
            ctime: 1710000000,
            like: 0,
            following: false,
            current_level: 1,
            location: String::new(),
            reply_count: 0,
        }
    }
}
