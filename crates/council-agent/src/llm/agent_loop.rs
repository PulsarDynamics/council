//! The agent's LLM loop. Given an incoming event, build a chat history,
//! call the provider, and iterate: if the LLM emits content, publish an
//! `AgentMessage`; if it emits tool calls, run them, publish `ToolCall`
//! + `ToolResult`, then call the LLM again with the results. End on
//! `EndTurn` or after a max-iterations safety stop.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use council_core::{
    AgentSpec, Event, EventEnvelope, EventKind, Tool as ToolTrait, ToolContext,
};
use serde_json::json;
use tracing::{info, warn};
use uuid::Uuid;

use super::{
    registry::load_config, ChatMessage, ChatRole, CompletionRequest, LlmError, LlmProvider,
    ProviderRegistry, StopReason, ToolCall, ToolSpec,
};

/// Max LLM iterations per incoming event. Prevents runaway loops if the
/// LLM keeps calling tools.
pub const MAX_ITERATIONS: usize = 8;

/// Trait alias used by the LLM loop. We re-export the canonical
/// `Publisher` from `crate::tools` so there's only one definition.
pub use crate::tools::Publisher as BusPublisher;

pub struct AgentLoop {
    pub spec: AgentSpec,
    pub registry: ProviderRegistry,
    pub tools: Vec<Arc<dyn ToolTrait>>,
}

impl AgentLoop {
    /// Build a loop with a provider looked up from env by the TOML's
    /// `provider` field.
    pub fn from_spec(
        spec: AgentSpec,
        registry: ProviderRegistry,
        tools: Vec<Arc<dyn ToolTrait>>,
    ) -> Result<Self, String> {
        Ok(Self { spec, registry, tools })
    }

    /// Run one inbound event through the LLM loop until EndTurn or
    /// `MAX_ITERATIONS`.
    pub async fn run_once<P: BusPublisher + ?Sized>(
        &self,
        trigger: &Event,
        bus: &P,
    ) -> Result<(), LlmError> {
        let provider = self.registry.get(&self.spec.model.provider).ok_or_else(|| {
            LlmError::Config(format!(
                "no provider registered for {:?}; add one in settings or set COUNCIL_PROVIDER_{}_KIND",
                self.spec.model.provider,
                self.spec.model.provider.to_uppercase()
            ))
        })?;

        let config = load_config(&self.spec.model.provider).ok_or_else(|| {
            LlmError::Config(format!("missing config for provider {}", self.spec.model.provider))
        })?;
        if config.api_key.is_empty() {
            return Err(LlmError::MissingApiKey(self.spec.model.provider.clone()));
        }

        let model = if !self.spec.model.name.is_empty() {
            self.spec.model.name.clone()
        } else {
            config.default_model.clone()
        };

        // Build the initial chat history from the system prompt + trigger.
        let mut messages: Vec<ChatMessage> = Vec::new();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: render_trigger(trigger),
            tool_call_id: None,
            tool_calls: None,
        });

        let tool_specs: Vec<ToolSpec> = self
            .tools
            .iter()
            .map(|t| ToolSpec {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.schema(),
            })
            .collect();

        let tool_ctx = ToolContext {
            session_id: trigger.session_id,
            agent_name: self.spec.name.clone(),
        };

        for iteration in 0..MAX_ITERATIONS {
            let started = Instant::now();
            let req = CompletionRequest {
                model: model.clone(),
                system: self.spec.prompt.system.clone(),
                messages: messages.clone(),
                temperature: self.spec.model.temperature,
                tools: tool_specs.clone(),
            };

            let resp = match provider.complete(req).await {
                Ok(r) => r,
                Err(e) => {
                    let _ = bus
                        .publish(&EventEnvelope::new(
                            "broadcast",
                            Event::new(
                                trigger.session_id,
                                EventKind::Error {
                                    source: self.spec.name.clone(),
                                    message: format!("llm: {e}"),
                                },
                            ),
                        ))
                        .await;
                    return Err(e);
                }
            };
            let elapsed = started.elapsed();

            // Publish the LLM call stats.
            let _ = bus
                .publish(&EventEnvelope::new(
                    "broadcast",
                    Event::new(
                        trigger.session_id,
                        EventKind::LlmCall {
                            agent: self.spec.name.clone(),
                            model: model.clone(),
                            prompt_tokens: resp.prompt_tokens,
                            completion_tokens: resp.completion_tokens,
                            duration_ms: elapsed.as_millis() as u64,
                        },
                    ),
                ))
                .await;

            info!(
                agent = %self.spec.name,
                iteration,
                prompt = resp.prompt_tokens,
                completion = resp.completion_tokens,
                elapsed_ms = elapsed.as_millis() as u64,
                stop = ?resp.stop_reason,
                "llm"
            );

            // If the LLM produced content, publish it as AgentMessage and
            // append to the history.
            if let Some(content) = &resp.content {
                if !content.is_empty() {
                    let _ = bus
                        .publish(&EventEnvelope::new(
                            &self.output_channel(),
                            Event::new(
                                trigger.session_id,
                                EventKind::AgentMessage {
                                    agent: self.spec.name.clone(),
                                    content: content.clone(),
                                },
                            ),
                        ))
                        .await;
                    messages.push(ChatMessage {
                        role: ChatRole::Assistant,
                        content: content.clone(),
                        tool_call_id: None,
                        tool_calls: if resp.tool_calls.is_empty() {
                            None
                        } else {
                            Some(resp.tool_calls.clone())
                        },
                    });
                }
            } else if !resp.tool_calls.is_empty() {
                // No text, but tool calls — record the assistant turn.
                messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: String::new(),
                    tool_call_id: None,
                    tool_calls: Some(resp.tool_calls.clone()),
                });
            }

            match resp.stop_reason {
                StopReason::EndTurn | StopReason::MaxTokens => {
                    return Ok(());
                }
                StopReason::Error => {
                    return Err(LlmError::Provider("llm returned error stop_reason".into()));
                }
                StopReason::ToolUse => {
                    // Run the tool calls, publish events, append to history, continue.
                    let mut tool_results: Vec<ChatMessage> = Vec::new();
                    for call in &resp.tool_calls {
                        let result = self.execute_tool(call, &tool_ctx, bus).await;
                        tool_results.push(result);
                    }
                    messages.extend(tool_results);
                }
            }
        }
        warn!(
            agent = %self.spec.name,
            "llm loop hit MAX_ITERATIONS={MAX_ITERATIONS}; stopping"
        );
        Ok(())
    }

    /// Publish `tool_call` + `tool_result` events and return a `ChatMessage`
    /// to append to the history.
    async fn execute_tool<P: BusPublisher + ?Sized>(
        &self,
        call: &ToolCall,
        ctx: &ToolContext,
        bus: &P,
    ) -> ChatMessage {
        let _ = bus
            .publish(&EventEnvelope::new(
                "broadcast",
                Event::new(
                    ctx.session_id,
                    EventKind::ToolCall {
                        agent: self.spec.name.clone(),
                        tool: call.name.clone(),
                        args: call.arguments.clone(),
                    },
                ),
            ))
            .await;

        let result_msg = match self.find_tool(&call.name) {
            Some(tool) => {
                let started = Instant::now();
                let outcome = tool.execute(call.arguments.clone(), ctx).await;
                let elapsed = started.elapsed();
                match outcome {
                    Ok(value) => {
                        let _ = bus
                            .publish(&EventEnvelope::new(
                                "broadcast",
                                Event::new(
                                    ctx.session_id,
                                    EventKind::ToolResult {
                                        agent: self.spec.name.clone(),
                                        tool: call.name.clone(),
                                        result: value.clone(),
                                        error: None,
                                    },
                                ),
                            ))
                            .await;
                        info!(agent = %self.spec.name, tool = %call.name, ms = elapsed.as_millis() as u64, "tool ok");
                        format_tool_result(&value)
                    }
                    Err(e) => {
                        let _ = bus
                            .publish(&EventEnvelope::new(
                                "broadcast",
                                Event::new(
                                    ctx.session_id,
                                    EventKind::ToolResult {
                                        agent: self.spec.name.clone(),
                                        tool: call.name.clone(),
                                        result: json!(null),
                                        error: Some(e.clone()),
                                    },
                                ),
                            ))
                            .await;
                        format!("error: {e}")
                    }
                }
            }
            None => {
                let _ = bus
                    .publish(&EventEnvelope::new(
                        "broadcast",
                        Event::new(
                            ctx.session_id,
                            EventKind::Error {
                                source: self.spec.name.clone(),
                                message: format!("unknown tool: {}", call.name),
                            },
                        ),
                    ))
                    .await;
                format!("error: unknown tool `{}`", call.name)
            }
        };

        ChatMessage {
            role: ChatRole::Tool,
            content: result_msg,
            tool_call_id: Some(call.id.clone()),
            tool_calls: None,
        }
    }

    fn find_tool(&self, name: &str) -> Option<Arc<dyn ToolTrait>> {
        self.tools.iter().find(|t| t.name() == name).cloned()
    }

    /// Pick the channel to publish AgentMessage on. Prefer the first
    /// non-`broadcast` entry in the TOML's `publishes` list; fall back to
    /// `broadcast`.
    fn output_channel(&self) -> String {
        self.spec
            .publishes
            .iter()
            .find(|c| c.as_str() != "broadcast")
            .cloned()
            .unwrap_or_else(|| "broadcast".to_string())
    }
}

