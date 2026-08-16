//! Web search + memory tools. The LLM can search the live web and store
//! notes in a per-agent SQLite database that persists across sessions.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use council_core::{Tool, ToolContext, ToolOutput};
use rusqlite::{params, Connection};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use tracing::info;
use url::Url;

// ---------------- web_search ----------------

pub struct WebSearchTool {
    http: reqwest::Client,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (Council-orchestrator)")
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("reqwest client"),
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Search the web via DuckDuckGo's HTML endpoint (no API key needed). \
         Returns up to 10 results as a list of {title, url, snippet}. \
         Use this for any question about current events, library docs, or facts \
         that might have changed since your training cutoff."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "max_results": { "type": "integer", "minimum": 1, "maximum": 20 }
            },
            "required": ["query"]
        })
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutput {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'query'".to_string())?;
        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(8) as usize;

        // DuckDuckGo HTML. The `kl=` param is a region code; default
        // `us-en`. POST avoids URL-length limits on long queries.
        let url = "https://html.duckduckgo.com/html/";
        let form = [("q", query), ("kl", "us-en")];
        let body = self
            .http
            .post(url)
            .form(&form)
            .send()
            .await
            .map_err(|e| format!("web_search http: {e}"))?;
        let html = body
            .text()
            .await
            .map_err(|e| format!("web_search body: {e}"))?;

        // Parse. The result rows are `.result` anchors with a sibling
        // `.result__snippet` and a `.result__url` for the URL. DuckDuckGo
        // obfuscates URLs via redirects; the visible URL is in
        // `.result__url`.
        let doc = Html::parse_document(&html);
        let result_sel = Selector::parse(".result").unwrap();
        let title_sel = Selector::parse(".result__a").unwrap();
        let snippet_sel = Selector::parse(".result__snippet").unwrap();
        let url_sel = Selector::parse(".result__url").unwrap();

        let mut out: Vec<Value> = Vec::new();
        for r in doc.select(&result_sel).take(max) {
            let title = r
                .select(&title_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();
            let snippet = r
                .select(&snippet_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();
            let url_text = r
                .select(&url_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();
            if title.is_empty() && snippet.is_empty() {
                continue;
            }
            // DuckDuckGo's visible URL has leading whitespace; trim and
            // ensure scheme. The string might be just the host + path.
            let normalized_url = if url_text.starts_with("http") {
                url_text.clone()
            } else if url_text.is_empty() {
                String::new()
            } else {
                format!("https://{url_text}")
            };
            out.push(json!({
                "title": title,
                "url": normalized_url,
                "snippet": snippet,
            }));
        }
        info!(query, count = out.len(), "web_search done");
        Ok(json!({
            "query": query,
            "results": out,
            "count": out.len(),
        }))
    }
}

// ---------------- memory ----------------

/// Persistent per-agent key/value store. SQLite at
/// `~/.config/council/memory.sqlite`. Each key is namespaced by the
/// agent name so two agents don't collide.
pub struct MemoryStore {
    conn: Arc<std::sync::Mutex<Connection>>,
}

impl MemoryStore {
    /// Open or create the memory DB. The path is shared across all
    /// agents on this machine.
    pub fn open(path: &PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS memory (
                agent TEXT NOT NULL,
                key   TEXT NOT NULL,
                value TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (agent, key)
            );
            CREATE INDEX IF NOT EXISTS memory_agent_idx ON memory(agent);
            "#,
        )?;
        Ok(Self {
            conn: Arc::new(std::sync::Mutex::new(conn)),
        })
    }

    pub fn set(&self, agent: &str, key: &str, value: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO memory (agent, key, value, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?4) \
             ON CONFLICT(agent, key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![agent, key, value, now],
        )?;
        Ok(())
    }

    pub fn get(&self, agent: &str, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM memory WHERE agent=?1 AND key=?2")?;
        let v: Option<String> = stmt
            .query_row(params![agent, key], |r| r.get(0))
            .ok();
        Ok(v)
    }

    pub fn delete(&self, agent: &str, key: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM memory WHERE agent=?1 AND key=?2",
            params![agent, key],
        )?;
        Ok(n > 0)
    }

    pub fn list(&self, agent: &str, prefix: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("{prefix}%");
        let mut stmt = conn.prepare(
            "SELECT key, value FROM memory WHERE agent=?1 AND key LIKE ?2 ORDER BY key",
        )?;
        let rows = stmt
            .query_map(params![agent, pattern], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn search(&self, agent: &str, needle: &str) -> Result<Vec<(String, String)>> {
        // Lightweight case-insensitive LIKE search over both key and value.
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", needle.to_lowercase());
        let mut stmt = conn.prepare(
            "SELECT key, value FROM memory \
             WHERE agent=?1 AND (LOWER(key) LIKE ?2 OR LOWER(value) LIKE ?2) \
             ORDER BY updated_at DESC LIMIT 50",
        )?;
        let rows = stmt
            .query_map(params![agent, pattern], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

fn memory_path() -> PathBuf {
    if let Ok(p) = std::env::var("COUNCIL_MEMORY_FILE") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/council/memory.sqlite");
    }
    PathBuf::from("./memory.sqlite")
}

/// Lazily-open the memory store. Cheap; re-opens per process.
pub fn open_memory() -> Result<Arc<MemoryStore>> {
    let path = memory_path();
    Ok(Arc::new(MemoryStore::open(&path)?))
}

pub struct MemorySetTool(pub Arc<MemoryStore>);
pub struct MemoryGetTool(pub Arc<MemoryStore>);
pub struct MemoryDeleteTool(pub Arc<MemoryStore>);
pub struct MemoryListTool(pub Arc<MemoryStore>);
pub struct MemorySearchTool(pub Arc<MemoryStore>);

#[async_trait]
impl Tool for MemorySetTool {
    fn name(&self) -> &str {
        "memory_set"
    }
    fn description(&self) -> &str {
        "Store a string under a key in the agent's persistent memory. \
         The key is namespaced by agent name, so two agents can use the \
         same key without colliding. Memory survives across sessions."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": { "type": "string" },
                "value": { "type": "string" }
            },
            "required": ["key", "value"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutput {
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'key'".to_string())?;
        let value = args
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'value'".to_string())?;
        self.0
            .set(&ctx.agent_name, key, value)
            .map_err(|e| format!("memory_set: {e}"))?;
        Ok(json!({ "ok": true, "key": key }))
    }
}

#[async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }
    fn description(&self) -> &str {
        "Read a value from the agent's persistent memory by key. \
         Returns null if the key is not set."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "key": { "type": "string" } },
            "required": ["key"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutput {
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'key'".to_string())?;
        let v = self
            .0
            .get(&ctx.agent_name, key)
            .map_err(|e| format!("memory_get: {e}"))?;
        Ok(json!({ "key": key, "value": v }))
    }
}

#[async_trait]
impl Tool for MemoryDeleteTool {
    fn name(&self) -> &str {
        "memory_delete"
    }
    fn description(&self) -> &str {
        "Delete a key from the agent's persistent memory."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "key": { "type": "string" } },
            "required": ["key"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutput {
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'key'".to_string())?;
        let removed = self
            .0
            .delete(&ctx.agent_name, key)
            .map_err(|e| format!("memory_delete: {e}"))?;
        Ok(json!({ "ok": removed, "key": key }))
    }
}

#[async_trait]
impl Tool for MemoryListTool {
    fn name(&self) -> &str {
        "memory_list"
    }
    fn description(&self) -> &str {
        "List all keys in the agent's memory, optionally filtered by a key prefix."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "prefix": { "type": "string" } }
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutput {
        let prefix = args
            .get("prefix")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let rows = self
            .0
            .list(&ctx.agent_name, prefix)
            .map_err(|e| format!("memory_list: {e}"))?;
        Ok(json!({
            "prefix": prefix,
            "count": rows.len(),
            "items": rows.iter().map(|(k, v)| json!({"key": k, "value": v})).collect::<Vec<_>>()
        }))
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }
    fn description(&self) -> &str {
        "Case-insensitive substring search across both keys and values in the \
         agent's memory. Returns up to 50 matches, most-recently-updated first."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutput {
        let q = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing 'query'".to_string())?;
        let rows = self
            .0
            .search(&ctx.agent_name, q)
            .map_err(|e| format!("memory_search: {e}"))?;
        Ok(json!({
            "query": q,
            "count": rows.len(),
            "items": rows.iter().map(|(k, v)| json!({"key": k, "value": v})).collect::<Vec<_>>()
        }))
    }
}
