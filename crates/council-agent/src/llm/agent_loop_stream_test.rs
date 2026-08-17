//! End-to-end test for the streaming path through `AgentLoop`.
//!
//! Verifies the full wire: a real provider stream() against a wiremock
//! SSE server, driven by `AgentLoop::run_once`, with a recording
//! `Publisher` that captures everything published to the bus. Asserts
//! that:
//!   - one or more `AgentMessageDelta` events are published (text
//!     appears token-by-token from the UI's point of view)
//!   - the `LlmCall` event lands with the right token usage
//!   - the final assembled `AgentMessage` lands with the concatenated
//!     content
//!
//! This is the "smoke test" we couldn't run against a live LLM, run
//! against a mock instead.

#![cfg(test)]

use super::agent_loop::AgentLoop;
use super::providers::OpenAiChatProvider;
use super::ProviderRegistry;
use crate::session::SessionMap;
use async_trait::async_trait;
use council_core::{
    AgentSpec, Event, EventEnvelope, EventKind, ModelConfig, PromptConfig, ToolsConfig,
};
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Process-wide lock used to serialize tests that mutate
/// `OPENAI_API_KEY`. (The env-var-touching tests in
/// `providers_stream_test` use a RAII guard; without this lock, a
/// drop-guard from one test can race a `set_var` in another.)
fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Acquire the env lock and recover from poisoning (a panic in one
/// test while holding the lock shouldn't break siblings).
fn acquire_env_lock() -> std::sync::MutexGuard<'static, ()> {
    env_lock().lock().unwrap_or_else(|e| e.into_inner())
}

/// In-memory bus that records every published envelope so the test can
/// assert on the event sequence.
struct RecordingBus {
    events: Mutex<Vec<EventEnvelope>>,
}

impl RecordingBus {
    fn new() -> Self {
        Self { events: Mutex::new(Vec::new()) }
    }
}

fn event_type_name(k: &EventKind) -> &'static str {
    match k {
        EventKind::UserMessage { .. } => "user_message",
        EventKind::AgentMessage { .. } => "agent_message",
        EventKind::AgentMessageDelta { .. } => "agent_message_delta",
        EventKind::AgentThinking { .. } => "agent_thinking",
        EventKind::ToolCall { .. } => "tool_call",
        EventKind::ToolResult { .. } => "tool_result",
        EventKind::FileChange { .. } => "file_change",
        EventKind::AgentStatus { .. } => "agent_status",
        EventKind::LlmCall { .. } => "llm_call",
        EventKind::System { .. } => "system",
        EventKind::SessionCreated { .. } => "session_created",
        EventKind::SessionCompleted { .. } => "session_completed",
        EventKind::SessionCancelled { .. } => "session_cancelled",
        EventKind::Error { .. } => "error",
    }
}

#[async_trait]
impl crate::tools::Publisher for RecordingBus {
    async fn publish(&self, env: &EventEnvelope) -> anyhow::Result<()> {
        self.events.lock().unwrap().push(env.clone());
        Ok(())
    }
}

fn planner_spec() -> AgentSpec {
    AgentSpec {
        name: "planner".into(),
        subscribes: vec!["goal".into()],
        publishes: vec!["plan".into()],
        prompt: PromptConfig {
            system: "you are a planner".into(),
            template: None,
        },
        model: ModelConfig {
            provider: "openai".into(),
            name: "gpt-4o".into(),
            temperature: 0.0,
        },
        tools: ToolsConfig {
            allowed: BTreeSet::new(),
        },
    }
}

fn sample_two_delta_stream() -> String {
    // The "Hello, mock." message broken into two text deltas.
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
                "delta": { "content": ", mock." },
                "finish_reason": "stop"
            }]
        }),
        json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [],
            "usage": { "prompt_tokens": 9, "completion_tokens": 2 }
        }),
    ];
    let mut s = String::new();
    for c in chunks {
        s.push_str(&format!("data: {}\n\n", c));
    }
    s.push_str("data: [DONE]\n\n");
    s
}

