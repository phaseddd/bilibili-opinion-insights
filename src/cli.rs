use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::bili::auth::{QrLoginStatus, render_terminal_qr};
use crate::bili::client::BiliClient;
use crate::bili::comment::CommentRecord;
use crate::bili::danmaku::{DanmakuRecord, DanmakuSegmentContext, segment_count};
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
    /// Check whether the current Bilibili credentials are recognized.
    Auth(AuthArgs),

    /// Log in to Bilibili by scanning a QR code.
    Login(LoginArgs),

    /// Fetch basic metadata for a Bilibili video.
    Video(VideoArgs),

    /// Collect comments for one or more Bilibili videos.
    Comments(CommentsArgs),

    /// Collect danmaku for one or more Bilibili videos.
    Danmaku(DanmakuArgs),
}

#[derive(Debug, Args)]
pub struct AuthArgs {
    /// Read the full Bilibili Cookie header from a local file.
    #[arg(long, value_name = "FILE")]
    pub cookie: Option<PathBuf>,

    /// Use a SESSDATA value directly. Treat this as a secret.
    #[arg(long, value_name = "VALUE")]
    pub sessdata: Option<String>,
}

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Save the returned Cookie header to this local file.
    #[arg(
        long,
        value_name = "FILE",
        default_value = "config/bilibili-cookie.txt"
    )]
    pub output_cookie: PathBuf,

    /// Poll interval while waiting for QR scan confirmation.
    #[arg(long, value_name = "SECONDS", default_value_t = 3)]
    pub poll_interval_seconds: u64,

    /// Stop waiting after this many seconds.
    #[arg(long, value_name = "SECONDS", default_value_t = 180)]
    pub timeout_seconds: u64,
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

    /// Delay between comment page requests in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 500)]
    pub request_delay_ms: u64,
}

#[derive(Debug, Args)]
pub struct DanmakuArgs {
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

    /// Output directory for collected danmaku.
    #[arg(long, value_name = "DIR", default_value = "output")]
    pub output: PathBuf,

    /// Limit segments per video page. Useful for smoke tests.
    #[arg(long, value_name = "N")]
    pub max_segments: Option<u64>,

    /// Delay between danmaku segment requests in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 500)]
    pub request_delay_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Csv,
    Jsonl,
}

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Auth(args) => run_auth(args).await,
        Commands::Login(args) => run_login(args).await,
        Commands::Video(args) => run_video(args).await,
        Commands::Comments(args) => run_comments(args).await,
        Commands::Danmaku(args) => run_danmaku(args).await,
    }
}

async fn run_login(args: LoginArgs) -> Result<()> {
    let client = BiliClient::new(None)?;
    let session = client.generate_qr_login().await?;

    println!("login_url: {}", session.url);
    println!("{}", render_terminal_qr(&session.url)?);
    println!("status: waiting_for_scan");

    let started = Instant::now();
    let poll_interval = Duration::from_secs(args.poll_interval_seconds.max(1));
    let timeout = Duration::from_secs(args.timeout_seconds.max(1));
    let mut last_status = String::from("waiting_for_scan");

    loop {
        if started.elapsed() >= timeout {
            bail!("QR login timed out");
        }

        tokio::time::sleep(poll_interval).await;
        match client.poll_qr_login(&session.qrcode_key).await? {
            QrLoginStatus::WaitingForScan => {
                print_login_status_once(&mut last_status, "waiting_for_scan");
            }
            QrLoginStatus::WaitingForConfirm => {
                print_login_status_once(&mut last_status, "waiting_for_confirm");
            }
            QrLoginStatus::Expired => bail!("QR login expired"),
            QrLoginStatus::Success { cookie_header } => {
                save_cookie_header(&args.output_cookie, &cookie_header)?;
                println!("status: logged_in");
                println!("cookie_saved: {}", args.output_cookie.display());
                break;
            }
        }
    }

    Ok(())
}

async fn run_auth(args: AuthArgs) -> Result<()> {
    let cookie_header = load_cookie_header(&args.cookie, &args.sessdata)?;
    let client = BiliClient::new(cookie_header)?;
    let login = client.login_state().await?;

    println!("logged_in: {}", login.is_login);
    println!(
        "mid: {}",
        login
            .mid
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );
    println!("uname: {}", login.uname.as_deref().unwrap_or("<none>"));
    println!("vip_status: {}", login.vip_status);

    Ok(())
}

fn print_login_status_once(last_status: &mut String, status: &str) {
    if last_status != status {
        println!("status: {status}");
        last_status.clear();
        last_status.push_str(status);
    }
}

fn save_cookie_header(path: &Path, cookie_header: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, cookie_header)?;
    Ok(())
}

async fn run_video(args: VideoArgs) -> Result<()> {
    let cookie_header = load_cookie_header(&args.cookie, &args.sessdata)?;
    let client = BiliClient::new(cookie_header)?;
    let video = client.video_info(&args.bvid).await?;
    print_video_info(&video);
    Ok(())
}

