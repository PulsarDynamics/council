//! Agent process manager. The orchestrator spawns one `council-agent`
//! subprocess per TOML config under `agents/`, watches their exits, and
//! restarts them with exponential backoff on crash.
//!
//! All event communication goes over Redis (see `bus.rs`). The subprocess
//! management is just lifecycle — nothing the agent does crosses a
//! process boundary except via the bus.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use council_core::AgentSpec;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

/// One managed agent subprocess + its parsed spec.
struct ManagedAgent {
    spec: AgentSpec,
    config_path: PathBuf,
    child: Option<Child>,
    /// Set when we're asked to stop (used by tests + graceful shutdown).
    stopped: bool,
}

/// Owns all managed agent subprocesses. Cheap to construct; load it once
/// at startup with `ProcessManager::load()`.
pub struct ProcessManager {
    agents: Vec<ManagedAgent>,
    agent_bin: PathBuf,
}

/// A lifecycle event the manager emits, so the caller (orchestrator) can
/// publish `agent_status` events on the bus.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Started { name: String },
    Stdout { name: String, line: String },
    Exited { name: String, code: Option<i32> },
    Restarting { name: String, attempt: u32, delay_ms: u64 },
    Failed { name: String, error: String },
}

impl ProcessManager {
    /// Load every `*.toml` under `dir` as an AgentSpec. Does not spawn yet.
    pub fn load(dir: &Path, agent_bin: PathBuf) -> Result<Self> {
        let mut agents = Vec::new();
        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("reading agents dir {}", dir.display()))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let spec: AgentSpec =
                toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
            // Sanity: the agent's `name` must match the filename stem.
            // (We saw this blow up in the original Agent Orchestra attempt.)
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem != spec.name {
                    anyhow::bail!(
                        "agent name mismatch: file {} declares name = {:?}",
                        path.display(),
                        spec.name
                    );
                }
            }
            agents.push(ManagedAgent {
                spec,
                config_path: path,
                child: None,
                stopped: false,
            });
        }
        agents.sort_by(|a, b| a.spec.name.cmp(&b.spec.name));
        Ok(Self {
            agents,
            agent_bin,
        })
    }

    /// How many agents we manage.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Iterate the parsed specs (read-only). Kept public so future
    /// `/api/agents` and health endpoints can surface them.
    #[allow(dead_code)]
    pub fn specs(&self) -> impl Iterator<Item = &AgentSpec> {
        self.agents.iter().map(|a| &a.spec)
    }

    /// Spawn every agent process. Streams lifecycle events on `tx`.
    pub fn start_all(&mut self, tx: mpsc::Sender<AgentEvent>) {
        for agent in &mut self.agents {
            Self::spawn_one(agent, &self.agent_bin, &tx);
        }
    }

    fn spawn_one(agent: &mut ManagedAgent, bin: &Path, tx: &mpsc::Sender<AgentEvent>) {
        let name = agent.spec.name.clone();
        let config = agent.config_path.clone();
        let tx = tx.clone();
        let bin = bin.to_path_buf();
        tokio::spawn(async move {
            spawn_and_supervise(name, config, bin, tx).await;
        });
    }

    /// Wait for all currently-running agent tasks to finish. Used in tests
    /// and as a shutdown signal.
    pub async fn shutdown(&mut self) {
        for agent in &mut self.agents {
            agent.stopped = true;
            if let Some(child) = agent.child.as_mut() {
                let _ = child.start_kill();
            }
        }
    }
}

/// Supervise one agent: spawn it, pump its stdout to the lifecycle channel,
/// wait for exit, schedule a restart with backoff, repeat.
async fn spawn_and_supervise(
    name: String,
    config: PathBuf,
    bin: PathBuf,
    tx: mpsc::Sender<AgentEvent>,
) {
    let mut attempt: u32 = 0;
    loop {
        info!(agent = %name, "spawning");
        let mut cmd = Command::new(&bin);
        cmd.arg("run")
            .arg("--config")
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx
                    .send(AgentEvent::Failed {
                        name: name.clone(),
                        error: format!("spawn failed: {e}"),
                    })
                    .await;
                return;
            }
        };

        let _ = tx.send(AgentEvent::Started { name: name.clone() }).await;

        // Pump stdout + stderr.
        if let Some(stdout) = child.stdout.take() {
            let tx_out = tx.clone();
            let n_out = name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx_out
                        .send(AgentEvent::Stdout { name: n_out.clone(), line })
                        .await;
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let n_err = name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    warn!(agent = %n_err, "{}", line);
                }
            });
        }

        // Wait for exit.
        let exit = child.wait().await;
        let code = match &exit {
            Ok(s) => s.code(),
            Err(e) => {
                error!(agent = %name, error = %e, "wait failed");
                None
            }
        };
        let _ = tx
            .send(AgentEvent::Exited {
                name: name.clone(),
                code,
            })
            .await;

        // Decide: restart or stop. For now, always restart (until told to stop).
        attempt = attempt.saturating_add(1);
        let backoff_ms = (500u64 << attempt.min(6)).min(30_000);
        let _ = tx
            .send(AgentEvent::Restarting {
                name: name.clone(),
                attempt,
                delay_ms: backoff_ms,
            })
            .await;
        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
    }
}

/// Helper used by tests / external callers to generate a unique session id
/// for a new goal submission. Re-exported so we don't pull in `uuid`
/// directly from the orchestrator's callers.
#[allow(dead_code)]
pub fn new_session_id() -> Uuid {
    Uuid::new_v4()
}
