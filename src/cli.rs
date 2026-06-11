use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::app::collection::{
    CollectionFailure, CollectionJobOutcome, CollectionRequest, CredentialOptions,
    DEFAULT_COOKIE_PATH, DEFAULT_OUTPUT_ROOT, DEFAULT_REQUEST_DELAY_MS, default_cookie_header,
    load_cookie_header, run_collection_with_events,
};
use crate::app::comments::CommentOutputFormat;
use crate::bili::auth::{QrLoginStatus, render_terminal_qr};
use crate::bili::client::BiliClient;
use crate::bili::video::{VideoInfo, normalize_bvid_input};

const CLI_LONG_ABOUT: &str =
    "Collect Bilibili video comments and danmaku for local opinion-analysis workflows.";
const CLI_AFTER_HELP: &str = r#"Common workflows:
  bili-opinion login
  bili-opinion auth
  bili-opinion run BV1yEEq68EQ3 --output output
  bili-opinion comments BV1yEEq68EQ3 --format csv,jsonl
  bili-opinion danmaku BV1yEEq68EQ3
  bili-opinion clean

Command-specific help:
  bili-opinion help video
  bili-opinion video --help
  bili-opinion help run
  bili-opinion run --help

Credential behavior:
  login writes config/bilibili-cookie.txt by default.
  auth, video, run, comments, and danmaku read that file automatically when it exists.
  Use --cookie FILE for another cookie file, --sessdata VALUE for a raw SESSDATA value,
  or --anonymous to force an anonymous request.

Output layout:
  output/<BVID>/comments.csv
  output/<BVID>/comments.jsonl
  output/<BVID>/danmaku.jsonl
  output/<BVID>/danmaku_segments.jsonl
  output/failures.csv
"#;

const LOGIN_AFTER_HELP: &str = r#"Examples:
  bili-opinion login
  bili-opinion login --output-cookie config/bilibili-cookie.txt --timeout-seconds 180

After a successful login, run `bili-opinion auth` to verify the saved cookie.
"#;

const AUTH_AFTER_HELP: &str = r#"Examples:
  bili-opinion auth
  bili-opinion auth --cookie config/bilibili-cookie.txt
  bili-opinion auth --sessdata YOUR_SESSDATA
  bili-opinion auth --anonymous
"#;

const VIDEO_AFTER_HELP: &str = r#"Examples:
  bili-opinion video BV1yEEq68EQ3
  bili-opinion video https://www.bilibili.com/video/BV1yEEq68EQ3
  bili-opinion video BV1yEEq68EQ3 --cookie config/bilibili-cookie.txt
  bili-opinion video BV1yEEq68EQ3 --anonymous
"#;

const RUN_AFTER_HELP: &str = r#"Examples:
  bili-opinion run BV1yEEq68EQ3 --output output
  bili-opinion run https://www.bilibili.com/video/BV1yEEq68EQ3 --output output
  bili-opinion run BV1yEEq68EQ3 --format csv,jsonl --request-delay-ms 1500
  bili-opinion run --input bvids.txt --danmaku-only

This command collects comments and danmaku in one pass by default.
"#;

const COMMENTS_AFTER_HELP: &str = r#"Examples:
  bili-opinion comments BV1yEEq68EQ3 --format csv,jsonl
  bili-opinion comments https://www.bilibili.com/video/BV1yEEq68EQ3 --format csv,jsonl
  bili-opinion comments --input bvids.txt --output output --request-delay-ms 1500
  bili-opinion comments BV1yEEq68EQ3 --max-pages 1 --max-reply-pages 1

For full collection, prefer a browser cookie and a conservative request delay.
"#;

const DANMAKU_AFTER_HELP: &str = r#"Examples:
  bili-opinion danmaku BV1yEEq68EQ3
  bili-opinion danmaku https://www.bilibili.com/video/BV1yEEq68EQ3
  bili-opinion danmaku --input bvids.txt --output output
  bili-opinion danmaku BV1yEEq68EQ3 --max-segments 1
"#;

const CLEAN_AFTER_HELP: &str = r#"Examples:
  bili-opinion clean
  bili-opinion clean --root output --archive-root doc/output-archives
  bili-opinion clean --no-archive

By default this archives the output directory under doc/output-archives first,
then recreates an empty output directory.
"#;

