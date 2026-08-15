//! The Tool trait. Seven tools ship in the starter `council-agent` binary
//! (`read_file`, `write_file`, `edit_file`, `list_dir`, `run_command`,
//! `delegate_to`, `ask_user`). The `Designer` agent also gets `search_code`.

use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

pub type ToolOutput = Result<Value, String>;

/// Context passed to a tool on every invocation.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub session_id: Uuid,
    pub agent_name: String,
}

#[async_trait]
pub trait Tool: Send + Sync {
    /// Stable name used in agent TOML allow-lists. Must be unique per binary.
    fn name(&self) -> &str;

    /// One-line description surfaced to the LLM.
    fn description(&self) -> &str;

    /// JSON schema describing the tool's `args` shape. Passed to the LLM.
    fn schema(&self) -> Value;

    /// Execute the tool. `args` is the JSON object the LLM produced.
    /// Return a `String` error for tool-level failures (caller will wrap in
    /// a `ToolResult { error: Some(...) }` event).
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutput;
}
