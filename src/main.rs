use anyhow::Result;
use bili_opinion::cli::{Cli, run};
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .without_time()
        .init();

    run(Cli::parse()).await
}
