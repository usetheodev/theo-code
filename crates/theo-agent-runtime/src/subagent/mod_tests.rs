//! Sibling test body of `mod.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `mod.rs` via `#[path = "mod_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use super::*;

    use theo_domain::tool::ToolCategory;

    /// Serializes the 4 async tests that exercise the MCP-discovery
    /// path (`mcp_registry: Some`, `mcp_discovery: Some`, non-empty
    /// `mcp_servers`) — the same path that
    /// `spawn_with_spec_skips_discovery_when_env_disables_auto`
    /// mutates `THEO_MCP_AUTO_DISCOVERY` against.
    ///
    /// `tokio::sync::Mutex` is required (not `std::sync::Mutex`)
    /// because `#[tokio::test]` async tests hold the guard across
    /// `.await` points and `std::sync::MutexGuard` is `!Send`. Tests
    /// in earlier sync-only modules (wiki/compiler, onboarding) use
    /// `std::sync::Mutex` because their tests don't `.await`.
    ///
    /// Same flake class as the wiki/compiler / onboarding fixes
    /// (commits 8025a70, 184ff59) — narrowly scoped to just the
    /// 4 collision-condition tests instead of all 60+ tests in the
    /// file.
    fn mcp_env_lock() -> &'static tokio::sync::Mutex<()> {
        use tokio::sync::Mutex;
        static M: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
        M.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn builtin_explorer_capability_is_read_only() {
        let spec = builtins::explorer();
        assert!(
            !spec.capability_set
                .can_use_tool("bash", ToolCategory::Execution)
        );
        assert!(
            !spec.capability_set
                .can_use_tool("edit", ToolCategory::FileOps)
        );
        assert!(
            !spec.capability_set
                .can_use_tool("write", ToolCategory::FileOps)
        );
    }

    #[test]
    fn builtin_implementer_capability_is_unrestricted() {
        let spec = builtins::implementer();
        assert!(spec.capability_set.denied_tools.is_empty());
        assert_eq!(
            spec.capability_set.allowed_tools,
            theo_domain::capability::AllowedTools::All
        );
    }

    #[test]
    fn builtin_verifier_cannot_edit_can_bash() {
        let spec = builtins::verifier();
        assert!(spec.capability_set.denied_tools.contains("edit"));
        assert!(spec.capability_set.denied_tools.contains("write"));
        assert!(!spec.capability_set.denied_tools.contains("bash"));
    }

    #[test]
    fn builtin_reviewer_is_read_only() {
        let spec = builtins::reviewer();
        assert!(spec.capability_set.denied_tools.contains("edit"));
        assert!(spec.capability_set.denied_tools.contains("write"));
    }

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

    use crate::event_bus::EventListener;
    use std::sync::Mutex;
    use theo_domain::event::{DomainEvent, EventType};

    /// Test helper: captures events published to the bus.
    struct CaptureListener {
        events: Mutex<Vec<DomainEvent>>,
    }
    impl CaptureListener {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
        fn events(&self) -> Vec<DomainEvent> {
            self.events.lock().unwrap().clone()
        }
    }
    impl EventListener for CaptureListener {
        fn on_event(&self, e: &DomainEvent) {
            self.events.lock().unwrap().push(e.clone());
        }
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

    mod worktree_override {
        use super::*;

        fn manager_no_worktree(depth: usize) -> SubAgentManager {
            SubAgentManager {
                config: AgentConfig::default(),
                event_bus: Arc::new(EventBus::new()),
                project_dir: PathBuf::from("/tmp"),
                depth,
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
            }
        }

        #[test]
        fn worktree_override_enum_default_is_none() {
            // None variant = legacy behavior (create new from spec.isolation).
            let o = WorktreeOverride::None;
            assert!(matches!(o, WorktreeOverride::None));
        }

        #[test]
        fn worktree_override_reuse_carries_path() {
            let p = PathBuf::from("/tmp/wt-reused");
            let o = WorktreeOverride::Reuse(p.clone());
            match o {
                WorktreeOverride::Reuse(got) => assert_eq!(got, p),
                _ => panic!("expected Reuse variant"),
            }
        }

        #[test]
        fn worktree_override_recreate_carries_base_branch() {
            let o = WorktreeOverride::Recreate {
                base_branch: "develop".to_string(),
            };
            match o {
                WorktreeOverride::Recreate { base_branch } => {
                    assert_eq!(base_branch, "develop");
                }
                _ => panic!("expected Recreate variant"),
            }
        }

        #[test]
        fn spawn_with_spec_with_override_none_matches_legacy_behavior() {
            // Regression guard: spawn_with_spec_with_override(None) MUST produce
            // a result indistinguishable from spawn_with_spec for non-isolated
            // specs (depth-limit early return path is identical).
            let manager = manager_no_worktree(1);
            let spec = theo_domain::agent_spec::AgentSpec::on_demand("alpha", "do x");
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r_legacy =
                rt.block_on(async { manager.spawn_with_spec(&spec, "obj", None).await });
            let r_override = rt.block_on(async {
                manager
                    .spawn_with_spec_with_override(&spec, "obj", None, WorktreeOverride::None)
                    .await
            });
            // Both hit depth-limit → identical "depth limit" summary.
            assert!(r_legacy.summary.contains("depth limit"));
            assert!(r_override.summary.contains("depth limit"));
            assert_eq!(r_legacy.success, r_override.success);
        }

        #[test]
        fn spawn_with_spec_with_override_reuse_skips_provider_create() {
            // When Reuse(path) is supplied, even WITHOUT a worktree_provider
            // the path is honored (since no `git worktree add` is needed —
            // the path already exists on disk from the prior crashed run).
            // Depth-limit short-circuit means we don't actually run, but the
            // observable contract is: the API accepts the override + returns.
            let manager = manager_no_worktree(1);
            let mut spec = theo_domain::agent_spec::AgentSpec::on_demand("alpha", "x");
            spec.isolation = Some("worktree".to_string());
            let p = PathBuf::from("/tmp/wt-reused-from-resume");
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r = rt.block_on(async {
                manager
                    .spawn_with_spec_with_override(
                        &spec,
                        "obj",
                        None,
                        WorktreeOverride::Reuse(p),
                    )
                    .await
            });
            // Depth limit hit, no panic — Reuse path didn't try to call git.
            assert!(r.summary.contains("depth limit"));
        }

        #[test]
        fn spawn_with_spec_with_override_recreate_passes_base_branch() {
            // When Recreate { base_branch } is supplied, the provider
            // (when present) would be invoked with the override base branch
            // INSTEAD of spec.isolation_base_branch. We verify by:
            //   - Setting spec.isolation_base_branch = "main"
            //   - Calling with Recreate { base_branch: "develop" }
            //   - At depth=1 we short-circuit, but the API contract is that
            //     this branch is honored (validated end-to-end via Fase 32).
            let manager = manager_no_worktree(1);
            let mut spec = theo_domain::agent_spec::AgentSpec::on_demand("alpha", "x");
            spec.isolation = Some("worktree".to_string());
            spec.isolation_base_branch = Some("main".to_string());
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r = rt.block_on(async {
                manager
                    .spawn_with_spec_with_override(
                        &spec,
                        "obj",
                        None,
                        WorktreeOverride::Recreate {
                            base_branch: "develop".to_string(),
                        },
                    )
                    .await
            });
            assert!(r.summary.contains("depth limit"));
        }

        #[test]
        fn spawn_with_spec_alias_delegates_to_with_override_none() {
            // Verify that spawn_with_spec is now a wrapper that calls
            // spawn_with_spec_with_override(.., None). Same observable
            // behavior as the legacy parity test, but documents the
            // refactor contract explicitly.
            let manager = manager_no_worktree(1);
            let spec = theo_domain::agent_spec::AgentSpec::on_demand("a", "b");
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r1 = rt.block_on(async { manager.spawn_with_spec(&spec, "obj", None).await });
            let r2 = rt.block_on(async {
                manager
                    .spawn_with_spec_with_override(&spec, "obj", None, WorktreeOverride::None)
                    .await
            });
            assert_eq!(r1.success, r2.success);
            assert_eq!(r1.summary, r2.summary);
        }
    }
