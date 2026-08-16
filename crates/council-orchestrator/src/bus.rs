//! Redis bus client for the orchestrator. Owns the publish side and the
//! subscribe side of `EVENTS_CHANNEL`. Envelopes are JSON over the wire.

use anyhow::{Context, Result};
use council_core::{ControlEnvelope, EventEnvelope, CONTROL_CHANNEL};
use futures::StreamExt;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use tracing::{debug, warn};

/// Long-lived Redis client. Cheap to clone (it's an `Arc` internally).
#[derive(Clone)]
pub struct Bus {
    conn: ConnectionManager,
}

impl Bus {
    /// Connect to Redis and verify the connection with a PING.
    pub async fn connect(url: &str) -> Result<Self> {
        let client = redis::Client::open(url).with_context(|| format!("invalid redis url: {url}"))?;
        let conn = ConnectionManager::new(client)
            .await
            .with_context(|| format!("failed to connect to redis at {url}"))?;
        Ok(Self { conn })
    }

    /// Publish an envelope on `EVENTS_CHANNEL`.
    pub async fn publish(&self, envelope: &EventEnvelope) -> Result<()> {
        let bytes = envelope.encode().context("serializing envelope")?;
        let mut conn = self.conn.clone();
        let _: i64 = conn
            .publish(council_core::EVENTS_CHANNEL, bytes)
            .await
            .context("redis PUBLISH")?;
        debug!(channel = %envelope.channel, event_id = %envelope.event.id, "published");
        Ok(())
    }

    /// Publish a control envelope on the control channel. The agent
    /// subscribes to this separately from the events channel.
    pub async fn publish_control(&self, envelope: &ControlEnvelope) -> Result<()> {
        let bytes = envelope.encode().context("serializing control envelope")?;
        let mut conn = self.conn.clone();
        let _: i64 = conn
            .publish(CONTROL_CHANNEL, bytes)
            .await
            .context("redis PUBLISH control")?;
        debug!(control = ?envelope.event, "published control");
        Ok(())
    }

    /// Subscribe to `EVENTS_CHANNEL` and yield each envelope as it arrives.
    /// The returned stream lives for the duration of the caller's task; if
    /// the Redis connection drops it will end (the caller should reconnect).
    pub async fn subscribe(&self) -> Result<futures::stream::BoxStream<'static, EventEnvelope>> {
        let client = redis::Client::open(
            std::env::var("COUNCIL_REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
        )?;
        let mut pubsub = client.get_async_pubsub().await.context("redis PUBSUB")?;
        pubsub
            .subscribe(council_core::EVENTS_CHANNEL)
            .await
            .context("redis SUBSCRIBE")?;
        warn!(channel = council_core::EVENTS_CHANNEL, "subscribed to redis");

        let stream = pubsub
            .into_on_message()
            .filter_map(|msg| async move {
                let payload: Vec<u8> = match msg.get_payload() {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(error = %e, "non-bytes payload on events channel");
                        return None;
                    }
                };
                match EventEnvelope::decode(&payload) {
                    Ok(env) => Some(env),
                    Err(e) => {
                        warn!(error = %e, "failed to decode envelope; dropping");
                        None
                    }
                }
            })
            .boxed();

        Ok(stream)
    }
}
