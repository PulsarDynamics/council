//! Redis pub/sub envelope. The whole system runs on a single Redis channel
//! (`EVENTS_CHANNEL`); every event carries a `channel` field for routing.
//! Agents filter incoming envelopes by their TOML `subscribes` list.

use serde::{Deserialize, Serialize};

use crate::event::Event;

/// The single Redis channel everything flows through. Why one channel?
/// - One subscribe-stream per process is cheaper than N.
/// - Routing by envelope field lets us add a new agent (with a new
///   `subscribes` channel) without touching the orchestrator or Redis.
pub const EVENTS_CHANNEL: &str = "council:events";

/// Reserved channel names. Agent TOML `subscribes` and `publishes` should
/// pick from this list (or add a new one — they're free-form, but new names
/// should be documented in `docs/WIRE_CONTRACT.md`).
pub mod channels {
    /// The first hop: user goal -> Planner.
    pub const GOAL: &str = "goal";
    /// Planner -> Designer.
    pub const PLAN: &str = "plan";
    /// Designer -> Implementer.
    pub const SPEC: &str = "spec";
    /// Implementer -> "done".
    pub const RESULT: &str = "result";
    /// Any agent can publish here; every subscribed agent will pick it up.
    pub const BROADCAST: &str = "broadcast";
}

/// What goes on the wire on `EVENTS_CHANNEL`. JSON-encoded, UTF-8 bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Routing key. The orchestrator ignores this (forwards to all WS clients);
    /// agents filter by membership in their `subscribes` list.
    pub channel: String,
    /// The event itself. See `crate::event::Event`.
    pub event: Event,
}

impl EventEnvelope {
    pub fn new(channel: impl Into<String>, event: Event) -> Self {
        Self {
            channel: channel.into(),
            event,
        }
    }

    /// Serialize to the JSON UTF-8 bytes that go on the Redis channel.
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Decode from the bytes read off the Redis channel.
    pub fn decode(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventKind;
    use uuid::Uuid;

    #[test]
    fn envelope_roundtrips_through_json() {
        let env = EventEnvelope::new(
            channels::GOAL,
            Event::new(Uuid::new_v4(), EventKind::UserMessage { content: "hi".into() }),
        );
        let bytes = env.encode().unwrap();
        let back = EventEnvelope::decode(&bytes).unwrap();
        assert_eq!(env.channel, back.channel);
        assert_eq!(env.event.id, back.event.id);
    }
}
