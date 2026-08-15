//! HTTP API. The scaffold exposes:
//! - GET  /health
//! - GET  /version
//! - GET  /api/agents           — list loaded agent specs
//! - POST /api/sessions         — submit a goal; creates a session, publishes
//!   SessionCreated + UserMessage envelopes.

use std::sync::Arc;

use anyhow::Context;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use council_core::{channels, Event, EventEnvelope, Session};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

pub async fn version() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "council-orchestrator",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[derive(Debug, Serialize)]
pub struct AgentDto {
    pub name: String,
    pub subscribes: Vec<String>,
    pub publishes: Vec<String>,
    pub model: String,
    pub provider: String,
    pub tools: Vec<String>,
}

pub async fn list_agents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<AgentDto>>, StatusCode> {
    // The process manager isn't on AppState yet — expose via a one-shot
    // collection on state. For now this is a placeholder; the WS channel
    // is the real-time view of the Council.
    let _ = &state;
    Ok(Json(Vec::new()))
}

#[derive(Debug, Deserialize)]
pub struct SubmitGoalRequest {
    pub goal: String,
}

#[derive(Debug, Serialize)]
pub struct SubmitGoalResponse {
    pub session_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn submit_goal(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubmitGoalRequest>,
) -> Result<Json<SubmitGoalResponse>, (StatusCode, String)> {
    let goal = req.goal.trim();
    if goal.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "goal must not be empty".into()));
    }

    let session = Session::new(goal);
    let session_id = session.id;

    // SessionCreated -> broadcast
    state
        .bus
        .publish(&EventEnvelope::new(
            channels::BROADCAST,
            Event::new(session_id, council_core::EventKind::SessionCreated { goal: goal.into() }),
        ))
        .await
        .context("publishing SessionCreated")
        .map_err(internal)?;

    // UserMessage -> goal channel (planner subscribes to "goal")
    state
        .bus
        .publish(&EventEnvelope::new(
            channels::GOAL,
            Event::new(session_id, council_core::EventKind::UserMessage { content: goal.into() }),
        ))
        .await
        .context("publishing UserMessage")
        .map_err(internal)?;

    Ok(Json(SubmitGoalResponse {
        session_id,
        created_at: session.created_at,
    }))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
