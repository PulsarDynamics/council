//! Control plane. The control channel is separate from the events channel
//! and carries `ControlEvent`s — instructions to the agents (or the
//! orchestrator) that are not part of the deliberation stream.
//!
//! Today: just `SwapProvider`. The point is to let a user change a running
//! agent's LLM mid-session without losing state. The agent responds by
//! summarizing what it has done, gathering any files it touched, and
//! continuing with the new provider/model.

use serde::{Deserialize, Serialize};

/// Redis channel for control events. Separate from `EVENTS_CHANNEL` so
/// regular subscribers don't see control traffic.
pub const CONTROL_CHANNEL: &str = "council:control";

/// Control plane envelope. JSON-encoded, same shape as `EventEnvelope`
/// (channel + event) but the event here is a `ControlEvent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlEnvelope {
    pub event: ControlEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlEvent {
    /// "Switch this agent to a new LLM provider/model. Keep going."
    SwapProvider {
        /// Name of the agent that should swap. The orchestrator and the
        /// UI also publish these for their own state.
        agent: String,
        /// New provider name (matches a registered provider, e.g. "openai",
        /// "anthropic", or a user-added custom name).
        provider: String,
        /// New model. If absent, the provider's `default_model` is used.
        #[serde(default)]
        model: Option<String>,
        /// Why the swap is happening — surfaces in the UI as a note.
        #[serde(default)]
        reason: Option<String>,
    },
    /// "Reset this agent's accumulated session state." Used after a
    /// successful handoff to free memory, or by the user to manually
    /// clear.
    ResetSession {
        agent: String,
    },
    /// "The providers file changed — reload it." Sent by the orchestrator
    /// right after a POST/DELETE on /api/providers. The agent ALSO
    /// watches the file via `notify` (so hand-edits are picked up
    /// too); this event is just a fast-path.
    #[serde(rename = "providers_changed")]
    ProvidersChanged,
}

impl ControlEnvelope {
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_roundtrips() {
        let env = ControlEnvelope {
            event: ControlEvent::SwapProvider {
                agent: "planner".into(),
                provider: "anthropic".into(),
                model: Some("claude-sonnet-4-5".into()),
                reason: Some("cost".into()),
            },
        };
        let bytes = env.encode().unwrap();
        let back = ControlEnvelope::decode(&bytes).unwrap();
        match back.event {
            ControlEvent::SwapProvider { agent, provider, model, reason } => {
                assert_eq!(agent, "planner");
                assert_eq!(provider, "anthropic");
                assert_eq!(model.as_deref(), Some("claude-sonnet-4-5"));
                assert_eq!(reason.as_deref(), Some("cost"));
            }
            _ => panic!("wrong variant"),
        }
    }
}
