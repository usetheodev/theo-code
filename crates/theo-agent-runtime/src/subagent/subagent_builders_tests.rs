//! Sibling test body of `subagent/mod.rs` — split per-feature (T3.5 of code-hygiene-5x5).
//!
//! Test-only file; gates use the inner `cfg(test)` attribute below to
//! classify every line as test code.

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::subagent_test_helpers::{mcp_env_lock, CaptureListener};
use super::*;
use theo_domain::tool::ToolCategory;

#[test]
fn registry_resolves_builtin_names() {
    let reg = SubAgentRegistry::with_builtins();
    assert!(reg.get("explorer").is_some());
    assert!(reg.get("implementer").is_some());
    assert!(reg.get("verifier").is_some());
    assert!(reg.get("reviewer").is_some());
    assert!(reg.get("unknown").is_none());
}

#[test]
fn with_builtins_preserves_backward_compat_constructor_signature() {
    // Drop-in replacement for `new()`. Legacy call sites work unchanged.
    let bus = Arc::new(EventBus::new());
    let manager =
        SubAgentManager::with_builtins(AgentConfig::default(), bus, PathBuf::from("/tmp"));
    assert!(manager.registry().is_some());
    // Has 4 builtin specs
    assert_eq!(manager.registry().unwrap().len(), 4);
}

#[test]
fn with_registry_uses_provided_registry() {
    let bus = Arc::new(EventBus::new());
    let mut custom = SubAgentRegistry::new();
    custom.register(theo_domain::agent_spec::AgentSpec::on_demand("x", "y"));
    let manager = SubAgentManager::with_registry(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
        Arc::new(custom),
    );
    assert_eq!(manager.registry().unwrap().len(), 1);
    assert!(manager.registry().unwrap().contains("x"));
}

#[test]
fn with_hooks_builder_stores_reference() {
    use crate::lifecycle_hooks::HookManager;
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager::with_builtins(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
    )
    .with_hooks(Arc::new(HookManager::new()));
    assert!(manager.hook_manager().is_some());
}

#[test]
fn with_worktree_provider_builder_stores_reference() {
    use std::path::PathBuf;
    let provider = Arc::new(theo_isolation::WorktreeProvider::new(
        PathBuf::from("/repo"),
        PathBuf::from("/wt"),
    ));
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager::with_builtins(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
    )
    .with_worktree_provider(provider);
    assert!(manager.worktree_provider.is_some());
}

#[test]
fn with_cancellation_builder_stores_reference() {
    use crate::cancellation::CancellationTree;
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager::with_builtins(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
    )
    .with_cancellation(Arc::new(CancellationTree::new()));
    assert!(manager.cancellation().is_some());
}

#[test]
fn with_run_store_builder_stores_reference() {
    use crate::subagent_runs::FileSubagentRunStore;
    let tempdir = tempfile::TempDir::new().unwrap();
    let store = Arc::new(FileSubagentRunStore::new(tempdir.path()));
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager::with_builtins(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
    )
    .with_run_store(store);
    assert!(manager.run_store().is_some());
}

// ── MCP discovery cache integration ──

#[test]
fn with_mcp_discovery_builder_stores_reference() {
    let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager::with_builtins(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
    )
    .with_mcp_discovery(cache);
    assert!(manager.mcp_discovery().is_some());
}

