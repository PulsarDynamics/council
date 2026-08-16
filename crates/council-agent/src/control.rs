//! Handle control events. The agent subscribes to `council:control` in
//! addition to its normal channels; this module processes whatever comes
//! in on that channel.
//!
//! Today:
//! - `SwapProvider { agent, provider, model?, reason? }` — hand the
//!   session off to a new LLM, summarized.
//! - `ResetSession { agent }` — clear accumulated session state.
//! - `ProvidersChanged` — the providers file changed; reload it (the
//!   `notify`-based watcher in `lib.rs` also picks up file changes
//!   independently; this event is the fast-path).

use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use council_core::{
    ControlEnvelope, ControlEvent, Event, EventEnvelope, EventKind, ProviderConfig, ProviderKind,
    SessionId, ToolContext,
};
use serde_json::json;
use tokio::time::interval;
use tracing::{info, warn};

use crate::llm::agent_loop::MAX_ITERATIONS;
use crate::llm::{
    providers::{AnthropicProvider, OpenAiChatProvider},
    ChatMessage, ChatRole, CompletionRequest, LlmError, LlmProvider, StopReason,
};
use crate::session::SessionMap;
use crate::tools::Publisher;

/// In-memory cache of the providers file. A tokio task polls mtime every
/// second and reloads on change. `lookup_provider` reads from this so a
/// swap after a file edit picks up the new custom without restarting
/// the agent.
///
/// Uses `std::sync::RwLock` (not tokio's) because the polling task and
/// lookup both need a sync read/write. The lock is held for microseconds.
#[derive(Default)]
pub struct ProvidersState {
    inner: RwLock<Vec<ProviderConfig>>,
}

impl ProvidersState {
    pub fn new(initial: Vec<ProviderConfig>) -> Self {
        Self {
            inner: RwLock::new(initial),
        }
    }
    pub fn snapshot(&self) -> Vec<ProviderConfig> {
        self.inner.read().unwrap().clone()
    }
    pub fn replace(&self, next: Vec<ProviderConfig>) {
        *self.inner.write().unwrap() = next;
    }
}

/// Spawn a tokio task that polls the providers file's mtime every second
/// and reloads on change. Returns a `JoinHandle` (drop to stop).
///
/// We use this rather than `notify` because the FSEvents backend
/// silently fails to deliver events to background processes in some
/// macOS environments (sandboxing, agent-style processes, etc.). Polling
/// is reliable; one stat per second per agent is negligible.
pub fn spawn_providers_watcher(
    path: std::path::PathBuf,
    state: Arc<ProvidersState>,
    on_change: Arc<dyn Fn(Vec<ProviderConfig>) + Send + Sync + 'static>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_mtime: Option<std::time::SystemTime> = None;
        let mut ticker = interval(Duration::from_secs(1));
        // First tick fires immediately; consume it.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let meta = match tokio::fs::metadata(&path).await {
                Ok(m) => m,
                Err(_) => continue, // file may not exist yet
            };
            let mtime = match meta.modified() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if last_mtime == Some(mtime) {
                continue;
            }
            last_mtime = Some(mtime);
            // Reload.
            let file = council_core::ProvidersFile::load(&path);
            let flattened = file.flatten();
            state.replace(flattened.clone());
            info!(
                count = flattened.len(),
                path = %path.display(),
                "providers reloaded (mtime)"
            );
            on_change(flattened);
        }
    })
}

// Re-export the debounce helper so callers can adjust if needed.
#[allow(dead_code)]
const DEBOUNCE_MS: u64 = 100;

// ---------------- swap routine ----------------

/// Read the most recent N events of the given session from a fresh Redis
/// subscription. Placeholder for a future "replay" feature.
#[allow(dead_code)]
async fn _replay_from_bus(_session: SessionId) -> Result<Vec<EventEnvelope>> {
    Ok(Vec::new())
}

pub struct SwapDeps {
    pub sessions: Arc<SessionMap>,
    pub provider: Arc<dyn LlmProvider>,
    pub system_prompt: String,
    pub temperature: f32,
    pub model: String,
    pub publisher: Arc<dyn Publisher>,
    pub session_id: SessionId,
    pub agent_name: String,
}

