//! LLM provider abstraction. Every provider implements the same `LlmProvider`
//! trait with a normalized request/response shape. Provider impls translate
//! that shape into the provider's native API (OpenAI Chat Completions,
//! OpenAI Responses, Anthropic Messages, or a custom OpenAI-compatible
//! endpoint).
//!
//! Adding a new provider = implement this trait + register it in the
//! `ProviderRegistry`. The UI's settings menu lets users add custom
//! providers without touching code.

pub mod agent_loop;
pub mod providers;
pub mod registry;
pub mod sse;

#[cfg(test)]
mod agent_loop_stream_test;
#[cfg(test)]
mod providers_stream_test;

pub use registry::ProviderRegistry;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

// Re-export the shared types from council-core so the LLM module
// doesn't have to import council-core::ProviderKind in every file.
pub use council_core::{ProviderConfig, ProviderKind};

/// Normalized chat message. The LLM loops in `loop.rs` build a list of these
/// from the event stream and hand them to the provider; the provider
/// translates to its native role/content shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    /// For role=Tool: which tool call this is a result for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// For role=Assistant: tool calls the LLM wants to make.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool the LLM is allowed to invoke. Mirrors `council_core::tool::Tool`
/// but in a shape that's easy to send to any provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing the tool's parameters.
    pub parameters: Value,
}

/// A request from the LLM to invoke a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Why the LLM stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of turn. No more tool calls, no more text.
    EndTurn,
    /// The LLM wants to call one or more tools. Caller should run them
    /// and call `complete()` again with the results.
    ToolUse,
    /// Hit the max_tokens limit mid-generation.
    MaxTokens,
    /// Provider-side error that we surfaced to the agent. The agent
    /// should publish an `error` event and stop.
    Error,
}

/// Normalized request to the LLM.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub tools: Vec<ToolSpec>,
}

/// Normalized response from the LLM.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub stop_reason: StopReason,
}

/// One chunk yielded by a provider's `stream()` method.
///
/// Providers with native streaming emit zero or more `Text` deltas
/// followed by exactly one `Done` carrying the final assembled
/// response (so the LLM loop doesn't have to re-call `complete()`
/// to learn about tool calls or token usage).
///
/// The default trait impl calls `complete()` and emits a single
/// `Done` with the assembled response, so a provider that doesn't
/// override `stream()` still works.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// A piece of incremental text from the assistant. The loop is
    /// responsible for accumulation; the chunk size is provider-
    /// dependent (a single token, a few tokens, or a whole sentence).
    Text(String),
    /// Stream is finished. `content` is the fully-assembled text
    /// (may be `None` if the response was tool-only); `tool_calls`
    /// and `usage` are the final structured fields.
    Done {
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        prompt_tokens: u32,
        completion_tokens: u32,
        stop_reason: StopReason,
    },
}

pub type CompletionStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Result<StreamChunk, LlmError>> + Send>>;

/// Configuration for one LLM provider. Loaded from env (built-ins) or from
/// the user's settings (custom). The agent holds one `ProviderConfig` per
/// provider it knows about, indexed by name.
// (moved to council-core as `ProviderConfig`; re-exported above.)

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("missing API key for provider {0}")]
    MissingApiKey(String),
}

/// The trait every provider implements.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stable name used in agent TOML `provider` fields and in env vars
    /// (`COUNCIL_PROVIDER_<NAME>_...`).
    fn name(&self) -> &str;

    /// Wire format this provider speaks.
    fn kind(&self) -> ProviderKind;

    /// Default model for this provider. The TOML's `model.name` can override.
    fn default_model(&self) -> &str;

    /// Default base URL. The TOML/ProviderConfig can override.
    fn default_base_url(&self) -> &str;

    /// Execute one completion. Returns the normalized response.
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError>;

    /// Streaming variant. Each provider implements this directly because
    /// the returned stream must be `'static` (it lives independently of
    /// `&self`); implementations clone the internal `reqwest::Client`
    /// (which is `Arc`-backed) and own all request data so no borrow of
    /// `self` outlives the stream construction.
    ///
    /// The contract: emit zero or more `Text` deltas as the upstream API
    /// produces them, then exactly one `Done` with the final assembled
    /// response (content + tool_calls + token usage). The LLM loop uses
    /// the `Done` to decide whether to run tools (matching the old
    /// `complete()` behavior) and to record the `LlmCall` event.
    fn stream(&self, req: CompletionRequest) -> CompletionStream;
}
