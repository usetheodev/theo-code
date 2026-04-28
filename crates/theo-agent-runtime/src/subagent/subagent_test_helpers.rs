//! Shared test fixtures for subagent_*_tests.rs sibling files (T3.5 split).
#![cfg(test)]
#![allow(unused_imports)]

use std::sync::Mutex;

use super::*;
use crate::event_bus::EventListener;
use theo_domain::event::DomainEvent;

/// Serialise tests that mutate `THEO_*` env vars (Rust 2024 made
/// `set_var`/`remove_var` unsafe — concurrent reads/writes are UB).
pub(super) fn mcp_env_lock() -> &'static tokio::sync::Mutex<()> {
    use tokio::sync::Mutex as TokioMutex;
    static M: std::sync::OnceLock<TokioMutex<()>> = std::sync::OnceLock::new();
    M.get_or_init(|| TokioMutex::new(()))
}

/// Test helper: captures events published to the bus.
pub(super) struct CaptureListener {
    events: Mutex<Vec<DomainEvent>>,
}

impl CaptureListener {
    pub(super) fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn events(&self) -> Vec<DomainEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl EventListener for CaptureListener {
    fn on_event(&self, e: &DomainEvent) {
        self.events.lock().unwrap().push(e.clone());
    }
}