#[derive(Debug, Parser)]
#[command(name = "bili-opinion")]
#[command(version, about = CLI_LONG_ABOUT, long_about = CLI_LONG_ABOUT)]
#[command(after_help = CLI_AFTER_HELP)]
#[command(arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Log in to Bilibili by scanning a QR code.
    #[command(after_help = LOGIN_AFTER_HELP)]
    Login(LoginArgs),

    /// Check whether the current Bilibili credentials are recognized.
    #[command(after_help = AUTH_AFTER_HELP)]
    Auth(AuthArgs),

    /// Fetch basic metadata for a Bilibili video.
    #[command(after_help = VIDEO_AFTER_HELP)]
    Video(VideoArgs),

    /// Collect comments and danmaku for one or more Bilibili videos.
    #[command(after_help = RUN_AFTER_HELP)]
    Run(RunArgs),

    /// Collect comments for one or more Bilibili videos.
    #[command(after_help = COMMENTS_AFTER_HELP)]
    Comments(CommentsArgs),

    /// Collect danmaku for one or more Bilibili videos.
    #[command(after_help = DANMAKU_AFTER_HELP)]
    Danmaku(DanmakuArgs),

    /// Archive and clear local collection output.
    #[command(after_help = CLEAN_AFTER_HELP)]
    Clean(CleanArgs),
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
pub struct AuthArgs {
    #[command(flatten)]
    pub credentials: CredentialArgs,
}

#[derive(Debug, Args)]
pub struct VideoArgs {
    /// Bilibili video BVID or video URL, for example BV1yEEq68EQ3.
    #[arg(value_name = "BVID_OR_URL")]
    pub bvid: String,

    #[command(flatten)]
    pub credentials: CredentialArgs,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Bilibili video BVID values or video URLs, for example BV1yEEq68EQ3.
    #[arg(value_name = "BVID_OR_URL")]
    pub bvids: Vec<String>,

    /// Read BVID values or video URLs from a UTF-8 text file, one item per line.
    #[arg(long, value_name = "FILE")]
    pub input: Option<PathBuf>,

    #[command(flatten)]
    pub credentials: CredentialArgs,

    /// Output directory for collected data.
    #[arg(long, value_name = "DIR", default_value = "output")]
    pub output: PathBuf,

    /// Comment output format. Use comma-separated values such as csv,jsonl.
    #[arg(long, value_enum, value_delimiter = ',', default_value = "csv,jsonl")]
    pub format: Vec<OutputFormat>,

    /// Collect only comments.
    #[arg(long, conflicts_with = "danmaku_only")]
    pub comments_only: bool,

    /// Collect only danmaku.
    #[arg(long, conflicts_with = "comments_only")]
    pub danmaku_only: bool,

    /// Limit main comment pages. Useful for smoke tests.
    #[arg(long, value_name = "N")]
    pub max_pages: Option<usize>,

    /// Limit secondary comment pages per root comment. Useful for smoke tests.
    #[arg(long, value_name = "N")]
    pub max_reply_pages: Option<usize>,

    /// Limit danmaku segments per video page. Useful for smoke tests.
    #[arg(long, value_name = "N")]
    pub max_segments: Option<u64>,

    /// Delay between Bilibili page/segment requests in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = DEFAULT_REQUEST_DELAY_MS)]
    pub request_delay_ms: u64,
}

#[derive(Debug, Args)]
pub struct CommentsArgs {
    /// Bilibili video BVID values or video URLs, for example BV1yEEq68EQ3.
    #[arg(value_name = "BVID_OR_URL")]
    pub bvids: Vec<String>,

    /// Read BVID values or video URLs from a UTF-8 text file, one item per line.
    #[arg(long, value_name = "FILE")]
    pub input: Option<PathBuf>,

    #[command(flatten)]
    pub credentials: CredentialArgs,

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
    #[arg(long, value_name = "MS", default_value_t = DEFAULT_REQUEST_DELAY_MS)]
    pub request_delay_ms: u64,
}

#[derive(Debug, Args)]
pub struct DanmakuArgs {
    /// Bilibili video BVID values or video URLs, for example BV1yEEq68EQ3.
    #[arg(value_name = "BVID_OR_URL")]
    pub bvids: Vec<String>,