async fn run_comments(args: CommentsArgs) -> Result<()> {
    let bvids = collect_bvids(&args.bvids, &args.input)?;
    let cookie_header = load_cookie_header(&args.cookie, &args.sessdata)?;
    let client = BiliClient::new(cookie_header)?;
    let mut failures = Vec::new();

    for bvid in bvids {
        match collect_one_video_comments(&client, &args, &bvid).await {
            Ok(outcome) => {
                println!(
                    "wrote {} comments for {} (expected_total: {}, main_pages: {}, reply_pages: {}, next_cursor: {})",
                    outcome.comment_count,
                    outcome.bvid,
                    outcome.expected_total,
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

async fn run_danmaku(args: DanmakuArgs) -> Result<()> {
    let bvids = collect_bvids(&args.bvids, &args.input)?;
    let cookie_header = load_cookie_header(&args.cookie, &args.sessdata)?;
    let client = BiliClient::new(cookie_header)?;
    let mut failures = Vec::new();

    for bvid in bvids {
        match collect_one_video_danmaku(&client, &args, &bvid).await {
            Ok(outcome) => {
                println!(
                    "wrote {} danmaku records for {} (segments_scanned: {})",
                    outcome.record_count, outcome.bvid, outcome.segments_scanned
                );
                println!("output: {}", outcome.path.display());
            }
            Err(error) => {
                eprintln!("failed to collect danmaku {bvid}: {error}");
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
    expected_total: u64,
    comment_count: usize,
    main_pages_scanned: usize,
    reply_pages_scanned: usize,
    next_cursor: Option<String>,
    outputs: Vec<OutputWriteResult>,
}

struct VideoDanmakuOutcome {
    bvid: String,
    record_count: usize,
    segments_scanned: u64,
    path: PathBuf,
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
    let batch = client
        .main_comments(
            &video.bvid,
            video.aid,
            args.max_pages,
            args.max_reply_pages,
            request_delay(args.request_delay_ms),
        )
        .await?;
    let outputs = write_comment_outputs(&args.output, &video.bvid, &batch.comments, &args.format)?;

    Ok(VideoCommentOutcome {
        bvid: video.bvid,
        expected_total,
        comment_count: batch.comments.len(),
        main_pages_scanned: batch.main_pages_scanned,
        reply_pages_scanned: batch.reply_pages_scanned,
        next_cursor: batch.next_cursor,
        outputs,
    })
}

async fn collect_one_video_danmaku(
    client: &BiliClient,
    args: &DanmakuArgs,
    bvid: &str,
) -> Result<VideoDanmakuOutcome> {
    tracing::info!(bvid, "collecting danmaku");
    let video = client.video_info(bvid).await?;
    let mut records = Vec::new();
    let mut segments_scanned = 0;
    let delay = request_delay(args.request_delay_ms);

    for page in &video.pages {
        let mut segments = segment_count(page.duration);
        if let Some(limit) = args.max_segments {
            segments = segments.min(limit);
        }

        for segment_index in 1..=segments {
            if segments_scanned > 0 {
                sleep_cli_delay(delay).await;
            }

            let mut segment_records = client
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
            records.append(&mut segment_records);
        }
    }

    let path = write_danmaku_jsonl(&args.output, &video.bvid, &records)?;

    Ok(VideoDanmakuOutcome {
        bvid: video.bvid,
        record_count: records.len(),
        segments_scanned,
        path,
    })
}

fn request_delay(milliseconds: u64) -> Option<Duration> {
    if milliseconds == 0 {
        None
    } else {
        Some(Duration::from_millis(milliseconds))
    }
}

async fn sleep_cli_delay(request_delay: Option<Duration>) {
    if let Some(delay) = request_delay
        && !delay.is_zero()
    {
        tokio::time::sleep(delay).await;
    }
}

fn collect_bvids(positional: &[String], input: &Option<PathBuf>) -> Result<Vec<String>> {
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

fn write_danmaku_jsonl(
    output_root: &Path,
    bvid: &str,
    records: &[DanmakuRecord],
) -> Result<PathBuf> {
    let video_dir = output_root.join(bvid);
    fs::create_dir_all(&video_dir)?;
    let path = video_dir.join("danmaku.jsonl");
    let file = File::create(&path)?;
    let mut writer = BufWriter::new(file);
    for record in records {
        serde_json::to_writer(&mut writer, record)?;
        writeln!(writer)?;
    }
    writer.flush()?;
    Ok(path)
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
    fn parses_auth_command() {
        let cli = Cli::parse_from(["bili-opinion", "auth", "--sessdata", "sample"]);

        let Commands::Auth(args) = cli.command else {
            panic!("expected auth command");
        };

        assert!(args.cookie.is_none());
        assert_eq!(args.sessdata.as_deref(), Some("sample"));
    }

    #[test]
    fn parses_login_command() {
        let cli = Cli::parse_from([
            "bili-opinion",
            "login",
            "--output-cookie",
            "config/test-cookie.txt",
            "--timeout-seconds",
            "5",
        ]);

        let Commands::Login(args) = cli.command else {
            panic!("expected login command");
        };

        assert_eq!(args.output_cookie, PathBuf::from("config/test-cookie.txt"));
        assert_eq!(args.timeout_seconds, 5);
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

        let bvids =
            collect_bvids(&["BV_positional".to_string()], &Some(path.clone())).expect("bvids");

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

    #[test]
    fn parses_danmaku_command() {
        let cli = Cli::parse_from([
            "bili-opinion",
            "danmaku",
            "BV1xx411c7mD",
            "--max-segments",
            "1",
        ]);

        let Commands::Danmaku(args) = cli.command else {
            panic!("expected danmaku command");
        };

        assert_eq!(args.bvids, ["BV1xx411c7mD"]);
        assert_eq!(args.max_segments, Some(1));
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
