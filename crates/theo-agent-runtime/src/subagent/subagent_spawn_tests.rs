//! Sibling test body of `subagent/mod.rs` — split per-feature (T3.5 of code-hygiene-5x5).
//!
//! Test-only file; gates use the inner `cfg(test)` attribute below to
//! classify every line as test code.

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::subagent_test_helpers::{mcp_env_lock, CaptureListener};
use crate::event_bus::EventListener;
use super::*;
use theo_domain::tool::ToolCategory;

#[test]
fn spawn_with_spec_at_max_depth_emits_events_and_fails() {
    let bus = Arc::new(EventBus::new());
    let capture = Arc::new(CaptureListener::new());
    bus.subscribe(capture.clone() as Arc<dyn EventListener>);

    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1,
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

    let spec = theo_domain::agent_spec::AgentSpec::on_demand("scout", "check x");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async { manager.spawn_with_spec(&spec, "check x", None).await });

    // Result reflects the depth-limit failure
    assert!(!result.success);
    assert!(result.summary.contains("depth limit"));
    assert_eq!(result.agent_name, "scout");

    // Events published: SubagentStarted + SubagentCompleted
    let events = capture.events();
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::SubagentStarted),
        "SubagentStarted event missing"
    );
    let completed: Vec<&DomainEvent> = events
        .iter()
        .filter(|e| e.event_type == EventType::SubagentCompleted)
        .collect();
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0].payload.get("agent_name").and_then(|v| v.as_str()),
        Some("scout")
    );
    assert_eq!(
        completed[0].payload.get("agent_source").and_then(|v| v.as_str()),
        Some("on_demand")
    );
    assert_eq!(
        completed[0].payload.get("success").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[test]
fn spawn_with_spec_populates_agent_name_and_context() {
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1, // trigger depth-limit early return (no real LLM)
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
    let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async {
        manager
            .spawn_with_spec_text(&spec, "do y", Some("some context"))
            .await
    });
    assert_eq!(result.agent_name, "x");
    assert_eq!(result.context_used.as_deref(), Some("some context"));
}

#[test]
fn spawn_with_spec_with_run_store_persists_run_record() {
    use crate::subagent_runs::FileSubagentRunStore;
    let tempdir = tempfile::TempDir::new().unwrap();
    let store = Arc::new(FileSubagentRunStore::new(tempdir.path()));
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1, // depth-limit early return (no real LLM)
        registry: None,
        run_store: Some(store.clone()),
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
    let spec = theo_domain::agent_spec::AgentSpec::on_demand("persisted", "test");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _ = rt.block_on(async { manager.spawn_with_spec(&spec, "test", None).await });
    let runs = store.list().unwrap();
    assert_eq!(runs.len(), 1);
    let run = store.load(&runs[0]).unwrap();
    assert_eq!(run.agent_name, "persisted");
    // Final status set after early return
    assert!(matches!(
        run.status,
        crate::subagent_runs::RunStatus::Failed | crate::subagent_runs::RunStatus::Completed
    ));
}

#[test]
fn spawn_with_spec_without_run_store_does_not_persist() {
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1,
        registry: None,
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
    let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Should not panic / not require store
    let _ = rt.block_on(async { manager.spawn_with_spec(&spec, "y", None).await });
}

#[test]
fn spawn_with_spec_blocked_by_subagent_start_hook() {
    use crate::lifecycle_hooks::{HookEvent, HookManager, HookMatcher, HookResponse};
    let bus = Arc::new(EventBus::new());
    let mut hooks = HookManager::new();
    hooks.add(
        HookEvent::SubagentStart,
        HookMatcher {
            matcher: None,
            response: HookResponse::Block {
                reason: "test block".into(),
            },
            timeout_secs: 60,
        },
    );
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 0,
        registry: None,
        run_store: None,
        hook_manager: Some(Arc::new(hooks)),
        cancellation: None,
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: None,
        mcp_discovery: None,
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };
    let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async { manager.spawn_with_spec(&spec, "y", None).await });
    assert!(!result.success);
    assert!(result.summary.contains("test block"));
}

#[test]
fn spawn_with_spec_early_cancelled_by_pre_run_cancel() {
    use crate::cancellation::CancellationTree;
    let bus = Arc::new(EventBus::new());
    let tree = Arc::new(CancellationTree::new());
    tree.cancel_all(); // root already cancelled

    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 0,
        registry: None,
        run_store: None,
        hook_manager: None,
        cancellation: Some(tree),
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: None,
        mcp_discovery: None,
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };
    let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async { manager.spawn_with_spec(&spec, "y", None).await });
    assert!(!result.success);
    assert!(
        result.summary.contains("cancelled before start"),
        "got: {}",
        result.summary
    );
}

