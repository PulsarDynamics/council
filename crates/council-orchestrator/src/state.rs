//! Shared application state. Cloned into every Axum handler.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::bus::Bus;

/// Capacity of the in-process broadcast channel that fans Redis events
/// out to WebSocket clients. Tune up if you expect bursts.
pub const WS_BROADCAST_CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct AppState {
    pub bus: Bus,
    /// Sender for the in-process event broadcast. Every Redis event the
    /// orchestrator receives is also pushed here so WS clients can read it.
    pub events_tx: broadcast::Sender<council_core::EventEnvelope>,
}

impl AppState {
    pub fn new(bus: Bus) -> Arc<Self> {
        let (events_tx, _rx) = broadcast::channel(WS_BROADCAST_CAPACITY);
        Arc::new(Self { bus, events_tx })
    }
}
