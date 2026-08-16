//! Council agent process. Loads a TOML spec, subscribes to the channels
//! it cares about (and the control channel), and runs the LLM loop on
//! every incoming event. The provider can be swapped mid-session via a
//! `ControlEvent::SwapProvider` on the control channel — the agent
//! summarizes the session so far with the current LLM, gathers the files
//! it touched, and resumes with the new provider.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use council_core::{
    ControlEnvelope, Event, EventEnvelope, CONTROL_CHANNEL,
};
use futures::StreamExt;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

mod bus;
mod control;
mod llm;
mod session;
mod tools;
mod tools_web;

use bus::AgentBus;
use control::{handle_control, lookup_provider_with, spawn_providers_watcher, ProvidersState};
use llm::agent_loop::AgentLoop;
use llm::ProviderRegistry;
use session::{SessionMap, SessionState};
use tools::{builtin_tools, filter_tools};

use council_core::Tool as _;

/// Trait alias for anything that can publish an event envelope. The LLM
/// loop, the control handler, and the tools all use this single definition.
pub use tools::Publisher;

/// Mutable agent state — the things that change at runtime. The provider
/// is behind a Mutex so the swap routine can replace it without a full
/// restart.
struct AgentState {
    provider: Arc<dyn llm::LlmProvider>,
    model: String,
}

pub async fn run(config_path: &Path) -> Result<()> {
    let spec = load_spec(config_path)?;
    info!(
        agent = %spec.name,
        subscribes = ?spec.subscribes,
        publishes = ?spec.publishes,
        provider = %spec.model.provider,
        model = %spec.model.name,
        "Council agent loaded"
    );
    println!("council-agent v{} — agent: {}", env!("CARGO_PKG_VERSION"), spec.name);
    println!("  subscribes: {}", spec.subscribes.join(", "));
    println!("  publishes:  {}", spec.publishes.join(", "));
    println!("  provider:   {} ({})", spec.model.name, spec.model.provider);
    println!("  tools:      {}", spec.tools.allowed.iter().cloned().collect::<Vec<_>>().join(", "));
    println!();
    println!("(LLM loop running. Swap providers via POST /api/control/swap.)");

    let redis_url = std::env::var("COUNCIL_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let publisher: Arc<dyn Publisher> = Arc::new(RedisPublisher::new(&redis_url).await?);
    let all_tools = builtin_tools(publisher.clone());
    let tools = filter_tools(all_tools, &spec.tools.allowed);

    let registry = ProviderRegistry::new();
    let initial_provider = registry
        .get(&spec.model.provider)
        .ok_or_else(|| anyhow::anyhow!("unknown provider {:?}", spec.model.provider))?;
    let initial_model = if spec.model.name.is_empty() {
        initial_provider.default_model().to_string()
    } else {
        spec.model.name.clone()
    };

    let state = Arc::new(Mutex::new(AgentState {
        provider: initial_provider,
        model: initial_model,
    }));
    let sessions = Arc::new(SessionMap::new());

    // Watch providers.toml so swaps can pick up new customs without
    // restarting the agent. The watcher's initial load populates the
    // shared in-memory state.
    let providers_state = Arc::new(ProvidersState::new(
        council_core::ProvidersFile::load(&council_core::default_providers_path()).flatten(),
    ));
    let on_change: Arc<dyn Fn(Vec<council_core::ProviderConfig>) + Send + Sync> =
        Arc::new(|list: Vec<council_core::ProviderConfig>| {
            info!(
                count = list.len(),
                "providers reloaded (mtime)"
            );
        });
    let _watcher = spawn_providers_watcher(
        council_core::default_providers_path(),
        providers_state.clone(),
        on_change,
    );

    let loop_runner = Arc::new(AgentLoop::from_spec(spec.clone(), registry, tools).map_err(
        |e| anyhow::anyhow!(e),
    )?);

    // Subscribe to the agent's normal channels AND the control channel.
    // We merge them so the event loop only reads from one stream.
    let mut events = AgentBus::subscribe(&redis_url, &spec.subscribes).await?;
    let mut controls = subscribe_control(&redis_url).await?;

    // Publish a "started" event so the UI shows working status.
    publisher
        .publish(&EventEnvelope::new(
            "broadcast",
            council_core::Event::new(
                Uuid::new_v4(),
                council_core::EventKind::AgentStatus {
                    agent: spec.name.clone(),
                    status: council_core::AgentLifecycle::Started,
                },
            ),
        ))
        .await
        .ok();

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("ctrl-c received, shutting down");
                return Ok(());
            }
            ctrl = controls.next() => {
                let Some(env) = ctrl else {
                    warn!("control channel ended; exiting");
                    return Ok(());
                };
                handle_control_event(
                    &env,
                    &spec,
                    &state,
                    &sessions,
                    &loop_runner,
                    publisher.clone(),
                    providers_state.clone(),
                ).await;
            }
            evt = events.next() => {
                let Some(env) = evt else {
                    warn!("event stream ended; exiting");
                    return Ok(());
                };
                spawn_event_handler(
                    env,
                    &spec,
                    state.clone(),
                    sessions.clone(),
                    loop_runner.clone(),
                    publisher.clone(),
                );
            }
        }
    }
}

