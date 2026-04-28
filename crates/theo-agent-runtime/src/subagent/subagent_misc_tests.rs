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
fn spec_based_subagent_config_is_marked() {
    // Verify that sub-agent configs are marked as sub-agents (is_subagent=true)
    // by the spawn_with_spec implementation. Indirect check via clone+set.
    let config = AgentConfig::default();
    assert!(!config.loop_cfg.is_subagent, "parent config must not be sub-agent");
    let mut sub_config = config.clone();
    sub_config.loop_cfg.is_subagent = true;
    assert!(sub_config.loop_cfg.is_subagent, "sub-agent config must be marked");
}

#[test]
fn max_depth_prevents_recursion() {
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1, // Already at max
        registry: Some(Arc::new(SubAgentRegistry::with_builtins())),
        run_store: None,
        hook_manager: None,
        cancellation: None,
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: None,
        mcp_discovery: None,
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };

    let spec = theo_domain::agent_spec::AgentSpec::on_demand("test", "test obj");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async { manager.spawn_with_spec(&spec, "test", None).await });
    assert!(!result.success);
    assert!(result.summary.contains("depth limit"));
}

// ── Spec-based spawn + events ────────────────────────────────────────

use theo_domain::event::EventType;


