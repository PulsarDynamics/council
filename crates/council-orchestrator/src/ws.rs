//! WebSocket endpoint. Streams every event the orchestrator sees on the
//! Redis bus to the client as a JSON-per-message text stream.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};

use crate::state::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.events_tx.subscribe();

    // Pump: events from the broadcast channel -> WebSocket text frames.
    let send_task = tokio::spawn(async move {
        while let Ok(env) = rx.recv().await {
            let bytes = match env.encode() {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to encode envelope for WS");
                    continue;
                }
            };
            if sender.send(Message::Binary(bytes)).await.is_err() {
                break;
            }
        }
    });

    // Receive: ignore inbound messages (the scaffold is read-only from the UI).
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }
}
