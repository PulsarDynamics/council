//! Mock-server tests for the streaming LLM providers.
//!
//! We can't hit real LLM APIs in CI, so these tests stand up a tiny
//! `wiremock` server, point an `OpenAiChatProvider` at it, drive
//! `stream()`, and assert that:
//!   - the Text chunks arrive in order and concatenate to the expected body
//!   - the final Done carries the right tool_calls + token usage
//!   - HTTP errors are surfaced as `LlmError::Provider`
//!
//! The mock server is the "real" SSE wire format from OpenAI's docs, so
//! the test catches any drift in the parser.

#![cfg(test)]

use super::providers::OpenAiChatProvider;
use super::{
    ChatMessage, ChatRole, CompletionRequest, LlmError, LlmProvider, StopReason, StreamChunk,
    ToolSpec,
};
use futures::StreamExt;
use serde_json::json;
use std::collections::HashMap;
use std::env;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_request() -> CompletionRequest {
    CompletionRequest {
        model: "gpt-4o".into(),
        system: "You are a test.".into(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "Say hello world".into(),
            tool_call_id: None,
            tool_calls: None,
        }],
        temperature: 0.0,
        tools: vec![],
    }
}

/// Set OPENAI_API_KEY for the duration of one test body. We restore
/// the previous value on drop so tests don't pollute each other or the
/// wider process env.
struct ApiKeyGuard(Option<String>);
impl ApiKeyGuard {
    fn new(v: &str) -> Self {
        let prev = env::var("OPENAI_API_KEY").ok();
        env::set_var("OPENAI_API_KEY", v);
        Self(prev)
    }
}
impl Drop for ApiKeyGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(v) => env::set_var("OPENAI_API_KEY", v),
            None => env::remove_var("OPENAI_API_KEY"),
        }
    }
}

/// Minimal valid OpenAI Chat streaming response, two text deltas + DONE.
fn sample_text_stream() -> String {
    let chunks = vec![
        json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "content": "Hello" },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "delta": { "content": ", world" },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        }),
        json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [],
            "usage": { "prompt_tokens": 7, "completion_tokens": 4 }
        }),
    ];
    let mut s = String::new();
    for c in chunks {
        s.push_str(&format!("data: {}\n\n", c));
    }
    s.push_str("data: [DONE]\n\n");
    s
}

/// Sample stream with a tool call arriving alongside text.
fn sample_tool_call_stream() -> String {
    let chunks = vec![
        json!({
            "id": "chatcmpl-2",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "content": "Let me check" },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-2",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc",
                        "type": "function",
                        "function": { "name": "read_file", "arguments": "{\"path\":" }
                    }]
                },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-2",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": "\"/tmp/x.txt\"}" }
                    }]
                },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-2",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        }),
    ];
    let mut s = String::new();
    for c in chunks {
        s.push_str(&format!("data: {}\n\n", c));
    }
    s.push_str("data: [DONE]\n\n");
    s
}

async fn collect_chunks(s: impl futures::Stream<Item = Result<StreamChunk, LlmError>>) -> Vec<StreamChunk> {
    let mut out = Vec::new();
    let mut s = std::pin::pin!(s);
    while let Some(c) = s.next().await {
        out.push(c.unwrap());
    }
    out
}

#[tokio::test]
async fn openai_chat_stream_emits_text_deltas_in_order() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer sk-test"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sample_text_stream()),
        )
        .mount(&server)
        .await;

    let provider = OpenAiChatProvider::with_base_url(server.uri());
    let _guard = ApiKeyGuard::new("sk-test");
    let chunks = collect_chunks(provider.stream(make_request())).await;
    let texts: Vec<String> = chunks
        .iter()
        .filter_map(|c| match c {
            StreamChunk::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["Hello", ", world"]);
    let done = chunks
        .iter()
        .find_map(|c| match c {
            StreamChunk::Done { .. } => Some(c),
            _ => None,
        })
        .expect("expected a Done chunk");
    match done {
        StreamChunk::Done {
            content,
            prompt_tokens,
            completion_tokens,
            stop_reason,
            ..
        } => {
            assert_eq!(content.as_deref(), Some("Hello, world"));
            assert_eq!(*prompt_tokens, 7);
            assert_eq!(*completion_tokens, 4);
            assert_eq!(*stop_reason, StopReason::EndTurn);
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn openai_chat_stream_stitches_tool_call_arguments() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sample_tool_call_stream()),
        )
        .mount(&server)
        .await;

    let provider = OpenAiChatProvider::with_base_url(server.uri());
    let _guard = ApiKeyGuard::new("sk-test");
    let chunks = collect_chunks(provider.stream(make_request())).await;
    let done = chunks
        .iter()
        .find_map(|c| match c {
            StreamChunk::Done { .. } => Some(c),
            _ => None,
        })
        .expect("expected a Done chunk");
    match done {
        StreamChunk::Done {
            content,
            tool_calls,
            stop_reason,
            ..
        } => {
            assert_eq!(content.as_deref(), Some("Let me check"));
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].name, "read_file");
            assert_eq!(tool_calls[0].id, "call_abc");
            assert_eq!(tool_calls[0].arguments, json!({ "path": "/tmp/x.txt" }));
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn openai_chat_stream_surfaces_http_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let provider = OpenAiChatProvider::with_base_url(server.uri());
    let _guard = ApiKeyGuard::new("sk-test");
    let mut s = std::pin::pin!(provider.stream(make_request()));
    let mut got_err = false;
    while let Some(c) = s.next().await {
        if let Err(LlmError::Provider(msg)) = c {
            assert!(msg.contains("401"), "expected 401 in error, got: {msg}");
            got_err = true;
        }
    }
    assert!(got_err, "expected a Provider error containing 401");
}

#[test]
fn tool_spec_is_serializable() {
    // Smoke test: the ToolSpec used in real requests still serializes to
    // the shape OpenAI expects (we only construct it for the request
    // body; this guards against accidental field renames).
    let spec = ToolSpec {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let mut map: HashMap<&str, &str> = HashMap::new();
    map.insert("name", &spec.name);
    let v = serde_json::to_value(&spec).unwrap();
    assert_eq!(v["name"], "read_file");
    assert_eq!(v["description"], "Read a file");
}