    /// Read BVID values or video URLs from a UTF-8 text file, one item per line.
    #[arg(long, value_name = "FILE")]
    pub input: Option<PathBuf>,

    #[command(flatten)]
    pub credentials: CredentialArgs,

    /// Output directory for collected danmaku.
    #[arg(long, value_name = "DIR", default_value = "output")]
    pub output: PathBuf,

    /// Limit segments per video page. Useful for smoke tests.
    #[arg(long, value_name = "N")]
    pub max_segments: Option<u64>,

    /// Delay between danmaku segment requests in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = DEFAULT_REQUEST_DELAY_MS)]
    pub request_delay_ms: u64,
}

#[derive(Debug, Args)]
pub struct CleanArgs {
    /// Output directory to archive and clear.
    #[arg(long, value_name = "DIR", default_value = "output")]
    pub root: PathBuf,

    /// Directory that receives archived output snapshots.
    #[arg(long, value_name = "DIR", default_value = "doc/output-archives")]
    pub archive_root: PathBuf,

    /// Delete output without first archiving it.
    #[arg(long)]
    pub no_archive: bool,
}

#[derive(Debug, Args)]
pub struct CredentialArgs {
    /// Read the full Bilibili Cookie header from a local file.
    #[arg(long, value_name = "FILE")]
    pub cookie: Option<PathBuf>,

    /// Use a SESSDATA value directly. Treat this as a secret.
    #[arg(long, value_name = "VALUE")]
    pub sessdata: Option<String>,

    /// Force anonymous requests even when config/bilibili-cookie.txt exists.
    #[arg(long)]
    pub anonymous: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Csv,
    Jsonl,
}

impl From<OutputFormat> for CommentOutputFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Csv => Self::Csv,
            OutputFormat::Jsonl => Self::Jsonl,
        }
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Login(args) => run_login(args).await,
        Commands::Auth(args) => run_auth(args).await,
        Commands::Video(args) => run_video(args).await,
        Commands::Run(args) => run_all(args).await,
        Commands::Comments(args) => run_comments(args).await,
        Commands::Danmaku(args) => run_danmaku(args).await,
        Commands::Clean(args) => run_clean(args),
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
                println!("next_auth_check: bili-opinion auth");
                println!("next_collection: bili-opinion run BV1yEEq68EQ3 --output output");
                break;
            }
        }
    }

    Ok(())
}

