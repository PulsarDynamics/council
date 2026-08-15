//! Redis subscriber for an agent. Connects to Redis, listens on
//! `EVENTS_CHANNEL`, and logs every event whose routing channel matches
//! the agent's `subscribes` list. The LLM loop (cycle 3) will replace
//! the log with a real handler.

use anyhow::{Context, Result};
use council_core::{EventEnvelope, EVENTS_CHANNEL};
use futures::StreamExt;
use redis::AsyncCommands;
use tracing::{info, warn};

pub struct AgentBus;

impl AgentBus {
    /// Subscribe to the events channel and yield every envelope whose
    /// `channel` field is in `subscribes`.
    pub async fn subscribe(
        redis_url: &str,
        subscribes: &[String],
    ) -> Result<futures::stream::BoxStream<'static, EventEnvelope>> {
        let client = redis::Client::open(redis_url)
            .with_context(|| format!("invalid redis url: {redis_url}"))?;
        let mut pubsub = client.get_async_pubsub().await.context("redis PUBSUB")?;
        pubsub.subscribe(EVENTS_CHANNEL).await.context("redis SUBSCRIBE")?;
        info!(channel = EVENTS_CHANNEL, "agent subscribed to events");

        // Pre-compute a fast lookup set.
        let subs: std::collections::BTreeSet<String> = subscribes.iter().cloned().collect();

        let stream = pubsub
            .into_on_message()
            .filter_map(move |msg| {
                let subs = subs.clone();
                async move {
                    let payload: Vec<u8> = match msg.get_payload() {
                        Ok(p) => p,
                        Err(e) => {
                            warn!(error = %e, "non-bytes payload");
                            return None;
                        }
                    };
                    let env = match EventEnvelope::decode(&payload) {
                        Ok(e) => e,
                        Err(e) => {
                            warn!(error = %e, "failed to decode envelope");
                            return None;
                        }
                    };
                    if subs.contains(&env.channel) {
                        Some(env)
                    } else {
                        None
                    }
                }
            })
            .boxed();

        Ok(stream)
    }
}

/// Helper to publish a single envelope. Useful for tests and (later) the
/// agent's response path. Public so other crates can publish from tests.
#[allow(dead_code)]
pub async fn publish(redis_url: &str, env: &EventEnvelope) -> Result<()> {
    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    let bytes = env.encode()?;
    let _: i64 = conn.publish(EVENTS_CHANNEL, bytes).await?;
    Ok(())
}
