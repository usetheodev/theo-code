//! Shared test fixtures for resume_*_tests.rs sibling files (T3.7 split).
#![cfg(test)]
#![allow(unused_imports)]

use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use super::*;
use crate::config::AgentConfig;
use crate::event_bus::EventBus;
use crate::subagent::SubAgentRegistry;
use crate::subagent_runs::{FileSubagentRunStore, SubagentRun};
use tempfile::TempDir;

pub(super) fn fixture_spec(name: &str) -> AgentSpec {
    AgentSpec::on_demand(name, "test obj")
}

pub(super) fn fixture_run(spec: &AgentSpec, status: RunStatus) -> SubagentRun {
    let mut run = SubagentRun::new_running("r-test", None, spec, "obj", "/tmp", None);
    run.status = status;
    run
}

pub(super) fn make_store() -> (TempDir, FileSubagentRunStore) {
    let dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(dir.path());
    (dir, store)
}

pub(super) fn make_manager() -> SubAgentManager {
    SubAgentManager::with_registry(
        AgentConfig::default(),
        Arc::new(EventBus::new()),
        PathBuf::from("/tmp"),
        Arc::new(SubAgentRegistry::with_builtins()),
    )
}

