use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};

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
    if args.bvids.is_empty() && args.input.is_none() {
        bail!("provide at least one BVID or pass --input <FILE>");
    }

    tracing::info!(
        bvid_count = args.bvids.len(),
        has_input = args.input.is_some(),
        output = %args.output.display(),
        "comment collection command parsed"
    );

    bail!("comment collection is not implemented yet; this milestone only scaffolds the CLI")
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
}
