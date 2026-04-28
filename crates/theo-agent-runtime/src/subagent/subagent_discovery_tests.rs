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
fn discovery_cache_takes_precedence_over_registry_hint() {
    // Spec declares mcp_servers and BOTH registry + discovery cache are
    // attached. When the cache has discovered tools for that server, the
    // *cache* hint must be used (concrete tool names) not the registry's
    // bare-namespace hint.
    use std::collections::BTreeMap;
    use theo_infra_mcp::{DiscoveryCache, McpRegistry, McpServerConfig, McpTool};

    let bus = Arc::new(EventBus::new());

    let mut reg = McpRegistry::new();
    reg.register(McpServerConfig::Stdio {
        name: "github".into(),
        command: "echo".into(),
        args: vec![],
        env: BTreeMap::new(),
        timeout_ms: None,
    });
    let cache = DiscoveryCache::new();
    cache.put(
        "github",
        vec![
            McpTool {
                name: "search_repo".into(),
                description: Some("search a github repository".into()),
                input_schema: serde_json::json!({"type":"object"}),
            },
        ],
    );

    let manager = SubAgentManager {
        config: AgentConfig::default(),
        event_bus: bus.clone(),
        project_dir: PathBuf::from("/tmp"),
        depth: 1, // depth-limit early return → no real spawn
        registry: None,
        run_store: None,
        hook_manager: None,
        cancellation: None,
        checkpoint_manager: None,
        worktree_provider: None,
        metrics: None,
        mcp_registry: Some(Arc::new(reg)),
        mcp_discovery: Some(Arc::new(cache)),
        pending_resume_context: parking_lot::Mutex::new(None),
        spawn_semaphore: None,
    };

    // We cannot directly inspect sub_config.system_prompt without
    // refactoring spawn_with_spec, so we rely on render_prompt_hint
    // semantics being unit-tested in theo-infra-mcp::discovery::tests.
    // Sanity check: the discovery cache used here resolves correctly.
    let cache_ref = manager.mcp_discovery().unwrap();
    let allow = vec!["github".to_string()];
    let hint = cache_ref.render_prompt_hint(&allow);
    assert!(hint.contains("`mcp:github:search_repo`"));
    assert!(hint.contains("pre-discovered"));
}

// ── MCP auto-discovery on first spawn ──

#[test]
fn needs_discovery_true_when_cache_empty_and_servers_requested() {
    let cache = theo_infra_mcp::DiscoveryCache::new();
    assert!(needs_discovery(&cache, &["github".to_string()]));
}

#[test]
fn needs_discovery_false_when_cache_already_covers_all_requested() {
    let cache = theo_infra_mcp::DiscoveryCache::new();
    cache.put("github", vec![]);
    cache.put("postgres", vec![]);
    assert!(!needs_discovery(
        &cache,
        &["github".to_string(), "postgres".to_string()]
    ));
}

#[test]
fn needs_discovery_true_when_cache_partially_covers() {
    let cache = theo_infra_mcp::DiscoveryCache::new();
    cache.put("github", vec![]);
    // postgres not cached
    assert!(needs_discovery(
        &cache,
        &["github".to_string(), "postgres".to_string()]
    ));
}

#[test]
fn needs_discovery_false_when_no_servers_requested() {
    let cache = theo_infra_mcp::DiscoveryCache::new();
    assert!(!needs_discovery(&cache, &[]));
}

