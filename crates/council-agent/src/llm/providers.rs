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
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use super::{
    sse::SseStream, ChatMessage, ChatRole, CompletionRequest, CompletionResponse,
    CompletionStream, LlmError, LlmProvider, ProviderKind, StopReason, StreamChunk, ToolCall,
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
        Self::with_base_url("https://api.openai.com/v1")
    }

    /// Construct with a custom base URL (used for OpenAI-compatible
    /// providers like Together, Groq, OpenRouter, local llama.cpp).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            name: "openai".into(),
            default_model: "gpt-4o".into(),
            default_base_url: base_url.into(),
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

    fn stream(&self, req: CompletionRequest) -> CompletionStream {
        // Clone the http client (it's `Arc`-backed) so the stream doesn't
        // borrow from `&self`. url/api_key/body are owned; the request
        // future therefore has no `&self` lifetime, which is what lets
        // us return a `'static` boxed stream.
        let http = self.http.clone();
        let url = format!("{}/chat/completions", self.default_base_url);
        let api_key = match std::env::var("OPENAI_API_KEY") {
            Ok(k) => k,
            Err(_) => {
                return Box::pin(futures::stream::once(async move {
                    Err(LlmError::MissingApiKey("openai".into()))
                }));
            }
        };

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
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        Box::pin(async_stream::stream! {
            let resp = match http.post(&url).bearer_auth(&api_key).json(&body).send().await {
                Ok(r) => r,
                Err(e) => { yield Err(LlmError::Http(e)); return; }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                let bytes = match resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => { yield Err(LlmError::Http(e)); return; }
                };
                yield Err(LlmError::Provider(format!(
                    "openai chat (stream): {} -> {}",
                    status,
                    String::from_utf8_lossy(&bytes)
                )));
                return;
            }

            // Accumulator state. Tool calls stream as partial fragments
            // keyed by their `index` field; we stitch them together as
            // we go.
            let mut content_acc = String::new();
            let mut tool_calls_acc: HashMap<u32, PartialToolCall> = HashMap::new();
            let mut prompt_tokens: u32 = 0;
            let mut completion_tokens: u32 = 0;
            let mut stop_reason = StopReason::EndTurn;

            let byte_stream = resp.bytes_stream();
            let mut sse = SseStream::new(byte_stream);
            while let Some(event_result) = sse.next().await {
                let event = match event_result {
                    Ok(e) => e,
                    Err(e) => { yield Err(LlmError::Http(e)); return; }
                };
                if event.data == "[DONE]" {
                    break;
                }
                let parsed: Value = match serde_json::from_str(&event.data) {
                    Ok(v) => v,
                    Err(_) => continue, // tolerate unknown event shapes
                };

                // The `usage` field appears in a final chunk after [DONE]
                // when stream_options.include_usage is true.
                if let Some(usage) = parsed.get("usage") {
                    if let Some(pt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                        prompt_tokens = pt as u32;
                    }
                    if let Some(ct) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                        completion_tokens = ct as u32;
                    }
                }

                if let Some(choices) = parsed.get("choices").and_then(|v| v.as_array()) {
                    for choice in choices {
                        if let Some(delta) = choice.get("delta") {
                            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                if !content.is_empty() {
                                    content_acc.push_str(content);
                                    yield Ok(StreamChunk::Text(content.to_string()));
                                }
                            }
                            if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                for tc in tcs {
                                    let index = tc
                                        .get("index")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0) as u32;
                                    let entry = tool_calls_acc
                                        .entry(index)
                                        .or_insert_with(|| PartialToolCall::default());
                                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                        entry.id.push_str(id);
                                    }
                                    if let Some(function) = tc.get("function") {
                                        if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                                            entry.name.push_str(name);
                                        }
                                        if let Some(args) = function.get("arguments").and_then(|v| v.as_str()) {
                                            entry.arguments.push_str(args);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                            stop_reason = match fr {
                                "tool_calls" => StopReason::ToolUse,
                                "length" => StopReason::MaxTokens,
                                _ => StopReason::EndTurn,
                            };
                        }
                    }
                }
            }

            // Stitch the accumulated tool calls.
            let tool_calls: Vec<ToolCall> = tool_calls_acc
                .into_iter()
                .filter(|(_, v)| !v.name.is_empty())
                .map(|(_, v)| {
                    let arguments: Value = serde_json::from_str(&v.arguments).unwrap_or(Value::Null);
                    ToolCall {
                        id: v.id,
                        name: v.name,
                        arguments,
                    }
                })
                .collect();
            let final_content = if content_acc.is_empty() { None } else { Some(content_acc) };

            yield Ok(StreamChunk::Done {
                content: final_content,
                tool_calls,
                prompt_tokens,
                completion_tokens,
                stop_reason,
            });
        })
    }
}

