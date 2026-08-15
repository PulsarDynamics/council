//! Council agent process. Loads a TOML config, subscribes to Redis on
//! the channels the agent cares about, and runs an event loop that (for
//! now) just logs each incoming event. The LLM-driven response loop
//! lands in cycle 3.

use std::path::Path;

use anyhow::{Context, Result};
use council_core::{AgentSpec, EventKind};
use futures::StreamExt;
use tracing::info;

mod bus;

use bus::AgentBus;

/// Load an agent spec from a TOML file, subscribe to its channels, and
/// process events until Ctrl+C.
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
    println!("(scaffold: LLM loop lands in cycle 3. Incoming events are logged below.)");

    let redis_url = std::env::var("COUNCIL_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let mut stream = AgentBus::subscribe(&redis_url, &spec.subscribes)
        .await
        .with_context(|| format!("subscribing to redis at {redis_url}"))?;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("ctrl-c received, shutting down");
                return Ok(());
            }
            env = stream.next() => {
                let Some(env) = env else {
                    info!("redis subscription ended");
                    return Ok(());
                };
                handle_event(&spec, &env.event);
            }
        }
    }
}

/// Stub event handler: log it. Cycle 3 will replace this with an LLM call.
fn handle_event(spec: &AgentSpec, event: &council_core::Event) {
    let kind = match &event.kind {
        EventKind::UserMessage { content } => format!("user_message({})", truncate(content, 60)),
        EventKind::AgentMessage { agent, content } => {
            format!("agent_message({}): {}", agent, truncate(content, 60))
        }
        EventKind::AgentThinking { agent, content } => {
            format!("agent_thinking({}): {}", agent, truncate(content, 60))
        }
        EventKind::ToolCall { agent, tool, .. } => format!("tool_call({}.{})", agent, tool),
        EventKind::ToolResult { agent, tool, error, .. } => {
            if let Some(err) = error {
                format!("tool_result({}.{}) ERROR: {}", agent, tool, err)
            } else {
                format!("tool_result({}.{})", agent, tool)
            }
        }
        EventKind::FileChange { path, kind, .. } => format!("file_change({:?}: {})", kind, path),
        EventKind::AgentStatus { agent, status } => {
            format!("agent_status({}.{:?})", agent, status)
        }
        EventKind::LlmCall {
            agent,
            model,
            prompt_tokens,
            completion_tokens,
            duration_ms,
        } => format!(
            "llm_call({}.{}: {} in / {} out, {} ms)",
            agent, model, prompt_tokens, completion_tokens, duration_ms
        ),
        EventKind::System { message } => format!("system: {}", message),
        EventKind::SessionCreated { goal } => format!("session_created: {}", truncate(goal, 60)),
        EventKind::SessionCompleted { summary } => {
            format!("session_completed: {}", truncate(summary, 60))
        }
        EventKind::Error { source, message } => format!("error({}): {}", source, message),
    };
    info!(agent = %spec.name, session = %event.session_id, "received: {}", kind);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
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
