//! Crate-wide error type. Library code uses [`CoreError`]; binary entry points
//! in `council-orchestrator` and `council-agent` use `anyhow` for ergonomic
//! error chaining at the edges.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde_json error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("channel not found: {0}")]
    ChannelNotFound(String),

    #[error("invalid event: {0}")]
    InvalidEvent(String),

    #[error("config error: {0}")]
    Config(String),
}
