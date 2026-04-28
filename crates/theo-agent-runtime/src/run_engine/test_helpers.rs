//! Shared test fixtures for run_engine/*_tests.rs sibling files (T3.2 split).
#![cfg(test)]
#![allow(unused_imports)]

use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::event_bus::CapturingListener;
use theo_domain::session::SessionId;
use theo_domain::task::AgentType;

pub(super) struct TestSetup {
    pub bus: Arc<EventBus>,
    pub listener: Arc<CapturingListener>,
    pub tm: Arc<TaskManager>,
    pub tcm: Arc<ToolCallManager>,
}

impl TestSetup {
    pub fn new() -> Self {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());
        let tm = Arc::new(TaskManager::new(bus.clone()));
        let tcm = Arc::new(ToolCallManager::new(bus.clone()));
        Self { bus, listener, tm, tcm }
    }

    pub fn create_engine(&self, task_objective: &str) -> AgentRunEngine {
        let task_id = self
            .tm
            .create_task(SessionId::new("s"), AgentType::Coder, task_objective.into());
        AgentRunEngine::new(
            task_id,
            self.tm.clone(),
            self.tcm.clone(),
            self.bus.clone(),
            LlmClient::new("http://localhost:9999", None, "test"),
            Arc::new(theo_tooling::registry::create_default_registry()),
            AgentConfig::default(),
            PathBuf::from("/tmp"),
        )
    }
}
