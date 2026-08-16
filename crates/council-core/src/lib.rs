//! `council-core` — shared types for the Council orchestrator and agent processes.
//!
//! Anything that crosses a process boundary (orchestrator ↔ agent, agent ↔ UI over
//! WebSocket, or agent ↔ agent over Redis pub/sub) is defined here so the wire
//! contract has a single source of truth.

pub mod agent;
pub mod bus;
pub mod control;
pub mod error;
pub mod event;
pub mod session;
pub mod tool;

pub use agent::{AgentSpec, ModelConfig, PromptConfig, ToolsConfig};
pub use bus::{channels, EventEnvelope, EVENTS_CHANNEL};
pub use control::{ControlEnvelope, ControlEvent, CONTROL_CHANNEL};
pub use error::{CoreError, Result};
pub use event::{AgentLifecycle, Event, EventId, EventKind, FileChangeKind};
pub use session::{Session, SessionId, SessionStatus};
pub use tool::{Tool, ToolContext, ToolOutput};
