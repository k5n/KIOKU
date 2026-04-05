use clap::Parser;
use evaluate::cli::{Cli, run_cli};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    run_cli(Cli::parse()).await
}