#[derive(Default, Debug)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
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
        Self::with_base_url("https://api.openai.com/v1")
    }

    /// Construct with a custom base URL.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            name: "openai-responses".into(),
            default_model: "gpt-4o".into(),
            default_base_url: base_url.into(),
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

    fn stream(&self, req: CompletionRequest) -> CompletionStream {
        let http = self.http.clone();
        let url = format!("{}/responses", self.default_base_url);
        let api_key = match std::env::var("OPENAI_API_KEY") {
            Ok(k) => k,
            Err(_) => {
                return Box::pin(futures::stream::once(async move {
                    Err(LlmError::MissingApiKey("openai-responses".into()))
                }));
            }
        };

        let mut input: Vec<Value> = Vec::new();
        for m in &req.messages {
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
            "stream": true,
        });

        Box::pin(async_stream::stream! {
            let resp = match http.post(&url).bearer_auth(&api_key).json(&body).send().await {
                Ok(r) => r,
                Err(e) => { yield Err(LlmError::Http(e)); return; }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                let bytes = match resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => { yield Err(LlmError::Http(e)); return; }
                };
                yield Err(LlmError::Provider(format!(
                    "openai responses (stream): {} -> {}",
                    status,
                    String::from_utf8_lossy(&bytes)
                )));
                return;
            }

            // State. Function-call tool calls stream as a sequence of
            // `output_item.added` (call_id + name), then several
            // `function_call_arguments.delta` events, then
            // `function_call_arguments.done` (with the final JSON).
            // We key by call_id since the Responses API gives each
            // function_call its own id.
            let mut content_acc = String::new();
            let mut tool_calls_acc: HashMap<String, PartialResponsesToolCall> = HashMap::new();
            let mut prompt_tokens: u32 = 0;
            let mut completion_tokens: u32 = 0;
            let mut stop_reason = StopReason::EndTurn;

            let byte_stream = resp.bytes_stream();
            let mut sse = SseStream::new(byte_stream);
            while let Some(event_result) = sse.next().await {
                let event = match event_result {
                    Ok(e) => e,
                    Err(e) => { yield Err(LlmError::Http(e)); return; }
                };
                let parsed: Value = match serde_json::from_str(&event.data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let event_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match event_type {
                    "response.output_text.delta" => {
                        if let Some(delta) = parsed.get("delta").and_then(|v| v.as_str()) {
                            if !delta.is_empty() {
                                content_acc.push_str(delta);
                                yield Ok(StreamChunk::Text(delta.to_string()));
                            }
                        }
                    }
                    "response.output_item.added" => {
                        if let Some(item) = parsed.get("item") {
                            if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                                let call_id = item
                                    .get("call_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let name = item
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if !call_id.is_empty() {
                                    tool_calls_acc.insert(
                                        call_id,
                                        PartialResponsesToolCall { name, arguments: String::new() },
                                    );
                                }
                            }
                        }
                    }
                    "response.function_call_arguments.delta" => {
                        let call_id = parsed
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let delta = parsed
                            .get("delta")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if let Some(entry) = tool_calls_acc.get_mut(&call_id) {
                            entry.arguments.push_str(delta);
                        }
                    }
                    "response.completed" => {
                        if let Some(response) = parsed.get("response") {
                            if let Some(usage) = response.get("usage") {
                                if let Some(t) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                                    prompt_tokens = t as u32;
                                }
                                if let Some(t) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                                    completion_tokens = t as u32;
                                }
                            }
                            // Infer stop_reason from the final output.
                            if let Some(output) = response.get("output").and_then(|v| v.as_array()) {
                                for item in output {
                                    if item.get("type").and_then(|v| v.as_str())
                                        == Some("function_call")
                                    {
                                        stop_reason = StopReason::ToolUse;
                                    }
                                }
                            }
                        }
                    }
                    _ => {} // ignore other event types
                }
            }

            let tool_calls: Vec<ToolCall> = tool_calls_acc
                .into_iter()
                .filter(|(_, v)| !v.name.is_empty())
                .map(|(id, v)| {
                    let arguments: Value = serde_json::from_str(&v.arguments).unwrap_or(Value::Null);
                    ToolCall { id, name: v.name, arguments }
                })
                .collect();
            let final_content = if content_acc.is_empty() { None } else { Some(content_acc) };

            yield Ok(StreamChunk::Done {
                content: final_content,
                tool_calls,
                prompt_tokens,
                completion_tokens,
                stop_reason,
            });
        })
    }
}

