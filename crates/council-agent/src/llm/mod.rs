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
}
