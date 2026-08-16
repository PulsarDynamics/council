//! The seven tools the agent can hand to the LLM. Real file ops for the
//! filesystem ones, real `tokio::process::Command` for `run_command`,
//! and event-driven stubs for `delegate_to` and `ask_user` (they publish
//! on the bus and return immediately; the LLM loop keeps going).

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use council_core::{EventEnvelope, EventKind, Tool, ToolContext, ToolOutput};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::info;

/// Trait alias for anything that can publish an envelope to the bus. Lets
/// us decouple tools from the concrete `Bus` type.
#[async_trait]
pub trait Publisher: Send + Sync {
    async fn publish(&self, env: &EventEnvelope) -> Result<()>;
}

// ---------------- read_file ----------------

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read the contents of a file at the given path. Returns UTF-8 text."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute or relative path to the file." }
            },
            "required": ["path"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'path'".to_string())?;
        match tokio::fs::read_to_string(path).await {
            Ok(content) => Ok(json!({ "path": path, "content": content, "lines": content.lines().count() })),
            Err(e) => Err(format!("read_file({path}): {e}")),
        }
    }
}

// ---------------- write_file ----------------

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Write `content` to the file at `path`, creating it (and parent directories) if needed."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutput {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'path'".to_string())?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'content'".to_string())?;
        let p = PathBuf::from(path);
        if let Some(parent) = p.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| format!("mkdir: {e}"))?;
        }
        let prev = tokio::fs::read_to_string(&p).await.ok();
        tokio::fs::write(&p, content)
            .await
            .map_err(|e| format!("write_file({path}): {e}"))?;
        // Best-effort: a real impl would publish a `file_change` event here.
        info!(agent = %ctx.agent_name, path, "write_file");
        let kind = if prev.is_none() { "created" } else { "modified" };
        Ok(json!({ "path": path, "kind": kind, "bytes": content.len() }))
    }
}

// ---------------- edit_file ----------------

pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }
    fn description(&self) -> &str {
        "Replace the first occurrence of `old` with `new` in the file at `path`."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old": { "type": "string", "description": "The text to find." },
                "new": { "type": "string", "description": "The replacement text." }
            },
            "required": ["path", "old", "new"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'path'".to_string())?;
        let old = args
            .get("old")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'old'".to_string())?;
        let new = args
            .get("new")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'new'".to_string())?;
        let original = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("read {path}: {e}"))?;
        let occurrences = original.matches(old).count();
        if occurrences == 0 {
            return Err(format!("edit_file: 'old' not found in {path}"));
        }
        if occurrences > 1 {
            return Err(format!(
                "edit_file: 'old' matched {occurrences} times in {path}; make it more specific"
            ));
        }
        let updated = original.replacen(old, new, 1);
        tokio::fs::write(path, &updated)
            .await
            .map_err(|e| format!("write {path}: {e}"))?;
        Ok(json!({ "path": path, "replacements": 1 }))
    }
}

// ---------------- list_dir ----------------

pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }
    fn description(&self) -> &str {
        "List the entries in a directory. Returns a list of {name, kind} objects."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory to list. Use '.' for the current dir." }
            },
            "required": ["path"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'path'".to_string())?;
        let mut entries = Vec::new();
        let mut rd = tokio::fs::read_dir(path)
            .await
            .map_err(|e| format!("read_dir({path}): {e}"))?;
        while let Some(e) = rd.next_entry().await.map_err(|e| e.to_string())? {
            let name = e.file_name().to_string_lossy().to_string();
            let kind = match e.file_type().await {
                Ok(ft) if ft.is_dir() => "dir",
                Ok(ft) if ft.is_file() => "file",
                _ => "other",
            }
            .to_string();
            entries.push(json!({ "name": name, "kind": kind }));
        }
        entries.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        Ok(json!({ "path": path, "entries": entries, "count": entries.len() }))
    }
}

// ---------------- run_command ----------------

pub struct RunCommandTool;

#[async_trait]
impl Tool for RunCommandTool {
    fn name(&self) -> &str {
        "run_command"
    }
    fn description(&self) -> &str {
        "Run a shell command. Returns stdout, stderr, and exit code. 30s timeout."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The full command line." },
                "timeout_ms": { "type": "integer", "description": "Optional timeout override." }
            },
            "required": ["command"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let cmd = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'command'".to_string())?;
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(30_000);
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn: {e}"))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let mut out_buf = String::new();
        let mut err_buf = String::new();
        if let Some(s) = stdout {
            let mut lines = BufReader::new(s).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                out_buf.push_str(&line);
                out_buf.push('\n');
            }
        }
        if let Some(s) = stderr {
            let mut lines = BufReader::new(s).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                err_buf.push_str(&line);
                err_buf.push('\n');
            }
        }
        let res = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), child.wait()).await;
        let code = match res {
            Ok(Ok(s)) => s.code(),
            Ok(Err(e)) => return Err(format!("wait: {e}")),
            Err(_) => return Err(format!("timeout after {timeout_ms}ms")),
        };
        Ok(json!({
            "stdout": out_buf,
            "stderr": err_buf,
            "exit_code": code,
        }))
    }
}