/// Execute a provider swap for the given session. Returns the new pending
/// history (to be loaded into the session state).
pub async fn perform_swap(
    deps: &SwapDeps,
    new_provider: Arc<dyn LlmProvider>,
    new_model: String,
    new_provider_name: String,
    reason: Option<String>,
) -> Result<Vec<ChatMessage>, LlmError> {
    let dump = deps
        .sessions
        .with_mut(deps.session_id, |s| {
            let dump = s.context_dump();
            let files = s.files_touched.clone();
            (dump, files)
        })
        .await;

    let (dump, files) = dump;
    if dump.trim().is_empty() {
        warn!(agent = %deps.agent_name, "swap requested on empty session; nothing to summarize");
    }

    const PER_FILE_CAP: usize = 4_000;
    const TOTAL_CAP: usize = 32_000;
    let mut file_block = String::new();
    let mut total = 0usize;
    for path in &files {
        if total >= TOTAL_CAP {
            file_block.push_str("\n…(remaining files truncated)\n");
            break;
        }
        let body = match tokio::fs::read_to_string(path).await {
            Ok(s) => s,
            Err(e) => {
                file_block.push_str(&format!("\n--- {path} (unreadable: {e}) ---\n"));
                continue;
            }
        };
        let truncated = if body.len() > PER_FILE_CAP {
            let mut t = body;
            t.truncate(PER_FILE_CAP);
            t.push_str("\n…(truncated)");
            t
        } else {
            body
        };
        file_block.push_str(&format!("\n--- {path} ---\n{truncated}\n"));
        total += truncated.len();
    }

    let summary_prompt = format!(
        "Summarize the following session so a fresh LLM can pick up where \
         this one left off. Be concrete: what's the goal, what's been \
         decided, what's been done, what's still open. 6-12 sentences max. \
         Use plain prose (no markdown headings). Do not include tool names \
         or implementation details that the next LLM can read from the \
         attached files.\n\nSession events:\n{dump}"
    );
    let summary_req = CompletionRequest {
        model: deps.model.clone(),
        system: "You are producing a session handoff summary. Be terse.".into(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: summary_prompt,
            tool_call_id: None,
            tool_calls: None,
        }],
        temperature: 0.2,
        tools: Vec::new(),
    };
    let summary = deps.provider.complete(summary_req).await?;
    let summary_text = summary.content.unwrap_or_default();
    info!(
        agent = %deps.agent_name,
        new = %new_provider_name,
        summary_tokens = summary.prompt_tokens + summary.completion_tokens,
        "swap: produced handoff summary"
    );

    let mut new_history: Vec<ChatMessage> = Vec::new();
    new_history.push(ChatMessage {
        role: ChatRole::User,
        content: format!(
            "## Session handoff\n\n\
             This is a continuation of an in-flight session. The previous \
             LLM ({}) has handed off. Here's the summary of what happened:\n\n\
             {}\n\n\
             Files in context (read these — they may have changed since \
             the summary was written):\n{}\n\n\
             Continue from where we left off. The next event will arrive \
             shortly.",
            deps.provider.name(),
            summary_text,
            if file_block.is_empty() { "(no files touched yet)".to_string() } else { file_block.clone() }
        ),
        tool_call_id: None,
        tool_calls: None,
    });
    new_history.push(ChatMessage {
        role: ChatRole::Assistant,
        content: format!(
            "Understood. I'm now running on {} ({}) and have the session \
             summary plus the listed files in context. Ready for the next event.",
            new_provider_name, new_model
        ),
        tool_call_id: None,
        tool_calls: None,
    });

    let _ = deps
        .publisher
        .publish(&EventEnvelope::new(
            "broadcast",
            Event::new(
                deps.session_id,
                EventKind::System {
                    message: format!(
                        "[{}] provider swap: {} -> {} (model: {}{})",
                        deps.agent_name,
                        deps.provider.name(),
                        new_provider_name,
                        new_model,
                        reason
                            .as_deref()
                            .map(|r| format!(", reason: {r}"))
                            .unwrap_or_default()
                    ),
                },
            ),
        ))
        .await;

    Ok(new_history)
}

