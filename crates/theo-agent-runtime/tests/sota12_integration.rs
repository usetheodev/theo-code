//! Integration test SOTA-12 — validates all 5 sota-gaps-plan.md phases
//! active simultaneously:
//!
//! - Phase 14: cost-aware routing (ComplexityClassifier + AutomaticModelRouter)
//! - Phase 15: dashboard per-agent endpoints (use case smoke)
//! - Phase 16: resume resilience (Resumer + ResumeContext)
//! - Phase 17: MCP discovery cache pre-population
//! - Phase 18: handoff guardrails 3-tier (built-ins + custom + PreHandoff hook)
//!
//! Goal: prove the features compose without surprising interactions, NOT
//! to test each feature in isolation (those tests live in their own modules).

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use theo_agent_runtime::config::AgentConfig;
use theo_agent_runtime::event_bus::EventBus;
use theo_agent_runtime::handoff_guardrail::{
    GuardrailChain, GuardrailDecision, HandoffContext, HandoffGuardrail,
};
use theo_agent_runtime::subagent::{
    builtins, Resumer, SubAgentManager, SubAgentRegistry,
};
use theo_agent_runtime::subagent_runs::{FileSubagentRunStore, RunStatus, SubagentRun};
use theo_domain::agent_spec::AgentSpec;
use theo_domain::routing::{ComplexityTier, RoutingContext, RoutingPhase};
use theo_infra_llm::routing::auto::AutomaticModelRouter;
use theo_infra_llm::routing::complexity::{ComplexityClassifier, TaskType};
use theo_infra_llm::routing::config::RoutingConfig;
use theo_infra_llm::routing::rules::RuleBasedRouter;
use theo_infra_mcp::{DiscoveryCache, McpServerConfig, McpTool};
use theo_domain::routing::ModelRouter;

// ── Phase 14: cost-aware routing ──

#[test]
fn phase14_complexity_classifier_routes_planning_to_strong() {
    use theo_domain::routing::ComplexityTier;
    let task = ComplexityClassifier::detect_task_type("plan the auth refactor");
    assert_eq!(task, TaskType::Planning);
    let signals = theo_infra_llm::routing::complexity::ComplexitySignals {
        task_type: TaskType::Planning,
        ..Default::default()
    };
    assert_eq!(
        ComplexityClassifier::classify(&signals),
        ComplexityTier::Strong
    );
}

#[test]
fn phase14_automatic_router_classifies_when_no_override() {
    let toml = r#"
        [routing]
        enabled = true
        strategy = "rules"
        [routing.slots.cheap]
        model = "haiku"
        provider = "anthropic"
        [routing.slots.default]
        model = "sonnet"
        provider = "anthropic"
        [routing.slots.strong]
        model = "opus"
        provider = "anthropic"
    "#;
    #[derive(serde::Deserialize)]
    struct W { routing: RoutingConfig }
    let w: W = toml::from_str(toml).unwrap();
    let inner = RuleBasedRouter::new(w.routing.to_pricing_table());
    let auto = AutomaticModelRouter::new(inner, true);

    // Retrieval objective → cheap
    let mut ctx = RoutingContext::new(RoutingPhase::Normal);
    ctx.latest_user_message = Some("read Cargo.toml");
    assert_eq!(auto.route(&ctx).model_id, "haiku");

    // Analysis → strong
    ctx.latest_user_message = Some("audit the security boundary");
    assert_eq!(auto.route(&ctx).model_id, "opus");
}

// ── Phase 15: dashboard per-agent ──
// (Lives in theo-application/tests/sota12_dashboard.rs because of ADR-016
// dependency direction — theo-agent-runtime cannot depend on theo-application.)
//
// Sanity check here: SubagentRun records aggregate by agent_name correctly
// in the persistence layer, which is what the dashboard reads.
#[test]
fn phase15_subagent_runs_index_by_agent_name() {
    let dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(dir.path().join(".theo").join("subagent"));

    let spec = builtins::explorer();
    for i in 0..3 {
        let mut run = SubagentRun::new_running(
            &format!("r-{}", i),
            None,
            &spec,
            "obj",
            "/tmp",
            None,
        );
        run.status = if i < 2 { RunStatus::Completed } else { RunStatus::Failed };
        run.tokens_used = 100 * (i as u64 + 1);
        run.iterations_used = (i as usize) + 1;
        run.started_at = i as i64;
        store.save(&run).unwrap();
    }

    let ids = store.list().unwrap();
    assert_eq!(ids.len(), 3);
    let runs: Vec<SubagentRun> = ids.into_iter().filter_map(|id| store.load(&id).ok()).collect();
    assert!(runs.iter().all(|r| r.agent_name == "explorer"));
    let succ = runs.iter().filter(|r| r.status == RunStatus::Completed).count();
    let fail = runs.iter().filter(|r| r.status == RunStatus::Failed).count();
    assert_eq!(succ, 2);
    assert_eq!(fail, 1);
}

// ── Phase 16: resume ──