#[derive(Default, Debug)]
struct PartialResponsesToolCall {
    name: String,
    arguments: String,
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
        Self::with_base_url("https://api.anthropic.com/v1")
    }

    /// Construct with a custom base URL (Anthropic-compatible providers).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            name: "anthropic".into(),
            default_model: "claude-sonnet-4-5".into(),
            default_base_url: base_url.into(),
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

    fn stream(&self, req: CompletionRequest) -> CompletionStream {
        let http = self.http.clone();
        let url = format!("{}/messages", self.default_base_url);
        let api_key = match std::env::var("ANTHROPIC_API_KEY") {
            Ok(k) => k,
            Err(_) => {
                return Box::pin(futures::stream::once(async move {
                    Err(LlmError::MissingApiKey("anthropic".into()))
                }));
            }
        };

        let mut messages: Vec<Value> = Vec::new();
        for m in &req.messages {
            match m.role {
                ChatRole::System => {}
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
            "stream": true,
        });

        Box::pin(async_stream::stream! {
            let resp = match http.post(&url).header("x-api-key", &api_key).header("anthropic-version", "2023-06-01").json(&body).send().await {
                Ok(r) => r,
                Err(e) => { yield Err(LlmError::Http(e)); return; }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                let bytes = match resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => { yield Err(LlmError::Http(e)); return; }
                };
                yield Err(LlmError::Provider(format!(
                    "anthropic (stream): {} -> {}",
                    status,
                    String::from_utf8_lossy(&bytes)
                )));
                return;
            }

            // Per-block accumulators. We key by `index` from the SSE
            // events so multiple blocks (text + multiple tool_uses)
            // can be in flight at once.
            let mut content_acc = String::new();
            let mut blocks: HashMap<u32, PartialAnthropicBlock> = HashMap::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut prompt_tokens: u32 = 0;
            let mut completion_tokens: u32 = 0;
            let mut stop_reason = StopReason::EndTurn;
            let mut finished = false;

            let byte_stream = resp.bytes_stream();
            let mut sse = SseStream::new(byte_stream);
            while let Some(event_result) = sse.next().await {
                if finished { break; }
                let event = match event_result {
                    Ok(e) => e,
                    Err(e) => { yield Err(LlmError::Http(e)); return; }
                };
                let parsed: Value = match serde_json::from_str(&event.data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let event_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match event_type {
                    "message_start" => {
                        if let Some(usage) = parsed.get("message").and_then(|m| m.get("usage")) {
                            if let Some(t) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                                prompt_tokens = t as u32;
                            }
                        }
                    }
                    "content_block_start" => {
                        let index = parsed.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        if let Some(block) = parsed.get("content_block") {
                            let kind = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match kind {
                                "text" => {
                                    blocks.insert(index, PartialAnthropicBlock::Text(String::new()));
                                }
                                "tool_use" => {
                                    let id = block
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let name = block
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    blocks.insert(
                                        index,
                                        PartialAnthropicBlock::ToolUse {
                                            id,
                                            name,
                                            arguments: String::new(),
                                        },
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    "content_block_delta" => {
                        let index = parsed.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        if let Some(delta) = parsed.get("delta") {
                            let kind = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match kind {
                                "text_delta" => {
                                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                        content_acc.push_str(text);
                                        yield Ok(StreamChunk::Text(text.to_string()));
                                        if let Some(PartialAnthropicBlock::Text(t)) = blocks.get_mut(&index) {
                                            t.push_str(text);
                                        }
                                    }
                                }
                                "input_json_delta" => {
                                    if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                        if let Some(PartialAnthropicBlock::ToolUse { arguments, .. }) = blocks.get_mut(&index) {
                                            arguments.push_str(partial);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "content_block_stop" => {
                        let index = parsed.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        if let Some(PartialAnthropicBlock::ToolUse { id, name, arguments }) = blocks.remove(&index) {
                            if !name.is_empty() {
                                let arguments: Value = serde_json::from_str(&arguments).unwrap_or(Value::Null);
                                tool_calls.push(ToolCall { id, name, arguments });
                            }
                        }
                    }
                    "message_delta" => {
                        if let Some(delta) = parsed.get("delta") {
                            if let Some(sr) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                                stop_reason = match sr {
                                    "tool_use" => StopReason::ToolUse,
                                    "max_tokens" => StopReason::MaxTokens,
                                    _ => StopReason::EndTurn,
                                };
                            }
                        }
                        if let Some(usage) = parsed.get("usage") {
                            if let Some(t) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                                completion_tokens = t as u32;
                            }
                        }
                    }
                    "message_stop" => {
                        finished = true;
                    }
                    _ => {}
                }
            }

            let final_content = if content_acc.is_empty() { None } else { Some(content_acc) };
            yield Ok(StreamChunk::Done {
                content: final_content,
                tool_calls,
                prompt_tokens,
                completion_tokens,
                stop_reason,
            });
        })
    }
}

#[derive(Debug)]
enum PartialAnthropicBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        arguments: String,
    },
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
