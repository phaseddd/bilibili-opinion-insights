use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};

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
    /// Collect comments for one or more Bilibili videos.
    Comments(CommentsArgs),
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
        Commands::Comments(args) => run_comments(args).await,
    }
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

        let Commands::Comments(args) = cli.command;
        assert_eq!(args.bvids, ["BV1xx411c7mD"]);
        assert_eq!(args.format, [OutputFormat::Csv, OutputFormat::Jsonl]);
        assert_eq!(args.output, PathBuf::from("output"));
    }
}