#[tokio::test]
async fn agent_loop_emits_deltas_then_final_message_via_stream() {
    // 1. Stand up the mock LLM.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sample_two_delta_stream()),
        )
        .mount(&server)
        .await;

    // 2. Build a registry that maps "openai" to the mock-URL provider.
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(OpenAiChatProvider::with_base_url(server.uri())));

    // 3. Sanity: the registry can be looked up by the spec's provider name.
    assert!(registry.get("openai").is_some());

    // 4. We also need `load_config("openai")` to return a config with a
    //    non-empty API key, because the agent loop validates that. The
    //    provider's own `stream()` reads `OPENAI_API_KEY` directly.
    // (Env var is set at the top of the test, under env_lock.)

    // 5. Build the loop and run one trigger.
    let spec = planner_spec();
    let loop_ = AgentLoop::from_spec(spec, registry, vec![]).unwrap();
    let bus = RecordingBus::new();
    let trigger = Event::new(
        uuid::Uuid::new_v4(),
        EventKind::UserMessage {
            content: "say hello".into(),
        },
    );
    loop_.run_once(&trigger, &bus, &tokio::sync::Notify::new(), &SessionMap::new()).await.expect("loop ok");

    // 6. Inspect what the loop published.
    let events = bus.events.lock().unwrap().clone();
    let kinds: Vec<String> = events
        .iter()
        .map(|e| {
            let k = &e.event.kind;
            match k {
                EventKind::AgentMessageDelta { agent, .. } => {
                    format!("delta:{agent}")
                }
                EventKind::AgentMessage { agent, .. } => format!("msg:{agent}"),
                EventKind::LlmCall { agent, .. } => format!("llm:{agent}"),
                other => format!("other:{}", event_type_name(other)),
            }
        })
        .collect();

    // We expect at least 2 deltas, 1 llm call, 1 final agent_message.
    let delta_count = kinds.iter().filter(|k| k.starts_with("delta:")).count();
    let msg_count = kinds.iter().filter(|k| k.starts_with("msg:")).count();
    let llm_count = kinds.iter().filter(|k| k.starts_with("llm:")).count();
    assert!(
        delta_count >= 2,
        "expected at least 2 deltas, got {delta_count} (sequence: {kinds:?})"
    );
    assert_eq!(
        msg_count, 1,
        "expected exactly 1 final AgentMessage, got {msg_count} ({kinds:?})"
    );
    assert_eq!(llm_count, 1, "expected 1 LlmCall event ({kinds:?})");

    // 7. Concatenate the deltas and assert they spell out the final message.
    let mut accumulated = String::new();
    for e in &events {
        if let EventKind::AgentMessageDelta { delta, .. } = &e.event.kind {
            accumulated.push_str(delta);
        }
    }
    assert_eq!(accumulated, "Hello, mock.");

    // 8. The final AgentMessage must equal the concatenated deltas.
    let final_msg = events
        .iter()
        .find_map(|e| match &e.event.kind {
            EventKind::AgentMessage { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("a final AgentMessage");
    assert_eq!(final_msg, "Hello, mock.");

    // 9. The LlmCall event should carry the usage the mock returned.
    let llm = events
        .iter()
        .find_map(|e| match &e.event.kind {
            EventKind::LlmCall {
                prompt_tokens,
                completion_tokens,
                ..
            } => Some((*prompt_tokens, *completion_tokens)),
            _ => None,
        })
        .expect("an LlmCall event");
    assert_eq!(llm, (9, 2));
}

#[tokio::test]
async fn agent_loop_does_not_emit_message_event_when_response_is_tool_only() {
    // A tool-only response (no text) should emit no AgentMessage and
    // no deltas — just an LlmCall and the tool-call events. With an
    // empty tools list the loop will just record the assistant turn
    // (no tool execution) and EndTurn.
    let _env = acquire_env_lock();
    std::env::set_var("OPENAI_API_KEY", "sk-test");
    use super::ToolCall;
    let server = MockServer::start().await;
    let tool_call_body = {
        let chunks = vec![json!({
            "id": "chatcmpl-tool",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "no_such_tool", "arguments": "{}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })];
        let mut s = String::new();
        for c in chunks {
            s.push_str(&format!("data: {}\n\n", c));
        }
        s.push_str("data: [DONE]\n\n");
        s
    };
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(tool_call_body),
        )
        .mount(&server)
        .await;

    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(OpenAiChatProvider::with_base_url(server.uri())));
    // (OPENAI_API_KEY is set at the top of the test, under env_lock.)

    let spec = planner_spec();
    let loop_ = AgentLoop::from_spec(spec, registry, vec![]).unwrap();
    let bus = RecordingBus::new();
    let trigger = Event::new(
        uuid::Uuid::new_v4(),
        EventKind::UserMessage { content: "call a tool".into() },
    );
    // We expect this to return Ok even with empty tools — the agent
    // records the turn and ends. (In a real run with tools available,
    // it would call the tool and then either continue or EndTurn.)
    let result = loop_.run_once(&trigger, &bus, &tokio::sync::Notify::new(), &SessionMap::new()).await;
    if let Err(ref e) = result {
        // Print the captured error events for debugging the failure
        // path — the second iteration's mock SSE body might not
        // serialize cleanly when wiremock re-uses the same body.
        let events = bus.events.lock().unwrap().clone();
        eprintln!("loop returned err: {e:?}");
        eprintln!("events captured:");
        for ev in &events {
            eprintln!("  - {} on {}", event_type_name(&ev.event.kind), ev.channel);
            if let EventKind::Error { message, .. } = &ev.event.kind {
                eprintln!("      error message: {message}");
            }
        }
    }
    assert!(result.is_ok(), "loop should handle tool-only response: {result:?}");

    // 0 AgentMessage deltas, 0 AgentMessage, at least 1 LlmCall.
    // (The loop iterates to MAX_ITERATIONS because the unknown tool
    // keeps being requested and there's no way to satisfy it; we don't
    // pin the exact count, just verify the no-text invariants.)
    let events = bus.events.lock().unwrap().clone();
    let delta_count = events
        .iter()
        .filter(|e| matches!(e.event.kind, EventKind::AgentMessageDelta { .. }))
        .count();
    let msg_count = events
        .iter()
        .filter(|e| matches!(e.event.kind, EventKind::AgentMessage { .. }))
        .count();
    let llm_count = events
        .iter()
        .filter(|e| matches!(e.event.kind, EventKind::LlmCall { .. }))
        .count();
    assert_eq!(delta_count, 0, "no text → no deltas");
    assert_eq!(msg_count, 0, "no text → no AgentMessage");
    assert!(llm_count >= 1, "expected at least one LlmCall (got {llm_count})");
}

#[tokio::test]
async fn agent_loop_aborts_mid_stream_when_cancel_is_signalled() {
    // The wiremock response is delayed via `set_delay`, so the agent
    // spends time waiting on the response body. We cancel during
    // that wait and verify the loop exits with a SessionCancelled
    // event (and no final AgentMessage).
    let _env = acquire_env_lock();
    std::env::set_var("OPENAI_API_KEY", "sk-test");

    let server = MockServer::start().await;
    let body = sample_two_delta_stream();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_delay(std::time::Duration::from_millis(500))
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(OpenAiChatProvider::with_base_url(server.uri())));
    let spec = planner_spec();
    let loop_ = AgentLoop::from_spec(spec, registry, vec![]).unwrap();
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<EventEnvelope>::new()));
    struct SharedBus {
        events: std::sync::Arc<std::sync::Mutex<Vec<EventEnvelope>>>,
    }
    #[async_trait]
    impl crate::tools::Publisher for SharedBus {
        async fn publish(&self, env: &EventEnvelope) -> anyhow::Result<()> {
            self.events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(env.clone());
            Ok(())
        }
    }
    let bus = SharedBus { events: events.clone() };
    let trigger = Event::new(
        uuid::Uuid::new_v4(),
        EventKind::UserMessage { content: "long task".into() },
    );
    let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
    let cancel_clone = cancel.clone();
    let handle = tokio::spawn(async move {
        loop_.run_once(&trigger, &bus, &cancel_clone, &SessionMap::new()).await
    });
    // Give the agent time to enter `stream.next().await`, then cancel.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.notify_one();
    let result = handle.await.expect("task didn't panic");
    assert!(result.is_ok(), "cancel should not be an error: {result:?}");

    let events = events.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let cancel_event = events
        .iter()
        .find(|e| matches!(e.event.kind, EventKind::SessionCancelled { .. }));
    let final_msg = events
        .iter()
        .find(|e| matches!(e.event.kind, EventKind::AgentMessage { .. }));
    assert!(
        cancel_event.is_some(),
        "expected SessionCancelled; events: {:?}",
        events
            .iter()
            .map(|e| event_type_name(&e.event.kind))
            .collect::<Vec<_>>()
    );
    assert!(final_msg.is_none(), "no final AgentMessage on cancel");
}

