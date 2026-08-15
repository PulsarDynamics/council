//! The 12-type event wire contract.
//!
//! Every event has a stable `id` (UUID v4), a `session_id` it belongs to, an
//! ISO-8601 `timestamp`, and a `kind` discriminator. Adding a new event kind
//! is a breaking change to the wire contract — bump the version in
//! `docs/WIRE_CONTRACT.md` and update the UI's TypeScript types in the same PR.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type EventId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub session_id: Uuid,
    pub kind: EventKind,
    pub timestamp: DateTime<Utc>,
}

impl Event {
    pub fn new(session_id: Uuid, kind: EventKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            session_id,
            kind,
            timestamp: Utc::now(),
        }
    }
}

/// The 12 event kinds that flow over Redis pub/sub and over the WebSocket to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    /// A message from the human user.
    UserMessage { content: String },
    /// A message emitted by an agent into the conversation.
    AgentMessage { agent: String, content: String },
    /// An agent's intermediate reasoning (shown collapsed in the UI by default).
    AgentThinking { agent: String, content: String },
    /// An agent invoked a tool.
    ToolCall { agent: String, tool: String, args: serde_json::Value },
    /// Result of a tool invocation.
    ToolResult {
        agent: String,
        tool: String,
        result: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A file was created, modified, or deleted.
    FileChange {
        path: String,
        kind: FileChangeKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
    },
    /// Lifecycle status of an agent process.
    AgentStatus { agent: String, status: AgentLifecycle },
    /// An LLM call was made (token usage + duration for observability).
    LlmCall {
        agent: String,
        model: String,
        prompt_tokens: u32,
        completion_tokens: u32,
        duration_ms: u64,
    },
    /// System-level message (startup, shutdown, config changes, etc.).
    System { message: String },
    /// A new session was created with a goal.
    SessionCreated { goal: String },
    /// A session completed (with a human-readable summary).
    SessionCompleted { summary: String },
    /// An error occurred somewhere in the system.
    Error { source: String, message: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycle {
    Started,
    Idle,
    Working,
    Stopped,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_roundtrips_through_json() {
        let event = Event::new(
            Uuid::new_v4(),
            EventKind::UserMessage { content: "hello".into() },
        );
        let json = serde_json::to_string(&event).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event.id, back.id);
        assert_eq!(event.session_id, back.session_id);
    }

    #[test]
    fn event_kind_uses_snake_case_tag() {
        let event = Event::new(
            Uuid::new_v4(),
            EventKind::AgentMessage { agent: "planner".into(), content: "hi".into() },
        );
        let v: serde_json::Value = serde_json::to_value(&event).unwrap();
        assert_eq!(v["kind"]["type"], "agent_message");
    }
}
