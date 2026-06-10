use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::app::comments::{CommentCollectionOptions, CommentOutputFormat, collect_video_comments};
use crate::app::danmaku::{DanmakuCollectionOptions, collect_video_danmaku};
use crate::bili::auth::{QrLoginStatus, render_terminal_qr};
use crate::bili::client::BiliClient;
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
    let options = CommentCollectionOptions {
        output: args.output.clone(),
        formats: args
            .format
            .iter()
            .copied()
            .map(CommentOutputFormat::from)
            .collect(),
        max_pages: args.max_pages,
        max_reply_pages: args.max_reply_pages,
        request_delay: request_delay(args.request_delay_ms),
    };

    for bvid in bvids {
        match collect_video_comments(&client, &bvid, &options).await {
            Ok(outcome) => {
                println!(
                    "scanned {} comments and appended {} new comments for {} (expected_total: {}, main_pages: {}, reply_pages: {}, next_cursor: {})",
                    outcome.summary.comments_scanned,
                    outcome.appended_count,
                    outcome.bvid,
                    outcome.expected_total,
                    outcome.summary.main_pages_scanned,
                    outcome.summary.reply_pages_scanned,
                    outcome.summary.next_cursor.as_deref().unwrap_or("<none>")
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
    let options = DanmakuCollectionOptions {
        output: args.output.clone(),
        max_segments: args.max_segments,
        request_delay: request_delay(args.request_delay_ms),
    };

    for bvid in bvids {
        match collect_video_danmaku(&client, &bvid, &options).await {
            Ok(outcome) => {
                println!(
                    "scanned {} danmaku records and appended {} new records for {} (segments_scanned: {}, segments_appended: {})",
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

#[derive(Debug, Serialize)]
struct VideoFailure {
    #[serde(rename = "Bvid")]
    bvid: String,
    #[serde(rename = "Error")]
    error: String,
}

fn request_delay(milliseconds: u64) -> Option<Duration> {
    if milliseconds == 0 {
        None
    } else {
        Some(Duration::from_millis(milliseconds))
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
}
