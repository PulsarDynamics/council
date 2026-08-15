//! Council agent library. The binary entry point is in `main.rs`.

use std::path::Path;

use anyhow::{Context, Result};
use council_core::AgentSpec;
use tracing::info;

/// Load an agent spec from a TOML file and report its contents. The scaffold
/// reads the config and exits; the real Redis subscription + LLM loop lands
/// in a later cycle.
pub async fn run(config_path: &Path) -> Result<()> {
    let spec = load_spec(config_path)?;
    info!(
        agent = %spec.name,
        subscribes = ?spec.subscribes,
        publishes = ?spec.publishes,
        model = %spec.model.name,
        "Council agent loaded"
    );
    println!("council-agent v{} — agent: {}", env!("CARGO_PKG_VERSION"), spec.name);
    println!("  subscribes: {}", spec.subscribes.join(", "));
    println!("  publishes:  {}", spec.publishes.join(", "));
    println!("  model:      {} ({})", spec.model.name, spec.model.provider);
    println!("  tools:      {}", spec.tools.allowed.iter().cloned().collect::<Vec<_>>().join(", "));
    println!();
    println!("(scaffold: real LLM loop lands in cycle 2. Ctrl-C to exit.)");

    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    Ok(())
}

fn load_spec(path: &Path) -> Result<AgentSpec> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading agent config {}", path.display()))?;
    let spec: AgentSpec = toml::from_str(&text)
        .with_context(|| format!("parsing agent config {}", path.display()))?;
    if spec.name.is_empty() {
        anyhow::bail!("agent config has empty `name` field");
    }
    Ok(spec)
}