// ---------------- delegate_to ----------------

pub struct DelegateToTool {
    pub publisher: Arc<dyn Publisher>,
    pub target_channel: String, // usually "goal"
}

#[async_trait]
impl Tool for DelegateToTool {
    fn name(&self) -> &str {
        "delegate_to"
    }
    fn description(&self) -> &str {
        "Hand a goal off to another agent. Publishes a UserMessage on the \
         target channel (default 'goal'). Use sparingly — most deliberation \
         should flow through the wire contract, not this tool."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "goal": { "type": "string", "description": "The goal to hand off." }
            },
            "required": ["goal"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutput {
        let goal = args
            .get("goal")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'goal'".to_string())?;
        let env = EventEnvelope::new(
            &self.target_channel,
            council_core::Event::new(
                ctx.session_id,
                EventKind::UserMessage {
                    content: format!("[from {}] {}", ctx.agent_name, goal),
                },
            ),
        );
        self.publisher
            .publish(&env)
            .await
            .map_err(|e| format!("publish: {e}"))?;
        Ok(json!({ "delegated": true, "channel": self.target_channel }))
    }
}

// ---------------- ask_user ----------------

pub struct AskUserTool {
    pub publisher: Arc<dyn Publisher>,
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }
    fn description(&self) -> &str {
        "Pause and ask the user a question. Publishes an event; the response \
         flow is wired in a later cycle. For the scaffold this returns a \
         stub answer so the LLM loop doesn't block."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": { "type": "string" }
            },
            "required": ["question"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutput {
        let q = args
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("(no question)");
        // Publish so the UI can show it, but don't block waiting for input.
        let env = EventEnvelope::new(
            "broadcast",
            council_core::Event::new(
                ctx.session_id,
                EventKind::System {
                    message: format!("[{}] asks: {}", ctx.agent_name, q),
                },
            ),
        );
        let _ = self.publisher.publish(&env).await;
        Ok(json!({
            "scaffold_stub": true,
            "hint": "real ask_user flow lands in a later cycle",
            "question": q,
        }))
    }
}

// ---------------- search_code (Designer only) ----------------

pub struct SearchCodeTool;

#[async_trait]
impl Tool for SearchCodeTool {
    fn name(&self) -> &str {
        "search_code"
    }
    fn description(&self) -> &str {
        "Recursively grep a directory for a regex. Returns up to 50 matches \
         with file:line:content."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "pattern": { "type": "string" },
                "max_results": { "type": "integer" }
            },
            "required": ["path", "pattern"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'path'".to_string())?;
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'pattern'".to_string())?;
        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;
        // Use ripgrep if available, else fall back to grep -R.
        let cmd = format!(
            "rg --no-heading -n --max-count 1 {pat} {path} 2>/dev/null || grep -RHn -- {pat} {path} 2>/dev/null",
            pat = shell_escape(pattern),
            path = shell_escape(path)
        );
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(&cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn: {e}"))?;
        let mut out = String::new();
        if let Some(s) = child.stdout.take() {
            let mut lines = BufReader::new(s).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                out.push_str(&line);
                out.push('\n');
                if out.lines().count() >= max {
                    break;
                }
            }
        }
        let _ = child.wait().await;
        Ok(json!({ "path": path, "pattern": pattern, "matches": out }))
    }
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ---------------- registry of built-in tools ----------------

/// Built-in toolset. The agent passes its TOML `[tools].allowed` list
/// through this filter to produce the per-agent registry.
pub fn builtin_tools(publisher: Arc<dyn Publisher>) -> Vec<Arc<dyn Tool>> {
    use crate::tools_web::{
        open_memory, MemoryDeleteTool, MemoryGetTool, MemoryListTool, MemorySearchTool,
        MemorySetTool, WebSearchTool,
    };

    let memory = open_memory().expect("open memory store");

    let mut v: Vec<Arc<dyn Tool>> = Vec::new();
    v.push(Arc::new(ReadFileTool));
    v.push(Arc::new(WriteFileTool));
    v.push(Arc::new(EditFileTool));
    v.push(Arc::new(ListDirTool));
    v.push(Arc::new(RunCommandTool));
    v.push(Arc::new(DelegateToTool {
        publisher: publisher.clone(),
        target_channel: "goal".into(),
    }));
    v.push(Arc::new(AskUserTool { publisher }));
    v.push(Arc::new(SearchCodeTool));
    v.push(Arc::new(WebSearchTool::new()));
    v.push(Arc::new(MemorySetTool(memory.clone())));
    v.push(Arc::new(MemoryGetTool(memory.clone())));
    v.push(Arc::new(MemoryDeleteTool(memory.clone())));
    v.push(Arc::new(MemoryListTool(memory.clone())));
    v.push(Arc::new(MemorySearchTool(memory)));
    v
}

/// Build a filtered list of tools, keeping only those in `allowed`.
pub fn filter_tools(tools: Vec<Arc<dyn Tool>>, allowed: &std::collections::BTreeSet<String>) -> Vec<Arc<dyn Tool>> {
    tools.into_iter().filter(|t| allowed.contains(t.name())).collect()
}
