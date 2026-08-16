//! Built-in provider implementations.
//!
//! - `OpenAiChatProvider` — POST {base_url}/chat/completions with the
//!   OpenAI Chat Completions shape. This is the de-facto standard; most
//!   "OpenAI-compatible" providers (Together, Groq, OpenRouter compat,
//!   local llama.cpp) speak this.
//! - `OpenAiResponsesProvider` — POST {base_url}/responses. Newer API.
//! - `AnthropicProvider` — POST {base_url}/messages. Distinctive
//!   content-block + tool-use shape; the most divergent of the three.
//!
//! All three return the normalized `CompletionResponse` so the agent's
//! LLM loop is provider-agnostic.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    ChatMessage, ChatRole, CompletionRequest, CompletionResponse, LlmError, LlmProvider,
    ProviderKind, StopReason, ToolCall,
};

const USER_AGENT: &str = concat!("council-agent/", env!("CARGO_PKG_VERSION"));

// =====================================================================
// OpenAI Chat Completions
// =====================================================================

pub struct OpenAiChatProvider {
    name: String,
    default_model: String,
    default_base_url: String,
    http: Client,
}

impl OpenAiChatProvider {
    pub fn new() -> Self {
        Self {
            name: "openai".into(),
            default_model: "gpt-4o".into(),
            default_base_url: "https://api.openai.com/v1".into(),
            http: Client::builder()
                .user_agent(USER_AGENT)
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client"),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiChatProvider {
    fn name(&self) -> &str {
        &self.name
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAiChat
    }
    fn default_model(&self) -> &str {
        &self.default_model
    }
    fn default_base_url(&self) -> &str {
        &self.default_base_url
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let url = format!("{}/chat/completions", self.default_base_url);
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| LlmError::MissingApiKey("openai".into()))?;

        // Build messages: system -> first "system" message, then rest.
        let mut messages: Vec<Value> = Vec::with_capacity(req.messages.len() + 1);
        if !req.system.is_empty() {
            messages.push(json!({ "role": "system", "content": req.system }));
        }
        for m in &req.messages {
            messages.push(chat_message_to_openai(m));
        }
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();

        let body = json!({
            "model": req.model,
            "messages": messages,
            "temperature": req.temperature,
            "tools": if tools.is_empty() { Value::Null } else { json!(tools) },
        });

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&api_key)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            return Err(LlmError::Provider(format!(
                "openai chat: {} -> {}",
                status,
                String::from_utf8_lossy(&bytes)
            )));
        }
        let parsed: OpenAiChatResponse = serde_json::from_slice(&bytes)?;
        Ok(openai_chat_to_response(parsed))
    }
}