/// Build the human-readable "user" message we hand to the LLM at the
/// start of a turn. For now: just the trigger event, kind-specific.
fn render_trigger(e: &Event) -> String {
    match &e.kind {
        EventKind::UserMessage { content } => content.clone(),
        EventKind::SessionCreated { goal } => format!("(Session started. Goal: {goal})"),
        EventKind::System { message } => format!("(System: {message})"),
        other => format!("(Incoming event: {:?})", std::mem::discriminant(other)),
    }
}

/// Format a tool's JSON result for the LLM's next-turn input. Truncate
/// deeply to keep the context window sane.
fn format_tool_result(v: &serde_json::Value) -> String {
    let s = v.to_string();
    const MAX: usize = 8_000;
    if s.len() > MAX {
        let mut out = s;
        out.truncate(MAX);
        out.push_str("\n…[truncated]");
        out
    } else {
        s
    }
}

/// Build the chat history from a list of past events, in chronological
/// order. Exposed so future cycles can feed prior context to the LLM.
#[allow(dead_code)]
pub fn history_from_events(events: &[EventEnvelope]) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    for env in events {
        let e = &env.event;
        let (role, content) = match &e.kind {
            EventKind::UserMessage { content } => (ChatRole::User, content.clone()),
            EventKind::AgentMessage { agent: _, content } => (ChatRole::Assistant, content.clone()),
            EventKind::System { message } => (ChatRole::User, format!("(System: {message})")),
            _ => continue,
        };
        out.push(ChatMessage {
            role,
            content,
            tool_call_id: None,
            tool_calls: None,
        });
    }
    out
}

#[allow(dead_code)]
pub fn new_session_marker() -> Uuid {
    Uuid::new_v4()
}

#[allow(dead_code)]
pub fn now_iso() -> chrono::DateTime<Utc> {
    Utc::now()
}
