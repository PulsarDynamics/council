use std::net::SocketAddr;

use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "council-orchestrator",
    about = "Council orchestrator server (Axum + Redis + agent process manager).",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
enum Cmd {
    /// Start the orchestrator HTTP/WS server.
    Serve {
        /// Address to bind on.
        #[arg(long, env = "COUNCIL_BIND", default_value = "0.0.0.0:8080")]
        bind: SocketAddr,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Serve { bind } => council_orchestrator::serve(bind)
            .await
            .with_context(|| format!("orchestrator serve failed on {bind}"))?,
    }
    Ok(())
}
