use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::bili::client::BiliClient;
use crate::bili::comment::CommentRecord;
use crate::bili::video::VideoInfo;

#[derive(Debug, Parser)]
#[command(name = "bili-opinion")]
#[command(
    version,
    about = "Collect Bilibili video comments for opinion analysis."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Fetch basic metadata for a Bilibili video.
    Video(VideoArgs),

    /// Collect comments for one or more Bilibili videos.
    Comments(CommentsArgs),
}

#[derive(Debug, Args)]
pub struct VideoArgs {
    /// Bilibili video BVID, for example BV1xx411c7mD.
    #[arg(value_name = "BVID")]
    pub bvid: String,

    /// Read the full Bilibili Cookie header from a local file.
    #[arg(long, value_name = "FILE")]
    pub cookie: Option<PathBuf>,

    /// Use a SESSDATA value directly. Treat this as a secret.
    #[arg(long, value_name = "VALUE")]
    pub sessdata: Option<String>,
}

#[derive(Debug, Args)]
pub struct CommentsArgs {
    /// Bilibili video BVID values, for example BV1xx411c7mD.
    #[arg(value_name = "BVID")]
    pub bvids: Vec<String>,

    /// Read BVID values from a UTF-8 text file, one BVID per line.
    #[arg(long, value_name = "FILE")]
    pub input: Option<PathBuf>,

    /// Read the full Bilibili Cookie header from a local file.
    #[arg(long, value_name = "FILE")]
    pub cookie: Option<PathBuf>,

    /// Use a SESSDATA value directly. Treat this as a secret.
    #[arg(long, value_name = "VALUE")]
    pub sessdata: Option<String>,

    /// Output directory for collected comments.
    #[arg(long, value_name = "DIR", default_value = "output")]
    pub output: PathBuf,

    /// Output format. Use comma-separated values such as csv,jsonl.
    #[arg(long, value_enum, value_delimiter = ',', default_value = "csv")]
    pub format: Vec<OutputFormat>,

    /// Limit main comment pages. Useful for smoke tests.
    #[arg(long, value_name = "N")]
    pub max_pages: Option<usize>,

    /// Limit secondary comment pages per root comment. Useful for smoke tests.
    #[arg(long, value_name = "N")]
    pub max_reply_pages: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Csv,
    Jsonl,
}

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Video(args) => run_video(args).await,
        Commands::Comments(args) => run_comments(args).await,
    }
}

async fn run_video(args: VideoArgs) -> Result<()> {
    let cookie_header = load_cookie_header(&args.cookie, &args.sessdata)?;
    let client = BiliClient::new(cookie_header)?;
    let video = client.video_info(&args.bvid).await?;
    print_video_info(&video);
    Ok(())
}

async fn run_comments(args: CommentsArgs) -> Result<()> {
    let bvids = collect_comment_bvids(&args.bvids, &args.input)?;
    let cookie_header = load_cookie_header(&args.cookie, &args.sessdata)?;
    let client = BiliClient::new(cookie_header)?;
    let mut failures = Vec::new();

    for bvid in bvids {
        match collect_one_video_comments(&client, &args, &bvid).await {
            Ok(outcome) => {
                println!(
                    "wrote {} comments for {} (main_pages: {}, reply_pages: {}, next_cursor: {})",
                    outcome.comment_count,
                    outcome.bvid,
                    outcome.main_pages_scanned,
                    outcome.reply_pages_scanned,
                    outcome.next_cursor.as_deref().unwrap_or("<none>")
                );
                for output in outcome.outputs {
                    println!(
                        "output: {} (appended: {})",
                        output.path.display(),
                        output.appended_count
                    );
                }
            }
            Err(error) => {
                eprintln!("failed to collect {bvid}: {error}");
                failures.push(VideoFailure {
                    bvid,
                    error: error.to_string(),
                });
            }
        }
    }

    if !failures.is_empty() {
        let path = write_failure_report(&args.output, &failures)?;
        eprintln!("failure_report: {}", path.display());
        bail!("{} video(s) failed", failures.len());
    }

    Ok(())
}