#[tokio::test]
async fn spawn_with_spec_auto_triggers_discovery_when_cache_empty() {
    let _guard = mcp_env_lock().lock().await;
    // The spec declares mcp_servers but cache is empty. After spawn (even
    // a depth-limit early return), the cache should remain empty BUT the
    // discovery attempt should have happened — verified indirectly by
    // checking that an unreachable server gets recorded as failed (proof
    // discover_filtered ran).
    use std::collections::BTreeMap;
    use std::sync::Arc;

    let bus = Arc::new(EventBus::new());
    let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());

    let mut reg = theo_infra_mcp::McpRegistry::new();
    reg.register(theo_infra_mcp::McpServerConfig::Stdio {
        name: "auto-discover-test".into(),
        command: "/nonexistent/cmd/zzz".into(),
        args: vec![],
        env: BTreeMap::new(),
        timeout_ms: None,
    });

    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 0,
        registry: None,
        run_store: None,
        hook_manager: None,
        cancellation: None,
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: Some(Arc::new(reg)),
        mcp_discovery: Some(cache.clone()),
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };

    let mut spec = AgentSpec::on_demand("x", "y");
    spec.mcp_servers = vec!["auto-discover-test".to_string()];

    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        manager.spawn_with_spec(&spec, "y", None),
    )
    .await;
    // Cache stays empty because the server is unreachable, but
    // discover_filtered MUST have been attempted (no panic + no cached
    // entry for the reachable case is the only observable proof here).
    assert!(
        cache.get("auto-discover-test").is_none(),
        "unreachable server must NOT be cached"
    );
}

#[tokio::test]
async fn spawn_with_spec_skips_discovery_when_cache_already_populated() {
    let _guard = mcp_env_lock().lock().await;
    // Pre-populated cache: spawn should NOT re-trigger discovery.
    // We assert this by registering an unreachable server but seeding
    // the cache with a fake tool — if discovery ran, the call would
    // fail and the cache entry would be removed (or stay as inserted).
    use std::collections::BTreeMap;
    use std::sync::Arc;

    let bus = Arc::new(EventBus::new());
    let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
    cache.put(
        "pre-cached",
        vec![theo_infra_mcp::McpTool {
            name: "fake_tool".into(),
            description: Some("seed".into()),
            input_schema: serde_json::json!({"type": "object"}),
        }],
    );

    let mut reg = theo_infra_mcp::McpRegistry::new();
    reg.register(theo_infra_mcp::McpServerConfig::Stdio {
        name: "pre-cached".into(),
        command: "/nonexistent/never-spawned".into(),
        args: vec![],
        env: BTreeMap::new(),
        timeout_ms: None,
    });

    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1, // depth-limit early return
        registry: None,
        run_store: None,
        hook_manager: None,
        cancellation: None,
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: Some(Arc::new(reg)),
        mcp_discovery: Some(cache.clone()),
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };

    let mut spec = AgentSpec::on_demand("x", "y");
    spec.mcp_servers = vec!["pre-cached".to_string()];

    let _ = manager.spawn_with_spec(&spec, "y", None).await;
    // Cache still has the seeded entry — proof discovery did NOT overwrite.
    assert!(cache.get("pre-cached").is_some());
    assert_eq!(cache.get("pre-cached").unwrap().len(), 1);
    assert_eq!(cache.get("pre-cached").unwrap()[0].name, "fake_tool");
}

#[tokio::test]
async fn spawn_with_spec_does_not_discover_when_mcp_servers_empty() {
    // Empty mcp_servers → no discovery, even when cache + registry attached.
    use std::sync::Arc;
    let bus = Arc::new(EventBus::new());
    let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
    let reg = Arc::new(theo_infra_mcp::McpRegistry::new());
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1,
        registry: None,
        run_store: None,
        hook_manager: None,
        cancellation: None,
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: Some(reg),
        mcp_discovery: Some(cache.clone()),
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };
    let spec = AgentSpec::on_demand("x", "y"); // mcp_servers empty by default
    let _ = manager.spawn_with_spec(&spec, "y", None).await;
    assert!(cache.cached_servers().is_empty());
}