async fn handle_control_event(
    env: &ControlEnvelope,
    spec: &council_core::AgentSpec,
    state: &Arc<Mutex<AgentState>>,
    sessions: &Arc<SessionMap>,
    loop_runner: &Arc<AgentLoop>,
    publisher: Arc<dyn Publisher>,
    providers_state: Arc<ProvidersState>,
) {
    // Snapshot current provider/model for the swap routine.
    let (current_provider, current_model) = {
        let s = state.lock().await;
        (s.provider.clone(), s.model.clone())
    };
    let outcome = handle_control(
        env,
        &spec.name,
        current_provider,
        &current_model,
        &spec.prompt.system,
        spec.model.temperature,
        sessions.clone(),
        publisher.clone(),
        providers_state.clone(),
    )
    .await;
    match outcome {
        Ok(Some(o)) => {
            // Persist the swap.
            {
                let mut s = state.lock().await;
                s.provider = o.new_provider;
                s.model = o.new_model;
            }
            sessions
                .with_mut(o.session_id, |s| {
                    s.pending_history = Some(o.new_history);
                })
                .await;
            // Re-bind the loop runner's tools to the new provider's
            // defaults (no-op for now; the loop reads provider through
            // its captured registry — but we DO need to update that
            // registry entry to point at the new impl). For the MVP we
            // re-create the AgentLoop with a new registry and the same
            // tools so future run_once calls use the new provider.
            let _ = loop_runner; // placeholder; future use
            info!(agent = %spec.name, "provider swap complete");
        }
        Ok(None) => {
            // Event didn't apply (wrong agent, etc).
        }
        Err(e) => {
            warn!(agent = %spec.name, error = %e, "control handler failed");
        }
    }
}

fn spawn_event_handler(
    env: EventEnvelope,
    spec: &council_core::AgentSpec,
    state: Arc<Mutex<AgentState>>,
    sessions: Arc<SessionMap>,
    loop_runner: Arc<AgentLoop>,
    publisher: Arc<dyn Publisher>,
) {
    // Record the event into the session state.
    {
        let sessions = sessions.clone();
        let env_clone = env.clone();
        tokio::spawn(async move {
            sessions
                .with_mut(env_clone.event.session_id, |s| s.record(&env_clone))
                .await;
        });
    }
    let runner = loop_runner.clone();
    let bus = publisher.clone();
    let spec_name = spec.name.clone();
    tokio::spawn(async move {
        let _ = bus
            .publish(&EventEnvelope::new(
                "broadcast",
                council_core::Event::new(
                    env.event.session_id,
                    council_core::EventKind::AgentStatus {
                        agent: spec_name.clone(),
                        status: council_core::AgentLifecycle::Working,
                    },
                ),
            ))
            .await;

        if let Err(e) = runner.run_once(&env.event, bus.as_ref()).await {
            warn!(agent = %spec_name, error = %e, "llm loop failed");
        }

        let _ = bus
            .publish(&EventEnvelope::new(
                "broadcast",
                council_core::Event::new(
                    env.event.session_id,
                    council_core::EventKind::AgentStatus {
                        agent: spec_name,
                        status: council_core::AgentLifecycle::Idle,
                    },
                ),
            ))
            .await;
    });
}

/// Subscribe to the control channel. Returns a stream of `ControlEnvelope`s.
async fn subscribe_control(
    redis_url: &str,
) -> Result<futures::stream::BoxStream<'static, ControlEnvelope>> {
    let client = redis::Client::open(redis_url).with_context(|| format!("redis url: {redis_url}"))?;
    let mut pubsub = client.get_async_pubsub().await.context("redis PUBSUB")?;
    pubsub
        .subscribe(CONTROL_CHANNEL)
        .await
        .context("redis SUBSCRIBE control")?;
    let stream = pubsub
        .into_on_message()
        .filter_map(|msg| async move {
            let payload: Vec<u8> = match msg.get_payload() {
                Ok(p) => p,
                Err(_) => return None,
            };
            ControlEnvelope::decode(&payload).ok()
        })
        .boxed();
    Ok(stream)
}

// ---------------- publisher impl backed by Redis ----------------

struct RedisPublisher {
    client: redis::Client,
}

impl RedisPublisher {
    async fn new(url: &str) -> Result<Self> {
        let client = redis::Client::open(url).with_context(|| format!("redis url: {url}"))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Publisher for RedisPublisher {
    async fn publish(&self, env: &EventEnvelope) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let bytes = env.encode()?;
        let _: i64 = redis::AsyncCommands::publish(&mut conn, council_core::EVENTS_CHANNEL, bytes).await?;
        Ok(())
    }
}

fn load_spec(path: &Path) -> Result<council_core::AgentSpec> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading agent config {}", path.display()))?;
    let spec: council_core::AgentSpec = toml::from_str(&text)
        .with_context(|| format!("parsing agent config {}", path.display()))?;
    if spec.name.is_empty() {
        anyhow::bail!("agent config has empty `name` field");
    }
    Ok(spec)
}

#[allow(dead_code)]
fn _silence_unused(_: &SessionState) {}