struct VideoCommentOutcome {
    bvid: String,
    comment_count: usize,
    main_pages_scanned: usize,
    reply_pages_scanned: usize,
    next_cursor: Option<String>,
    outputs: Vec<OutputWriteResult>,
}

#[derive(Debug, Serialize)]
struct VideoFailure {
    #[serde(rename = "Bvid")]
    bvid: String,
    #[serde(rename = "Error")]
    error: String,
}

async fn collect_one_video_comments(
    client: &BiliClient,
    args: &CommentsArgs,
    bvid: &str,
) -> Result<VideoCommentOutcome> {
    tracing::info!(bvid, "collecting comments");
    let video = client.video_info(bvid).await?;
    let batch = client
        .main_comments(&video.bvid, video.aid, args.max_pages, args.max_reply_pages)
        .await?;
    let outputs = write_comment_outputs(&args.output, &video.bvid, &batch.comments, &args.format)?;

    Ok(VideoCommentOutcome {
        bvid: video.bvid,
        comment_count: batch.comments.len(),
        main_pages_scanned: batch.main_pages_scanned,
        reply_pages_scanned: batch.reply_pages_scanned,
        next_cursor: batch.next_cursor,
        outputs,
    })
}

fn collect_comment_bvids(positional: &[String], input: &Option<PathBuf>) -> Result<Vec<String>> {
    let mut bvids = positional
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if let Some(path) = input {
        let content = fs::read_to_string(path)?;
        bvids.extend(
            content
                .lines()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
        );
    }

    if bvids.is_empty() {
        bail!("provide at least one BVID or pass --input <FILE>");
    }

    Ok(bvids)
}

fn load_cookie_header(
    cookie: &Option<PathBuf>,
    sessdata: &Option<String>,
) -> Result<Option<String>> {
    match (cookie, sessdata) {
        (Some(_), Some(_)) => bail!("use either --cookie or --sessdata, not both"),
        (Some(path), None) => {
            let content = fs::read_to_string(path)
                .map(|value| value.trim().to_string())
                .map_err(anyhow::Error::from)?;

            if content.is_empty() {
                bail!("cookie file is empty: {}", path.display());
            }

            Ok(Some(content))
        }
        (None, Some(value)) => {
            let sessdata = value.trim();
            if sessdata.is_empty() {
                bail!("--sessdata cannot be empty");
            }

            Ok(Some(format!("SESSDATA={sessdata}")))
        }
        (None, None) => Ok(None),
    }
}

fn write_failure_report(output_root: &Path, failures: &[VideoFailure]) -> Result<PathBuf> {
    fs::create_dir_all(output_root)?;
    let path = output_root.join("failures.csv");
    let mut writer = csv::Writer::from_path(&path)?;
    for failure in failures {
        writer.serialize(failure)?;
    }
    writer.flush()?;
    Ok(path)
}

fn write_comment_outputs(
    output_root: &Path,
    bvid: &str,
    comments: &[CommentRecord],
    formats: &[OutputFormat],
) -> Result<Vec<OutputWriteResult>> {
    let video_dir = output_root.join(bvid);
    fs::create_dir_all(&video_dir)?;

    let mut outputs = Vec::new();
    for format in formats {
        match format {
            OutputFormat::Csv => {
                let path = video_dir.join("comments.csv");
                let existing = read_existing_csv_rpids(&path)?;
                let comments = comments_missing_from(comments, &existing);
                let appended_count = comments.len();
                write_comments_csv(&path, &comments)?;
                outputs.push(OutputWriteResult {
                    path,
                    appended_count,
                });
            }
            OutputFormat::Jsonl => {
                let path = video_dir.join("comments.jsonl");
                let existing = read_existing_jsonl_rpids(&path)?;
                let comments = comments_missing_from(comments, &existing);
                let appended_count = comments.len();
                write_comments_jsonl(&path, &comments)?;
                outputs.push(OutputWriteResult {
                    path,
                    appended_count,
                });
            }
        }
    }

    Ok(outputs)
}

struct OutputWriteResult {
    path: PathBuf,
    appended_count: usize,
}