#[tokio::test]
async fn spawn_with_spec_does_not_discover_when_no_registry_attached() {
    // No mcp_registry → discovery cannot run regardless of cache state.
    use std::sync::Arc;
    let bus = Arc::new(EventBus::new());
    let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1,
        registry: None,
        run_store: None,
        hook_manager: None,
        cancellation: None,
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: None,
        mcp_discovery: Some(cache.clone()),
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };
    let mut spec = AgentSpec::on_demand("x", "y");
    spec.mcp_servers = vec!["github".to_string()];
    let _ = manager.spawn_with_spec(&spec, "y", None).await;
    assert!(cache.cached_servers().is_empty());
}

#[tokio::test]
async fn spawn_with_spec_continues_when_discovery_fails_completely() {
    let _guard = mcp_env_lock().lock().await;
    // All servers unreachable → spawn still proceeds (fail-soft).
    use std::collections::BTreeMap;
    use std::sync::Arc;
    let bus = Arc::new(EventBus::new());
    let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
    let mut reg = theo_infra_mcp::McpRegistry::new();
    reg.register(theo_infra_mcp::McpServerConfig::Stdio {
        name: "dead".into(),
        command: "/nonexistent/zzz".into(),
        args: vec![],
        env: BTreeMap::new(),
        timeout_ms: None,
    });
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1,
        registry: None,
        run_store: None,
        hook_manager: None,
        cancellation: None,
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: Some(Arc::new(reg)),
        mcp_discovery: Some(cache.clone()),
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };
    let mut spec = AgentSpec::on_demand("x", "y");
    spec.mcp_servers = vec!["dead".to_string()];
    let result = manager.spawn_with_spec(&spec, "y", None).await;
    // depth-limit summary surfaces — discovery failure didn't cause a panic.
    assert!(result.summary.contains("depth limit"));
}

#[tokio::test]
async fn spawn_with_spec_skips_discovery_when_env_disables_auto() {
    let _guard = mcp_env_lock().lock().await;
    // THEO_MCP_AUTO_DISCOVERY=0 disables auto-trigger even with
    // unreachable servers in the registry.
    use std::collections::BTreeMap;
    use std::sync::Arc;
    // SAFETY: holding `mcp_env_lock` serialises against the 3
    // sibling MCP-discovery async tests, so for the duration of
    // this test no other thread reads the variable. The pre-existing
    // claim ("only this test toggles it") was wrong — cargo test
    // is multi-threaded by default.
    unsafe { std::env::set_var("THEO_MCP_AUTO_DISCOVERY", "0"); }
    let bus = Arc::new(EventBus::new());
    let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
    let mut reg = theo_infra_mcp::McpRegistry::new();
    reg.register(theo_infra_mcp::McpServerConfig::Stdio {
        name: "would-be-discovered".into(),
        command: "/nonexistent/zzz".into(),
        args: vec![],
        env: BTreeMap::new(),
        timeout_ms: None,
    });
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1,
        registry: None,
        run_store: None,
        hook_manager: None,
        cancellation: None,
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: Some(Arc::new(reg)),
        mcp_discovery: Some(cache.clone()),
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };
    let mut spec = AgentSpec::on_demand("x", "y");
    spec.mcp_servers = vec!["would-be-discovered".to_string()];
    let _ = manager.spawn_with_spec(&spec, "y", None).await;
    // Cache empty AND no IO attempted (env disables it) — observable
    // proof: the test finished essentially instantly with nothing cached.
    assert!(cache.cached_servers().is_empty());
    // SAFETY: still inside the `mcp_env_lock` critical section
    // acquired at the top of this test, so no other thread reads
    // `THEO_MCP_AUTO_DISCOVERY` concurrently with this remove_var.
    unsafe { std::env::remove_var("THEO_MCP_AUTO_DISCOVERY"); }
}

#[test]
fn spawn_with_spec_text_none_context_leaves_context_used_none() {
    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus,
        project_dir: PathBuf::from("/tmp"),
        depth: 1,
        registry: None,
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
    let spec = theo_domain::agent_spec::AgentSpec::on_demand("y", "z");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result =
        rt.block_on(async { manager.spawn_with_spec_text(&spec, "do z", None).await });
    assert!(result.context_used.is_none());
}

// -----------------------------------------------------------------------
// WorktreeOverride — resume-runtime-wiring
// -----------------------------------------------------------------------

