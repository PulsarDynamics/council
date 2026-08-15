//! Council orchestrator library. The binary entry point is in `main.rs`.

use std::net::SocketAddr;

use anyhow::Result;
use axum::{routing::get, Json, Router};
use serde_json::json;
use tracing::info;

/// Start the orchestrator on the given bind address. The scaffold boots an
/// Axum server with a `/health` endpoint; the real HTTP API, WebSocket, Redis
/// subscriber, and agent process manager land in later cycles.
pub async fn serve(bind: SocketAddr) -> Result<()> {
    let app = Router::new()
        .route("/health", get(health))
        .route("/version", get(version));

    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "Council orchestrator listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn version() -> Json<serde_json::Value> {
    Json(json!({
        "name": "council-orchestrator",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