fn chat_message_to_openai(m: &ChatMessage) -> Value {
    match m.role {
        ChatRole::System => json!({ "role": "system", "content": m.content }),
        ChatRole::User => json!({ "role": "user", "content": m.content }),
        ChatRole::Assistant => {
            let mut o = json!({ "role": "assistant" });
            if !m.content.is_empty() {
                o["content"] = json!(m.content);
            }
            if let Some(calls) = &m.tool_calls {
                o["tool_calls"] = json!(calls.iter().map(|c| json!({
                    "id": c.id,
                    "type": "function",
                    "function": {
                        "name": c.name,
                        "arguments": c.arguments.to_string(),
                    }
                })).collect::<Vec<_>>());
            }
            o
        }
        ChatRole::Tool => json!({
            "role": "tool",
            "tool_call_id": m.tool_call_id,
            "content": m.content,
        }),
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChatChoice>,
    usage: Option<OpenAiChatUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatChoice {
    message: OpenAiChatMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAiChatToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatToolCall {
    id: String,
    function: OpenAiChatFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatFunction {
    name: String,
    /// OpenAI returns arguments as a JSON string.
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

fn openai_chat_to_response(r: OpenAiChatResponse) -> CompletionResponse {
    let choice = r.choices.into_iter().next();
    let mut content = None;
    let mut tool_calls = Vec::new();
    let mut stop_reason = StopReason::EndTurn;
    if let Some(c) = choice {
        content = c.message.content.filter(|s| !s.is_empty());
        if let Some(calls) = c.message.tool_calls {
            for call in calls {
                let args: Value = serde_json::from_str(&call.function.arguments)
                    .unwrap_or(Value::Null);
                tool_calls.push(ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments: args,
                });
            }
        }
        stop_reason = match c.finish_reason.as_deref() {
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            Some("stop") | None => StopReason::EndTurn,
            Some(other) => {
                tracing::warn!(reason = other, "unknown openai finish_reason");
                StopReason::EndTurn
            }
        };
    }
    CompletionResponse {
        content,
        tool_calls,
        prompt_tokens: r.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
        completion_tokens: r.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
        stop_reason,
    }
}

// =====================================================================
// OpenAI Responses
// =====================================================================

pub struct OpenAiResponsesProvider {
    name: String,
    default_model: String,
    default_base_url: String,
    http: Client,
}

impl OpenAiResponsesProvider {
    pub fn new() -> Self {
        Self {
            name: "openai-responses".into(),
            default_model: "gpt-4o".into(),
            default_base_url: "https://api.openai.com/v1".into(),
            http: Client::builder()
                .user_agent(USER_AGENT)
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client"),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiResponsesProvider {
    fn name(&self) -> &str {
        &self.name
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAiResponses
    }
    fn default_model(&self) -> &str {
        &self.default_model
    }
    fn default_base_url(&self) -> &str {
        &self.default_base_url
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let url = format!("{}/responses", self.default_base_url);
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| LlmError::MissingApiKey("openai-responses".into()))?;

        // The Responses API uses an `input` array of message items. System
        // instructions go in `instructions`.
        let mut input: Vec<Value> = Vec::new();
        for m in &req.messages {
            // Skip the "system" role here — it goes in `instructions` instead.
            if matches!(m.role, ChatRole::System) {
                continue;
            }
            let content = json!([{ "type": "input_text", "text": m.content }]);
            input.push(json!({ "role": role_str(m.role), "content": content }));
        }
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();

        let body = json!({
            "model": req.model,
            "instructions": req.system,
            "input": input,
            "temperature": req.temperature,
            "tools": if tools.is_empty() { Value::Null } else { json!(tools) },
        });

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&api_key)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            return Err(LlmError::Provider(format!(
                "openai responses: {} -> {}",
                status,
                String::from_utf8_lossy(&bytes)
            )));
        }
        let parsed: OpenAiResponsesResponse = serde_json::from_slice(&bytes)?;
        Ok(openai_responses_to_response(parsed))
    }
}

fn role_str(r: ChatRole) -> &'static str {
    match r {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesResponse {
    output: Vec<OpenAiResponsesOutput>,
    usage: Option<OpenAiResponsesUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAiResponsesOutput {
    Message {
        content: Vec<OpenAiResponsesContent>,
    },
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesContent {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesUsage {
    input_tokens: u32,
    output_tokens: u32,
}

fn openai_responses_to_response(r: OpenAiResponsesResponse) -> CompletionResponse {
    let mut content = None;
    let mut tool_calls = Vec::new();
    let mut stop_reason = StopReason::EndTurn;
    for item in r.output {
        match item {
            OpenAiResponsesOutput::Message { content: blocks } => {
                for b in blocks {
                    if b.kind == "output_text" {
                        if let Some(t) = b.text {
                            content = Some(match content {
                                Some(c) => format!("{c}\n{t}"),
                                None => t,
                            });
                        }
                    }
                }
            }
            OpenAiResponsesOutput::FunctionCall {
                call_id,
                name,
                arguments,
            } => {
                let args: Value = serde_json::from_str(&arguments).unwrap_or(Value::Null);
                tool_calls.push(ToolCall {
                    id: call_id,
                    name,
                    arguments: args,
                });
                stop_reason = StopReason::ToolUse;
            }
            OpenAiResponsesOutput::Other => {}
        }
    }
    CompletionResponse {
        content,
        tool_calls,
        prompt_tokens: r.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
        completion_tokens: r.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
        stop_reason,
    }
}

// =====================================================================
// Anthropic Messages
// =====================================================================

pub struct AnthropicProvider {
    name: String,
    default_model: String,
    default_base_url: String,
    http: Client,
}

impl AnthropicProvider {
    pub fn new() -> Self {
        Self {
            name: "anthropic".into(),
            default_model: "claude-sonnet-4-5".into(),
            default_base_url: "https://api.anthropic.com/v1".into(),
            http: Client::builder()
                .user_agent(USER_AGENT)
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client"),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.name
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::AnthropicMessages
    }
    fn default_model(&self) -> &str {
        &self.default_model
    }
    fn default_base_url(&self) -> &str {
        &self.default_base_url
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let url = format!("{}/messages", self.default_base_url);
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| LlmError::MissingApiKey("anthropic".into()))?;

        // Anthropic separates system into its own field, and the user/
        // assistant roles use content blocks (text + tool_use + tool_result).
        let mut messages: Vec<Value> = Vec::new();
        for m in &req.messages {
            match m.role {
                ChatRole::System => { /* goes in `system` field */ }
                ChatRole::User => {
                    messages.push(json!({
                        "role": "user",
                        "content": [{ "type": "text", "text": m.content }]
                    }));
                }
                ChatRole::Assistant => {
                    let mut blocks: Vec<Value> = Vec::new();
                    if !m.content.is_empty() {
                        blocks.push(json!({ "type": "text", "text": m.content }));
                    }
                    if let Some(calls) = &m.tool_calls {
                        for c in calls {
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": c.id,
                                "name": c.name,
                                "input": c.arguments,
                            }));
                        }
                    }
                    messages.push(json!({ "role": "assistant", "content": blocks }));
                }
                ChatRole::Tool => {
                    // Each tool result is its own user message with a
                    // tool_result block.
                    let id = m.tool_call_id.clone().unwrap_or_default();
                    messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": m.content,
                        }]
                    }));
                }
            }
        }

        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();

        let body = json!({
            "model": req.model,
            "system": req.system,
            "messages": messages,
            "max_tokens": 4096,
            "temperature": req.temperature,
            "tools": if tools.is_empty() { Value::Null } else { json!(tools) },
        });

        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            return Err(LlmError::Provider(format!(
                "anthropic: {} -> {}",
                status,
                String::from_utf8_lossy(&bytes)
            )));
        }
        let parsed: AnthropicResponse = serde_json::from_slice(&bytes)?;
        Ok(anthropic_to_response(parsed))
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    usage: AnthropicUsage,
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContent {
    Text { text: String },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

fn anthropic_to_response(r: AnthropicResponse) -> CompletionResponse {
    let mut content = None;
    let mut tool_calls = Vec::new();
    for block in r.content {
        match block {
            AnthropicContent::Text { text } => {
                content = Some(match content {
                    Some(c) => format!("{c}\n{text}"),
                    None => text,
                });
            }
            AnthropicContent::ToolUse { id, name, input } => {
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments: input,
                });
            }
            AnthropicContent::Other => {}
        }
    }
    let stop_reason = match r.stop_reason.as_deref() {
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("end_turn") | None => StopReason::EndTurn,
        Some(other) => {
            tracing::warn!(reason = other, "unknown anthropic stop_reason");
            StopReason::EndTurn
        }
    };
    CompletionResponse {
        content,
        tool_calls,
        prompt_tokens: r.usage.input_tokens,
        completion_tokens: r.usage.output_tokens,
        stop_reason,
    }
}