#[tokio::test]
async fn phase16_resume_terminal_run_is_rejected() {
    let dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(dir.path());
    let spec = builtins::explorer();
    let mut run = SubagentRun::new_running("r-done", None, &spec, "obj", "/tmp", None);
    run.status = RunStatus::Completed;
    store.save(&run).unwrap();

    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager::with_registry(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
        Arc::new(SubAgentRegistry::with_builtins()),
    );
    let resumer = Resumer::new(&store, &manager);
    let err = resumer.resume("r-done").await.unwrap_err();
    assert!(format!("{}", err).contains("terminal"));
}

#[test]
fn phase16_resume_running_returns_context_with_spec_snapshot() {
    let dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(dir.path());
    let spec = builtins::implementer();
    let mut run = SubagentRun::new_running("r-live", None, &spec, "obj", "/tmp", None);
    run.tokens_used = 7777;
    store.save(&run).unwrap();

    let bus = Arc::new(EventBus::new());
    let manager = SubAgentManager::with_registry(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
        Arc::new(SubAgentRegistry::with_builtins()),
    );
    let resumer = Resumer::new(&store, &manager);
    let ctx = resumer.build_context("r-live").unwrap();
    assert_eq!(ctx.spec.name, "implementer");
    assert_eq!(ctx.prior_tokens_used, 7777);
}

// ── Phase 17: MCP discovery cache + tool injection ──

#[test]
fn phase17_mcp_tool_adapter_appears_in_tool_bridge_definitions() {
    // Plan §17 DOD: tools added to LLM tool array with prefix mcp:server:tool.
    // Verifies end-to-end:
    //   1. Build a ToolRegistry seeded with default tools.
    //   2. Register an McpToolAdapter for a "discovered" tool.
    //   3. tool_bridge::registry_to_definitions() exposes it with the
    //      qualified name AND the raw inputSchema (not the empty placeholder).
    use std::sync::Arc;
    use theo_agent_runtime::subagent::McpToolAdapter;
    use theo_agent_runtime::tool_bridge::registry_to_definitions;
    use theo_infra_mcp::{McpDispatcher, McpRegistry, McpTool};

    let mut registry = theo_tooling::registry::create_default_registry();
    let raw_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "search term"}
        },
        "required": ["query"]
    });
    let tool = McpTool {
        name: "search_code".into(),
        description: Some("Search GitHub repositories".into()),
        input_schema: raw_schema.clone(),
    };
    let dispatcher = Arc::new(McpDispatcher::new(Arc::new(McpRegistry::new())));
    let adapter = McpToolAdapter::new("github", &tool, dispatcher);
    registry
        .register(Box::new(adapter))
        .expect("adapter must register cleanly");

    let defs = registry_to_definitions(&registry);
    let mcp_def = defs
        .iter()
        .find(|d| d.function.name == "mcp:github:search_code")
        .expect("MCP tool must appear in the LLM-side tool definitions");
    // The raw schema (with `required`) survives — verifying that
    // llm_schema_override defeats the empty-ToolSchema fallback.
    let params = &mcp_def.function.parameters;
    assert_eq!(params, &raw_schema);
    assert_eq!(
        params.get("required").and_then(|v| v.as_array()).map(|a| a.len()),
        Some(1)
    );
    assert!(mcp_def.function.description.contains("GitHub"));
}



#[test]
fn phase17_discovery_cache_renders_concrete_tool_names_in_hint() {
    let cache = DiscoveryCache::new();
    cache.put(
        "github",
        vec![
            McpTool {
                name: "list_repos".into(),
                description: Some("List the user's repos".into()),
                input_schema: serde_json::json!({"type":"object"}),
            },
            McpTool {
                name: "search_code".into(),
                description: Some("Code search".into()),
                input_schema: serde_json::json!({"type":"object"}),
            },
        ],
    );
    let hint = cache.render_prompt_hint(&["github".to_string()]);
    assert!(hint.contains("`mcp:github:list_repos`"));
    assert!(hint.contains("`mcp:github:search_code`"));
    assert!(hint.contains("pre-discovered"));
}

#[tokio::test]
async fn phase17_discover_all_unreachable_server_records_failure_softly() {
    use std::collections::BTreeMap;
    use theo_infra_mcp::McpRegistry;
    use std::time::Duration;

    let cache = DiscoveryCache::new();
    let mut reg = McpRegistry::new();
    reg.register(McpServerConfig::Stdio {
        name: "ghost".into(),
        command: "/nonexistent/command/zzz".into(),
        args: vec![],
        env: BTreeMap::new(),
    });
    let report = cache.discover_all(&reg, Duration::from_secs(1)).await;
    assert_eq!(report.successful.len(), 0);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, "ghost");
    assert!(cache.get("ghost").is_none(), "fail-soft: not cached");
}

// ── Phase 18: handoff guardrails ──

#[test]
fn phase18_default_chain_has_two_builtins() {
    let chain = GuardrailChain::with_default_builtins();
    assert_eq!(chain.len(), 2);
    let ids = chain.ids();
    assert!(ids.iter().any(|i| i == "builtin.read_only_agent_must_not_mutate"));
    assert!(ids.iter().any(|i| i == "builtin.objective_must_not_be_empty"));
}