#[derive(Debug, serde::Deserialize)]
struct ExistingCommentRow {
    #[serde(rename = "Rpid")]
    rpid: u64,
}

fn comments_missing_from(
    comments: &[CommentRecord],
    existing: &HashSet<u64>,
) -> Vec<CommentRecord> {
    comments
        .iter()
        .filter(|comment| !existing.contains(&comment.rpid))
        .cloned()
        .collect()
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
    if !path.exists() {
        return Ok(HashSet::new());
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut rpids = HashSet::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)?;
        if let Some(rpid) = value.get("Rpid").and_then(serde_json::Value::as_u64) {
            rpids.insert(rpid);
        }
    }
    Ok(rpids)
}

fn write_comments_csv(path: &Path, comments: &[CommentRecord]) -> Result<()> {
    let append_without_header = path.exists() && path.metadata()?.len() > 0;
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut writer = csv::WriterBuilder::new()
        .has_headers(!append_without_header)
        .from_writer(file);
    for comment in comments {
        writer.serialize(comment)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_comments_jsonl(path: &Path, comments: &[CommentRecord]) -> Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut writer = BufWriter::new(file);
    for comment in comments {
        serde_json::to_writer(&mut writer, comment)?;
        writeln!(writer)?;
    }
    writer.flush()?;
    Ok(())
}

fn print_video_info(video: &VideoInfo) {
    println!("bvid: {}", video.bvid);
    println!("aid: {}", video.aid);
    println!("cid: {}", video.cid);
    println!("title: {}", video.title);
    println!("comment_count: {}", video.comment_count);
    println!("danmaku_count: {}", video.danmaku_count);
    println!("pages: {}", video.pages.len());

    for page in &video.pages {
        println!(
            "page {}: cid={}, duration={}, part={}",
            page.page, page.cid, page.duration, page.part
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_comments_command_with_multiple_formats() {
        let cli = Cli::parse_from([
            "bili-opinion",
            "comments",
            "BV1xx411c7mD",
            "--format",
            "csv,jsonl",
            "--output",
            "output",
        ]);

        let Commands::Comments(args) = cli.command else {
            panic!("expected comments command");
        };
        assert_eq!(args.bvids, ["BV1xx411c7mD"]);
        assert_eq!(args.format, [OutputFormat::Csv, OutputFormat::Jsonl]);
        assert_eq!(args.output, PathBuf::from("output"));
    }

    #[test]
    fn parses_video_command() {
        let cli = Cli::parse_from(["bili-opinion", "video", "BV1xx411c7mD"]);

        let Commands::Video(args) = cli.command else {
            panic!("expected video command");
        };

        assert_eq!(args.bvid, "BV1xx411c7mD");
        assert!(args.cookie.is_none());
        assert!(args.sessdata.is_none());
    }

    #[test]
    fn formats_sessdata_as_cookie_header() {
        let cookie =
            load_cookie_header(&None, &Some("sample_sessdata".to_string())).expect("cookie header");

        assert_eq!(cookie, Some("SESSDATA=sample_sessdata".to_string()));
    }

    #[test]
    fn combines_positional_and_input_bvids() {
        let path =
            std::env::temp_dir().join(format!("bili-opinion-bvids-{}.txt", std::process::id()));
        fs::write(&path, "\nBV_input_1\nBV_input_2\n").expect("write input");

        let bvids = collect_comment_bvids(&["BV_positional".to_string()], &Some(path.clone()))
            .expect("bvids");

        fs::remove_file(path).expect("remove input");

        assert_eq!(bvids, ["BV_positional", "BV_input_1", "BV_input_2"]);
    }

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
            &[OutputFormat::Csv, OutputFormat::Jsonl],
        )
        .expect("first write");
        let second = write_comment_outputs(
            &output_root,
            "BV1xx411c7mD",
            &comments,
            &[OutputFormat::Csv, OutputFormat::Jsonl],
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

    fn sample_comment(rpid: u64) -> CommentRecord {
        CommentRecord {
            uname: "tester".to_string(),
            sex: "保密".to_string(),
            content: format!("comment {rpid}"),
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