/// Process a `ControlEnvelope` arriving on the control channel.
pub async fn handle_control(
    env: &ControlEnvelope,
    agent_name: &str,
    current_provider: Arc<dyn LlmProvider>,
    current_model: &str,
    system_prompt: &str,
    temperature: f32,
    sessions: Arc<SessionMap>,
    publisher: Arc<dyn Publisher>,
    providers: Arc<ProvidersState>,
) -> Result<Option<SwapOutcome>, LlmError> {
    match &env.event {
        ControlEvent::SwapProvider {
            agent,
            provider,
            model,
            reason,
        } => {
            if agent != agent_name {
                return Ok(None);
            }
            let new_provider = lookup_provider_with(&providers, provider).ok_or_else(|| {
                LlmError::Config(format!(
                    "swap requested unknown provider {provider:?}; add it in the UI or providers.toml first"
                ))
            })?;
            let new_model = model.clone().unwrap_or_else(|| new_provider.default_model().to_string());
            let session_id = pick_recent_session(&sessions).await;
            let deps = SwapDeps {
                sessions,
                provider: current_provider,
                system_prompt: system_prompt.to_string(),
                temperature,
                model: current_model.to_string(),
                publisher,
                session_id,
                agent_name: agent_name.to_string(),
            };
            let new_history = perform_swap(
                &deps,
                new_provider.clone(),
                new_model.clone(),
                provider.clone(),
                reason.clone(),
            )
            .await?;
            Ok(Some(SwapOutcome {
                new_provider,
                new_model,
                new_history,
                session_id,
            }))
        }
        ControlEvent::ResetSession { agent } => {
            if agent != agent_name {
                return Ok(None);
            }
            let session_id = pick_recent_session(&sessions).await;
            sessions.with_mut(session_id, |s| s.reset()).await;
            info!(agent = agent_name, "session reset by control event");
            Ok(None)
        }
        ControlEvent::ProvidersChanged => {
            // The notify watcher also reloads on file mtime change; this
            // is the fast-path after an in-process POST/DELETE. Force
            // a fresh load so we don't depend on the watcher's debounce
            // window.
            let path = council_core::default_providers_path();
            let file = council_core::ProvidersFile::load(&path);
            let flat = file.flatten();
            providers.replace(flat.clone());
            info!(count = flat.len(), "providers reloaded by control event");
            Ok(None)
        }
    }
}

pub struct SwapOutcome {
    pub new_provider: Arc<dyn LlmProvider>,
    pub new_model: String,
    pub new_history: Vec<ChatMessage>,
    pub session_id: SessionId,
}

async fn pick_recent_session(sessions: &SessionMap) -> SessionId {
    sessions
        .first_session_id()
        .await
        .unwrap_or_else(uuid::Uuid::new_v4)
}

/// Look up a provider by name. Reads from the in-memory `ProvidersState`
/// first (so the file-watcher reload is visible immediately), then falls
/// through to built-ins. Synchronous — the state lock is held for
/// microseconds.
pub fn lookup_provider_with(
    state: &ProvidersState,
    name: &str,
) -> Option<Arc<dyn LlmProvider>> {
    use crate::llm::providers::{
        AnthropicProvider, OpenAiChatProvider, OpenAiResponsesProvider,
    };
    // 1. Built-ins.
    if matches!(name, "openai" | "openai-chat") {
        return Some(Arc::new(OpenAiChatProvider::new()));
    }
    if name == "openai-responses" {
        return Some(Arc::new(OpenAiResponsesProvider::new()));
    }
    if name == "anthropic" {
        return Some(Arc::new(AnthropicProvider::new()));
    }
    // 2. Custom from the in-memory state (kept fresh by notify).
    let snapshot = state.snapshot();
    let entry = snapshot.into_iter().find(|c| c.name == name)?;
    let upper = name.to_uppercase();
    if !entry.api_key.is_empty() {
        std::env::set_var(format!("COUNCIL_PROVIDER_{upper}_API_KEY"), &entry.api_key);
    }
    if !entry.base_url.is_empty() {
        std::env::set_var(format!("COUNCIL_PROVIDER_{upper}_BASE_URL"), &entry.base_url);
    }
    let provider: Arc<dyn LlmProvider> = match entry.kind {
        ProviderKind::AnthropicMessages => Arc::new(AnthropicProvider::with_base_url(entry.base_url)),
        _ => Arc::new(OpenAiChatProvider::with_base_url(entry.base_url)),
    };
    Some(provider)
}

#[allow(dead_code)]
fn _silence_max_iterations() {
    let _ = MAX_ITERATIONS;
    let _ = json!({});
}