#[test]
fn phase18_explorer_implementing_redirects_to_implementer() {
    // Plan §18 default behavior: built-in `ReadOnlyAgentMustNotMutate`
    // redirects (instead of blocking) so the LLM's intent survives.
    let chain = GuardrailChain::with_default_builtins();
    let target = builtins::explorer();
    let ctx = HandoffContext {
        source_agent: "main",
        target_agent: &target.name,
        target_spec: &target,
        objective: "implement caching layer",
        source_capabilities: None,
    };
    let (id, decision) = chain.first_decision(&ctx).expect("must decide");
    assert_eq!(id, "builtin.read_only_agent_must_not_mutate");
    assert!(decision.is_redirect(), "expected Redirect, got {:?}", decision);
    if let GuardrailDecision::Redirect { new_agent_name } = decision {
        assert_eq!(new_agent_name, "implementer");
    }
}

#[test]
fn phase18_custom_guardrail_runs_after_builtins() {
    #[derive(Debug)]
    struct DenyAll;
    impl HandoffGuardrail for DenyAll {
        fn id(&self) -> &str { "project.deny_all" }
        fn evaluate(&self, _ctx: &HandoffContext<'_>) -> GuardrailDecision {
            GuardrailDecision::Block { reason: "policy".into() }
        }
    }

    let mut chain = GuardrailChain::with_default_builtins();
    chain.add(Arc::new(DenyAll));
    let target = builtins::implementer();
    let ctx = HandoffContext {
        source_agent: "main",
        target_agent: &target.name,
        target_spec: &target,
        objective: "implement foo",  // builtins allow this
        source_capabilities: None,
    };
    // Builtins allow → custom guardrail blocks
    let (id, _) = chain.first_block(&ctx).expect("custom blocks");
    assert_eq!(id, "project.deny_all");
}

// ── End-to-end composition ──

#[test]
fn sota12_all_features_can_be_constructed_together_without_panic() {
    // Sanity: the 5 features compose into a single SubAgentManager + chain.
    let bus = Arc::new(EventBus::new());

    let dir = TempDir::new().unwrap();
    let store = Arc::new(FileSubagentRunStore::new(
        dir.path().join(".theo").join("subagent"),
    ));

    let mcp_cache = Arc::new(DiscoveryCache::new());
    mcp_cache.put(
        "github",
        vec![McpTool {
            name: "search_code".into(),
            description: Some("find code".into()),
            input_schema: serde_json::json!({}),
        }],
    );

    let manager = SubAgentManager::with_registry(
        AgentConfig::default(),
        bus,
        PathBuf::from("/tmp"),
        Arc::new(SubAgentRegistry::with_builtins()),
    )
    .with_run_store(store)
    .with_mcp_discovery(mcp_cache);

    assert!(manager.run_store().is_some());
    assert!(manager.mcp_discovery().is_some());
    assert!(manager.registry().is_some());

    // Independently: a guardrail chain with project guardrail.
    let chain = GuardrailChain::with_default_builtins();
    assert!(!chain.is_empty());

    // And: a discovery render works with the cache from above.
    let hint = manager
        .mcp_discovery()
        .unwrap()
        .render_prompt_hint(&["github".to_string()]);
    assert!(hint.contains("`mcp:github:search_code`"));
}

#[test]
fn sota12_explorer_redirect_decision_does_not_touch_persistence() {
    // Plan §18: the ReadOnly guardrail now redirects rather than blocks.
    // Either way, evaluating the chain alone (without invoking spawn) must
    // never touch the persistence store.
    let dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(dir.path().join(".theo").join("subagent"));

    let chain = GuardrailChain::with_default_builtins();
    let target: AgentSpec = builtins::explorer();
    let ctx = HandoffContext {
        source_agent: "main",
        target_agent: &target.name,
        target_spec: &target,
        objective: "implement security fixes",
        source_capabilities: None,
    };
    let (id, decision) = chain
        .first_decision(&ctx)
        .expect("must produce a decision (redirect) for read-only target asked to mutate");
    assert_eq!(id, "builtin.read_only_agent_must_not_mutate");
    assert!(decision.is_redirect(), "expected Redirect, got {:?}", decision);

    // Pure evaluation: store untouched.
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn sota12_complexity_signals_drive_routing_for_real_tasks() {
    // Validates: planning task → Strong tier, retrieval → Cheap.
    let cases = [
        ("plan the migration", ComplexityTier::Strong),
        ("read Cargo.toml and list crates", ComplexityTier::Cheap),
        ("audit security boundary", ComplexityTier::Strong),
        ("implement small fix", ComplexityTier::Cheap),
    ];
    for (objective, expected) in cases {
        let task_type = ComplexityClassifier::detect_task_type(objective);
        let signals = theo_infra_llm::routing::complexity::ComplexitySignals {
            task_type,
            objective_tokens: (objective.len() / 4) as u32,
            ..Default::default()
        };
        let actual = ComplexityClassifier::classify(&signals);
        assert_eq!(
            actual, expected,
            "objective '{}' (task={:?}) expected {:?}, got {:?}",
            objective, task_type, expected, actual
        );
    }
}