#[tokio::test]
async fn agent_loop_aborts_between_iterations_when_cancel_precedes_run() {
    // Cancel is signalled BEFORE the loop starts; the iteration
    // boundary check should catch it and the loop should exit before
    // any LLM call.
    let _env = acquire_env_lock();
    std::env::set_var("OPENAI_API_KEY", "sk-test");

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sample_two_delta_stream()),
        )
        .mount(&server)
        .await;

    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(OpenAiChatProvider::with_base_url(server.uri())));
    let spec = planner_spec();
    let loop_ = AgentLoop::from_spec(spec, registry, vec![]).unwrap();

    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<EventEnvelope>::new()));
    struct SharedBus {
        events: std::sync::Arc<std::sync::Mutex<Vec<EventEnvelope>>>,
    }
    #[async_trait]
    impl crate::tools::Publisher for SharedBus {
        async fn publish(&self, env: &EventEnvelope) -> anyhow::Result<()> {
            self.events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(env.clone());
            Ok(())
        }
    }
    let bus = SharedBus { events: events.clone() };
    let trigger = Event::new(
        uuid::Uuid::new_v4(),
        EventKind::UserMessage { content: "shouldn't even start".into() },
    );
    let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
    // Pre-cancel before the loop runs.
    cancel.notify_one();

    let result = loop_.run_once(&trigger, &bus, &cancel, &SessionMap::new()).await;
    assert!(result.is_ok());

    let events = events.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let llm_count = events
        .iter()
        .filter(|e| matches!(e.event.kind, EventKind::LlmCall { .. }))
        .count();
    let cancel_event = events
        .iter()
        .find(|e| matches!(e.event.kind, EventKind::SessionCancelled { .. }));
    assert_eq!(llm_count, 0, "pre-cancelled loop should never call the LLM");
    assert!(cancel_event.is_some(), "expected SessionCancelled");
}