async fn run_auth(args: AuthArgs) -> Result<()> {
    let credentials = credential_options(&args.credentials);
    let credential_source = credential_source(&credentials);
    let cookie_header = load_cookie_header(&credentials)?;
    let client = BiliClient::new(cookie_header)?;
    let login = client.login_state().await?;

    println!("credential_source: {credential_source}");
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

async fn run_video(args: VideoArgs) -> Result<()> {
    let credentials = credential_options(&args.credentials);
    let cookie_header = load_cookie_header(&credentials)?;
    let client = BiliClient::new(cookie_header)?;
    let bvid = normalize_bvid_value(&args.bvid)?;
    let video = client.video_info(&bvid).await?;
    print_video_info(&video);
    Ok(())
}

async fn run_all(args: RunArgs) -> Result<()> {
    let request = CollectionRequest {
        bvids: collect_bvids(&args.bvids, &args.input)?,
        credentials: credential_options(&args.credentials),
        output: args.output,
        collect_comments: !args.danmaku_only,
        collect_danmaku: !args.comments_only,
        comment_formats: comment_formats(&args.format),
        max_comment_pages: args.max_pages,
        max_reply_pages: args.max_reply_pages,
        max_danmaku_segments: args.max_segments,
        request_delay: request_delay(args.request_delay_ms),
    };
    run_collection_request(request).await
}

async fn run_comments(args: CommentsArgs) -> Result<()> {
    let request = CollectionRequest {
        bvids: collect_bvids(&args.bvids, &args.input)?,
        credentials: credential_options(&args.credentials),
        output: args.output,
        collect_comments: true,
        collect_danmaku: false,
        comment_formats: comment_formats(&args.format),
        max_comment_pages: args.max_pages,
        max_reply_pages: args.max_reply_pages,
        max_danmaku_segments: None,
        request_delay: request_delay(args.request_delay_ms),
    };
    run_collection_request(request).await
}

async fn run_danmaku(args: DanmakuArgs) -> Result<()> {
    let request = CollectionRequest {
        bvids: collect_bvids(&args.bvids, &args.input)?,
        credentials: credential_options(&args.credentials),
        output: args.output,
        collect_comments: false,
        collect_danmaku: true,
        comment_formats: vec![CommentOutputFormat::Csv],
        max_comment_pages: None,
        max_reply_pages: None,
        max_danmaku_segments: args.max_segments,
        request_delay: request_delay(args.request_delay_ms),
    };
    run_collection_request(request).await
}

async fn run_collection_request(request: CollectionRequest) -> Result<()> {
    println!(
        "credential_source: {}",
        credential_source(&request.credentials)
    );
    println!("output_root: {}", request.output.display());

    let outcome = run_collection_with_events(&request, |_| Ok(())).await?;
    for job in &outcome.jobs {
        print_collection_job(job);
    }

    if !outcome.failures.is_empty() {
        for failure in &outcome.failures {
            eprintln!(
                "failed to collect {} for {}: {}",
                failure.kind.as_str(),
                failure.bvid,
                failure.error
            );
        }
        let path = write_failure_report(&request.output, &outcome.failures)?;
        eprintln!("failure_report: {}", path.display());
    }

    outcome.ensure_success()
}

fn run_clean(args: CleanArgs) -> Result<()> {
    if !args.root.exists() {
        println!("clean_target: {}", args.root.display());
        println!("status: nothing_to_clean");
        return Ok(());
    }

    ensure_clean_target(&args.root)?;
    let summary = summarize_tree(&args.root)?;
    println!("clean_target: {}", args.root.display());
    println!("files: {}", summary.files);
    println!("directories: {}", summary.directories);
    println!("bytes: {}", summary.bytes);

    if args.no_archive {
        fs::remove_dir_all(&args.root)
            .with_context(|| format!("failed to remove {}", args.root.display()))?;
        fs::create_dir_all(&args.root)
            .with_context(|| format!("failed to recreate {}", args.root.display()))?;
        println!("status: deleted_without_archive");
        return Ok(());
    }

    fs::create_dir_all(&args.archive_root)
        .with_context(|| format!("failed to create {}", args.archive_root.display()))?;
    let archive_path = unique_archive_path(&args.archive_root, &args.root)?;
    fs::rename(&args.root, &archive_path).with_context(|| {
        format!(
            "failed to archive {} to {}",
            args.root.display(),
            archive_path.display()
        )
    })?;
    write_clean_manifest(&archive_path, &args.root, &summary)?;
    fs::create_dir_all(&args.root)
        .with_context(|| format!("failed to recreate {}", args.root.display()))?;

    println!("archive: {}", archive_path.display());
    println!(
        "manifest: {}",
        archive_path.join("CLEAN_MANIFEST.txt").display()
    );
    println!("status: archived_and_cleaned");
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

fn credential_options(args: &CredentialArgs) -> CredentialOptions {
    CredentialOptions {
        cookie: args.cookie.clone(),
        sessdata: args.sessdata.clone(),
        anonymous: args.anonymous,
    }
}

fn credential_source(credentials: &CredentialOptions) -> String {
    if credentials.anonymous {
        return "anonymous (--anonymous)".to_string();
    }
    if let Some(path) = &credentials.cookie {
        return format!("cookie file: {}", path.display());
    }
    if credentials.sessdata.is_some() {
        return "--sessdata".to_string();
    }
    if default_cookie_header().ok().flatten().is_some() {
        format!("default cookie file: {DEFAULT_COOKIE_PATH}")
    } else {
        format!("anonymous (no {DEFAULT_COOKIE_PATH} found)")
    }
}

fn request_delay(milliseconds: u64) -> Option<Duration> {
    if milliseconds == 0 {
        None
    } else {
        Some(Duration::from_millis(milliseconds))
    }
}

fn collect_bvids(positional: &[String], input: &Option<PathBuf>) -> Result<Vec<String>> {
    let mut bvids = Vec::new();

    for value in positional {
        if let Some(bvid) = normalize_bvid_value_if_present(value)? {
            bvids.push(bvid);
        }
    }

    if let Some(path) = input {
        let content = fs::read_to_string(path)?;
        for value in content.lines() {
            if let Some(bvid) = normalize_bvid_value_if_present(value)? {
                bvids.push(bvid);
            }
        }
    }

    if bvids.is_empty() {
        bail!("provide at least one BVID or pass --input <FILE>");
    }

    Ok(bvids)
}

fn normalize_bvid_value(value: &str) -> Result<String> {
    normalize_bvid_input(value)
        .ok_or_else(|| anyhow::anyhow!("could not find a BVID in input: {}", value.trim()))
}

fn normalize_bvid_value_if_present(value: &str) -> Result<Option<String>> {
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        normalize_bvid_value(value).map(Some)
    }
}

fn comment_formats(formats: &[OutputFormat]) -> Vec<CommentOutputFormat> {
    formats
        .iter()
        .copied()
        .map(CommentOutputFormat::from)
        .collect()
}

#[derive(Debug, Serialize)]
struct FailureReportRow {
    #[serde(rename = "Kind")]
    kind: String,
    #[serde(rename = "Bvid")]
    bvid: String,
    #[serde(rename = "Error")]
    error: String,
}

fn write_failure_report(output_root: &Path, failures: &[CollectionFailure]) -> Result<PathBuf> {
    fs::create_dir_all(output_root)?;
    let path = output_root.join("failures.csv");
    let mut writer = csv::Writer::from_path(&path)?;
    for failure in failures {
        writer.serialize(FailureReportRow {
            kind: failure.kind.as_str().to_string(),
            bvid: failure.bvid.clone(),
            error: failure.error.clone(),
        })?;
    }
    writer.flush()?;
    Ok(path)
}

fn print_collection_job(job: &CollectionJobOutcome) {
    match job {
        CollectionJobOutcome::Comments(outcome) => {
            println!(
                "comments: scanned {} comments and appended {} new comments for {} (expected_total: {}, main_pages: {}, reply_pages: {}, next_cursor: {})",
                outcome.summary.comments_scanned,
                outcome.appended_count,
                outcome.bvid,
                outcome.expected_total,
                outcome.summary.main_pages_scanned,
                outcome.summary.reply_pages_scanned,
                outcome.summary.next_cursor.as_deref().unwrap_or("<none>")
            );
            for output in &outcome.outputs {
                println!(
                    "output: {} (appended: {})",
                    output.path.display(),
                    output.appended_count
                );
            }
        }
        CollectionJobOutcome::Danmaku(outcome) => {
            println!(
                "danmaku: scanned {} records and appended {} new records for {} (segments_scanned: {}, segments_appended: {})",
                outcome.records_scanned,
                outcome.records_appended,
                outcome.bvid,
                outcome.segments_scanned,
                outcome.segments_appended
            );
            println!("output: {}", outcome.record_path.display());
            println!(
                "segment_metadata: {}",
                outcome.segment_metadata_path.display()
            );
        }
    }
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

#[derive(Debug, Default)]
struct TreeSummary {
    files: usize,
    directories: usize,
    bytes: u64,
    paths: Vec<PathBuf>,
}

fn ensure_clean_target(root: &Path) -> Result<()> {
    if !root.is_dir() {
        bail!("clean target is not a directory: {}", root.display());
    }

    let current_dir = std::env::current_dir()?.canonicalize()?;
    let target = root.canonicalize()?;

    if target == current_dir {
        bail!("refusing to clean the workspace root: {}", root.display());
    }
    if !target.starts_with(&current_dir) {
        bail!(
            "clean target must be inside the current workspace: {}",
            root.display()
        );
    }
    if target.file_name().is_none() {
        bail!("refusing to clean filesystem root: {}", root.display());
    }

    Ok(())
}

fn summarize_tree(root: &Path) -> Result<TreeSummary> {
    let mut summary = TreeSummary::default();
    summarize_tree_entry(root, root, &mut summary)?;
    Ok(summary)
}

fn summarize_tree_entry(root: &Path, path: &Path, summary: &mut TreeSummary) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        let relative_path = path.strip_prefix(root).unwrap_or(&path).to_path_buf();

        if metadata.is_dir() {
            summary.directories += 1;
            summary.paths.push(relative_path);
            summarize_tree_entry(root, &path, summary)?;
        } else {
            summary.files += 1;
            summary.bytes += metadata.len();
            summary.paths.push(relative_path);
        }
    }
    Ok(())
}

