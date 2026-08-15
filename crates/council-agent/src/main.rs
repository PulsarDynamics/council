use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "council-agent",
    about = "Council agent process: subscribes to a Redis channel, runs an LLM loop, emits events.",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
enum Cmd {
    /// Load the agent TOML and run the LLM loop.
    Run {
        /// Path to the agent TOML config (e.g., `agents/planner.toml`).
        #[arg(long, env = "COUNCIL_AGENT_CONFIG")]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { config } => council_agent::run(&config)
            .await
            .with_context(|| format!("agent run failed for config {}", config.display()))?,
    }
    Ok(())
}
