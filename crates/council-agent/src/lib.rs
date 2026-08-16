//! Council agent process. Loads a TOML spec, subscribes to the channels
//! it cares about, and runs the LLM loop on every incoming event. The
//! loop builds a chat history, calls the LLM, publishes its response
//! (and any tool calls) back to the bus, and iterates until the LLM says
//! end-of-turn.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use council_core::{AgentSpec, EventEnvelope};
use futures::StreamExt;
use tracing::{info, warn};
use uuid::Uuid;

mod bus;
mod llm;
mod tools;

use bus::AgentBus;
use llm::ProviderRegistry;
use tools::{builtin_tools, filter_tools};

use council_core::Tool as _;

/// Trait alias for anything that can publish an event envelope. The LLM
/// loop and the tools both use this single definition.
pub use tools::Publisher;

/// Load an agent spec, subscribe to its channels, and run the LLM loop on
/// every event until Ctrl+C.
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
    println!("(LLM loop running. Events will be published on the configured channels.)");

    let redis_url = std::env::var("COUNCIL_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    // Publisher for the loop and the tools.
    let publisher: Arc<dyn Publisher> = Arc::new(RedisPublisher::new(&redis_url).await?);
    let all_tools = builtin_tools(publisher.clone());
    let tools = filter_tools(all_tools, &spec.tools.allowed);

    // Build the provider registry. Built-ins are auto-registered.
    let registry = ProviderRegistry::new();

    let loop_runner = Arc::new(
        llm::agent_loop::AgentLoop::from_spec(spec.clone(), registry, tools)
            .map_err(|e| anyhow::anyhow!(e))?,
    );

    let bus_publisher: Arc<dyn Publisher> = publisher.clone();

    let mut stream = AgentBus::subscribe(&redis_url, &spec.subscribes)
        .await
        .with_context(|| format!("subscribing to redis at {redis_url}"))?;

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

    while let Some(env) = stream.next().await {
        let runner = loop_runner.clone();
        let bus = bus_publisher.clone();
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

    info!("redis subscription ended");
    Ok(())
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