fn unique_archive_path(archive_root: &Path, root: &Path) -> Result<PathBuf> {
    let root_name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(DEFAULT_OUTPUT_ROOT);
    let timestamp = current_utc_archive_timestamp()?;
    let base_name = format!("{root_name}-archive-{timestamp}");
    let mut candidate = archive_root.join(&base_name);
    let mut suffix = 1;

    while candidate.exists() {
        candidate = archive_root.join(format!("{base_name}-{suffix}"));
        suffix += 1;
    }

    Ok(candidate)
}

fn current_utc_archive_timestamp() -> Result<String> {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok(format_unix_timestamp_utc(seconds))
}

fn format_unix_timestamp_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = (seconds % 86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}Z")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }

    (year as i32, month as u32, day as u32)
}

fn write_clean_manifest(
    archive_path: &Path,
    original_root: &Path,
    summary: &TreeSummary,
) -> Result<()> {
    let mut content = String::new();
    content.push_str("Bilibili Opinion Insights output cleanup manifest\n");
    content.push_str(&format!("original_root: {}\n", original_root.display()));
    content.push_str(&format!("archive_path: {}\n", archive_path.display()));
    content.push_str(&format!("files: {}\n", summary.files));
    content.push_str(&format!("directories: {}\n", summary.directories));
    content.push_str(&format!("bytes: {}\n", summary.bytes));
    content.push_str("\nArchived paths:\n");
    for path in &summary.paths {
        content.push_str(&format!("{}\n", path.display()));
    }

    fs::write(archive_path.join("CLEAN_MANIFEST.txt"), content)?;
    Ok(())
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
    fn parses_run_command_with_default_collection_plan() {
        let cli = Cli::parse_from(["bili-opinion", "run", "BV1yEEq68EQ3"]);

        let Commands::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.bvids, ["BV1yEEq68EQ3"]);
        assert!(!args.comments_only);
        assert!(!args.danmaku_only);
        assert_eq!(args.format, [OutputFormat::Csv, OutputFormat::Jsonl]);
        assert_eq!(args.output, PathBuf::from(DEFAULT_OUTPUT_ROOT));
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
        assert!(args.credentials.cookie.is_none());
        assert!(args.credentials.sessdata.is_none());
    }

    #[test]
    fn parses_auth_command() {
        let cli = Cli::parse_from(["bili-opinion", "auth", "--sessdata", "sample"]);

        let Commands::Auth(args) = cli.command else {
            panic!("expected auth command");
        };

        assert!(args.credentials.cookie.is_none());
        assert_eq!(args.credentials.sessdata.as_deref(), Some("sample"));
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
    fn combines_positional_and_input_bvids() {
        let path =
            std::env::temp_dir().join(format!("bili-opinion-bvids-{}.txt", std::process::id()));
        fs::write(
            &path,
            "\nhttps://www.bilibili.com/video/BV1xx411c7mD\nBV17LwAzoEgw\n",
        )
        .expect("write input");

        let bvids =
            collect_bvids(&["BV1yEEq68EQ3".to_string()], &Some(path.clone())).expect("bvids");

        fs::remove_file(path).expect("remove input");

        assert_eq!(bvids, ["BV1yEEq68EQ3", "BV1xx411c7mD", "BV17LwAzoEgw"]);
    }

    #[test]
    fn rejects_input_without_bvid() {
        let error = collect_bvids(&["https://www.bilibili.com/video/av123".to_string()], &None)
            .expect_err("invalid input should fail");

        assert!(error.to_string().contains("could not find a BVID in input"));
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

    #[test]
    fn clean_archive_path_uses_output_name() {
        let archive_root = std::env::temp_dir().join("bili-opinion-archives");
        let archive =
            unique_archive_path(&archive_root, Path::new("output")).expect("archive path");

        assert!(
            archive
                .file_name()
                .and_then(|value| value.to_str())
                .expect("file name")
                .starts_with("output-archive-")
        );
    }

    #[test]
    fn formats_unix_timestamp_as_utc_archive_label() {
        assert_eq!(format_unix_timestamp_utc(1_781_202_781), "20260611-183301Z");
    }
}
