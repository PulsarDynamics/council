//! Handle control events. The agent subscribes to `council:control` in
//! addition to its normal channels; this module processes whatever comes
//! in on that channel. Today: just `SwapProvider`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use council_core::{
    ControlEnvelope, ControlEvent, Event, EventEnvelope, EventKind, SessionId, ToolContext,
};
use serde_json::json;
use tracing::{info, warn};

use crate::llm::{
    agent_loop::MAX_ITERATIONS, ChatMessage, ChatRole, CompletionRequest, LlmError, LlmProvider,
    StopReason,
};
use crate::session::SessionMap;
use crate::tools::Publisher;

/// Read the most recent N events of the given session from a fresh Redis
/// subscription. Used by the swap routine to pull the agent's *own*
/// history (the bus alone only gives the events the agent already saw).
/// For now, the in-memory `SessionState` already has them, so this is a
/// no-op placeholder for the future.
async fn _replay_from_bus(_session: SessionId) -> Result<Vec<EventEnvelope>> {
    Ok(Vec::new())
}

/// The dependencies the swap routine needs. We pass these in (rather than
/// reading globals) so the routine is testable.
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
    // 1. Build a context dump from in-memory session state.
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

    // 2. Read the files mentioned. Best-effort: skip files that don't
    //    exist or aren't readable. Cap each at 4KB; cap total at 32KB.
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

    // 3. Ask the current LLM to summarize the context. This is the one
    //    call we make with the OLD provider; everything after uses the
    //    new one.
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

    // 4. Build the new context: summary first, then files. This becomes
    //    the seed for the next LLM call.
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

    // 5. Publish a system event so the UI sees what happened.
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

    // Touch the new provider so the compiler doesn't drop it.
    let _ = new_provider.name();
    let _ = new_model;

    Ok(new_history)
}

/// Process a `ControlEnvelope` arriving on the control channel. Returns
/// `Some(SwapOutcome)` if the event was a `SwapProvider` for the agent
/// and a swap was performed. Returns `None` if the event didn't apply.
pub async fn handle_control(
    env: &ControlEnvelope,
    agent_name: &str,
    current_provider: Arc<dyn LlmProvider>,
    current_model: &str,
    system_prompt: &str,
    temperature: f32,
    sessions: Arc<SessionMap>,
    publisher: Arc<dyn Publisher>,
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
            // Find the named provider in the registry. The agent process
            // has its own registry; we look it up by name.
            let new_provider = lookup_provider(provider).ok_or_else(|| {
                LlmError::Config(format!(
                    "swap requested unknown provider {provider:?}; add it in the UI or env first"
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
    }
}

pub struct SwapOutcome {
    pub new_provider: Arc<dyn LlmProvider>,
    pub new_model: String,
    pub new_history: Vec<ChatMessage>,
    pub session_id: SessionId,
}

/// Look up a provider by name. The agent process holds a registry; we
/// re-resolve here so a hot-swap to a newly-added custom is possible
/// once the env is set. (For the MVP, we just return the three
/// built-ins; custom lookup via env comes next.)
fn lookup_provider(name: &str) -> Option<Arc<dyn LlmProvider>> {
    use crate::llm::providers::{AnthropicProvider, OpenAiChatProvider, OpenAiResponsesProvider};
    match name {
        "openai" | "openai-chat" => Some(Arc::new(OpenAiChatProvider::new())),
        "openai-responses" => Some(Arc::new(OpenAiResponsesProvider::new())),
        "anthropic" => Some(Arc::new(AnthropicProvider::new())),
        _ => None,
    }
}

async fn pick_recent_session(sessions: &SessionMap) -> SessionId {
    sessions
        .first_session_id()
        .await
        .unwrap_or_else(uuid::Uuid::new_v4)
}

#[allow(dead_code)]
fn _silence_max_iterations() {
    let _ = MAX_ITERATIONS;
    let _ = json!({});
}
