//! Council orchestrator. Boots an Axum HTTP/WS server, spawns one
//! `council-agent` subprocess per TOML config under `agents/`, forwards
//! Redis pub/sub events to WebSocket clients, and accepts goal
//! submissions on `POST /api/sessions`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    routing::{get, post},
    Router,
};
use council_core::EventEnvelope;
use futures::StreamExt;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

mod agents;
mod api;
mod bus;
mod state;
mod ws;

use agents::{AgentEvent, ProcessManager};
use bus::Bus;
use state::AppState;

/// Start the orchestrator. Blocks until `bind` errors or `Ctrl+C`.
pub async fn serve(bind: SocketAddr) -> Result<()> {
    let redis_url = std::env::var("COUNCIL_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let bus = Bus::connect(&redis_url)
        .await
        .with_context(|| format!("connecting to redis at {redis_url}"))?;

    // Load + spawn agent subprocesses.
    let agents_dir = std::env::var("COUNCIL_AGENTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("agents"));
    let agent_bin = std::env::var("COUNCIL_AGENT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("council-agent"));

    let mut pm = ProcessManager::load(&agents_dir, agent_bin)
        .with_context(|| format!("loading agents from {}", agents_dir.display()))?;
    info!(count = pm.len(), dir = %agents_dir.display(), "loaded agents");

    let (pm_tx, mut pm_rx) = tokio::sync::mpsc::channel::<AgentEvent>(256);
    pm.start_all(pm_tx);
    spawn_agent_event_logger(&mut pm_rx);

    // Build the shared state and start forwarding Redis events to it.
    let state = AppState::new(bus.clone());
    spawn_event_forwarder(bus.clone(), state.events_tx.clone());
    spawn_event_persister(bus.clone(), state.clone());

    // HTTP + WS router.
    let app = Router::new()
        .route("/health", get(api::health))
        .route("/version", get(api::version))
        .route("/api/agents", get(api::list_agents))
        .route("/api/sessions", get(api::list_sessions).post(api::submit_goal))
        .route("/api/sessions/:id/events", get(api::get_session_events))
        .route("/api/control/swap", post(api::swap_provider))
        .route("/api/control/reset", post(api::reset_session))
        .route("/api/control/cancel", post(api::cancel_session))
        .route("/api/providers", get(api::get_providers).post(api::upsert_provider))
        .route("/api/providers/:name", axum::routing::delete(api::delete_provider))
        .route("/ws", get(ws::ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding to {bind}"))?;
    info!(%bind, "Council orchestrator listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("axum serve")?;

    info!("shutting down: stopping agent processes");
    pm.shutdown().await;
    Ok(())
}

/// Subscribe to the bus and forward every envelope to the in-process
/// broadcast channel. Exits if the Redis stream ends.
fn spawn_event_forwarder(bus: Bus, tx: tokio::sync::broadcast::Sender<EventEnvelope>) {
    tokio::spawn(async move {
        let mut stream = match bus.subscribe().await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "failed to subscribe to bus");
                return;
            }
        };
        while let Some(env) = stream.next().await {
            // Best-effort: if no WS clients are listening, the send errors
            // and we drop the envelope. That's fine — broadcast is lossy
            // by design and a slow client shouldn't backpressure the bus.
            let _ = tx.send(env);
        }
        warn!("bus subscription stream ended");
    });
}

/// Same subscription, but write each event to Redis so the history
/// endpoints can serve past sessions. TTL'd to 24h; older sessions
/// fall off the sidebar automatically.
fn spawn_event_persister(bus: Bus, state: Arc<state::AppState>) {
    tokio::spawn(async move {
        let mut stream = match bus.subscribe().await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "event persister: failed to subscribe");
                return;
            }
        };
        while let Some(env) = stream.next().await {
            if let Err(e) = api::persist_event(&state, &env).await {
                warn!(error = %e, "event persister: persist_event failed");
            }
        }
        warn!("event persister: bus stream ended");
    });
}

/// Log agent lifecycle events to the orchestrator's tracing subscriber.
fn spawn_agent_event_logger(rx: &mut tokio::sync::mpsc::Receiver<AgentEvent>) {
    let mut rx = std::mem::replace(rx, tokio::sync::mpsc::channel(1).1);
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::Started { name } => info!(agent = %name, "agent process started"),
                AgentEvent::Stdout { name, line } => {
                    info!(agent = %name, "{}", line);
                }
                AgentEvent::Exited { name, code } => {
                    warn!(agent = %name, code = ?code, "agent process exited");
                }
                AgentEvent::Restarting { name, attempt, delay_ms } => {
                    info!(agent = %name, attempt, delay_ms, "agent restarting");
                }
                AgentEvent::Failed { name, error } => {
                    tracing::error!(agent = %name, error, "agent process failed");
                }
            }
        }
    });
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received");
}

/// Print the version banner. Used by the binary entry point and tests.
pub fn print_banner() {
    println!(
        "council-orchestrator v{} (rust {})",
        env!("CARGO_PKG_VERSION"),
        rustc_version_runtime(),
    );
}

fn rustc_version_runtime() -> &'static str {
    // Compile-time, but cheap to expose.
    env!("CARGO_PKG_RUST_VERSION")
}

/// Resolve the default path to the agent binary, next to the current exe.
pub fn default_agent_bin() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .map(|d| d.join(format!("council-agent{}", std::env::consts::EXE_SUFFIX)))
        .unwrap_or_else(|| PathBuf::from("council-agent"))
}

/// Init tracing once. Idempotent — safe to call from multiple entry points.
pub fn init_tracing() {
    use tracing_subscriber::filter::LevelFilter;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,council_core=debug"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_max_level(LevelFilter::DEBUG)
        .try_init();
}
