//! Per-session agent state. The LLM loop is normally stateless (it
//! builds a fresh chat history per `run_once` call), but for the
//! provider-swap handoff we need to remember:
//! - All events seen (so we can summarize)
//! - Files the agent has touched (so we can pull them into the new context)
//! - A pending "next history" the LLM loop should pick up on its next
//!   call (the summary + files the swap routine produced)
//!
//! The state is in-memory; it's lost if the agent process restarts. That's
//! fine — a restart means a fresh session anyway.

use std::collections::BTreeSet;

use council_core::{EventEnvelope, EventKind, SessionId};
use tokio::sync::Mutex;

use crate::llm::ChatMessage;

/// One session's worth of state. Public so tests can poke at it.
#[derive(Debug, Default)]
pub struct SessionState {
    /// All events we've seen for this session, in arrival order.
    pub events: Vec<EventEnvelope>,
    /// Files the agent (or earlier agents) have touched — captured from
    /// `file_change` events. The swap routine reads these to seed the
    /// new context.
    pub files_touched: BTreeSet<String>,
    /// Pending history to seed the next `run_once` call. Set by the swap
    /// routine; consumed by the LLM loop on its next call.
    pub pending_history: Option<Vec<ChatMessage>>,
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an event and update derived state.
    pub fn record(&mut self, env: &EventEnvelope) {
        if let EventKind::FileChange { path, .. } = &env.event.kind {
            self.files_touched.insert(path.clone());
        }
        self.events.push(env.clone());
    }

    /// Build a human-readable context dump: event kinds + short contents.
    /// The swap routine hands this to the current LLM and asks for a
    /// summary.
    pub fn context_dump(&self) -> String {
        let mut out = String::new();
        for env in &self.events {
            let e = &env.event;
            let kind = match &e.kind {
                EventKind::UserMessage { content } => format!("user: {content}"),
                EventKind::AgentMessage { agent, content } => {
                    format!("{agent}: {}", truncate(content, 240))
                }
                EventKind::AgentThinking { agent, .. } => {
                    format!("{agent} (thinking)")
                }
                EventKind::ToolCall { agent, tool, .. } => {
                    format!("{agent} -> {tool}()")
                }
                EventKind::ToolResult { agent, tool, error, .. } => {
                    if let Some(err) = error {
                        format!("{agent} <- {tool} (err: {err})")
                    } else {
                        format!("{agent} <- {tool}()")
                    }
                }
                EventKind::FileChange { path, kind, .. } => {
                    format!("file_change: {kind:?} {path}")
                }
                EventKind::AgentStatus { agent, status } => {
                    format!("{agent} -> {status:?}")
                }
                EventKind::LlmCall { agent, model, prompt_tokens, completion_tokens, .. } => {
                    format!("{agent} llm_call {model} ({prompt_tokens} in / {completion_tokens} out)")
                }
                EventKind::System { message } => format!("system: {message}"),
                EventKind::SessionCreated { goal } => format!("session_created: {goal}"),
                EventKind::SessionCompleted { summary } => format!("session_completed: {summary}"),
                EventKind::Error { source, message } => format!("error({source}): {message}"),
            };
            out.push_str("- ");
            out.push_str(&kind);
            out.push('\n');
        }
        out
    }

    /// Clear everything (used by the ResetSession control event).
    pub fn reset(&mut self) {
        self.events.clear();
        self.files_touched.clear();
        self.pending_history = None;
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// Thread-safe per-session state. Indexed by session id.
#[derive(Default)]
pub struct SessionMap {
    pub inner: Mutex<std::collections::HashMap<SessionId, SessionState>>,
}

impl SessionMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lock and run a closure with the state for the given session,
    /// creating an empty one if it doesn't exist yet.
    pub async fn with_mut<F, R>(&self, session_id: SessionId, f: F) -> R
    where
        F: FnOnce(&mut SessionState) -> R,
    {
        let mut guard = self.inner.lock().await;
        let state = guard.entry(session_id).or_insert_with(SessionState::new);
        f(state)
    }

    /// Take the pending history (if any) for the given session. Returns
    /// the messages; clears the slot.
    pub async fn take_pending(&self, session_id: SessionId) -> Option<Vec<ChatMessage>> {
        let mut guard = self.inner.lock().await;
        guard.get_mut(&session_id).and_then(|s| s.pending_history.take())
    }

    /// Snapshot the most recent session id (for the swap routine to know
    /// which session is "current"). MVP: returns the first key.
    pub async fn first_session_id(&self) -> Option<SessionId> {
        let guard = self.inner.lock().await;
        guard.keys().next().copied()
    }
}
