//! R4 acceptance tests — compaction phase, subagent slots, TOML config.
//! Plan: `outputs/smart-model-routing-plan.md` §2 R4 table.

use std::sync::{Arc, Mutex};

use theo_agent_runtime::config::RouterHandle;
use theo_agent_runtime::subagent::builtins;
use theo_domain::routing::{
    ModelChoice, ModelRouter, RoutingContext, RoutingFailureHint, RoutingPhase, SubAgentRoleId,
};
use theo_infra_llm::routing::{RoutingConfig, RuleBasedRouter};

/// RecordingRouter captures the phase it observes on each call.
struct RecordingRouter {
    inner: RuleBasedRouter,
    phases: Arc<Mutex<Vec<RoutingPhase>>>,
}

impl RecordingRouter {
    fn new(inner: RuleBasedRouter) -> (Self, Arc<Mutex<Vec<RoutingPhase>>>) {
        let phases = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                inner,
                phases: phases.clone(),
            },
            phases,
        )
    }
}

impl ModelRouter for RecordingRouter {
    fn route(&self, ctx: &RoutingContext<'_>) -> ModelChoice {
        self.phases.lock().unwrap().push(ctx.phase.clone());
        self.inner.route(ctx)
    }
    fn fallback(
        &self,
        previous: &ModelChoice,
        hint: RoutingFailureHint,
    ) -> Option<ModelChoice> {
        self.inner.fallback(previous, hint)
    }
}

fn full_router() -> RecordingRouter {
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

        [routing.slots.compact]
        model = "haiku"
        provider = "anthropic"
        max_output_tokens = 2048

        [routing.slots.vision]
        model = "sonnet-vl"
        provider = "anthropic"

        [routing.slots.subagent_explorer]
        model = "haiku-explorer"
        provider = "anthropic"

        [routing.slots.subagent_implementer]
        model = "sonnet-implementer"
        provider = "anthropic"
    "#;

    #[derive(serde::Deserialize)]
    struct Wrapper {
        routing: RoutingConfig,
    }
    let wrapper: Wrapper = toml::from_str(toml).unwrap();
    let table = wrapper.routing.to_pricing_table();
    let rules = RuleBasedRouter::new(table);
    RecordingRouter::new(rules).0
}

// ── R4-AC-1 ──────────────────────────────────────────────────────────
#[test]
fn test_r4_ac_1_compaction_uses_routing_phase_compaction() {
    let toml = r#"
        [routing]
        enabled = true
        strategy = "rules"
        [routing.slots.default]
        model = "sonnet"
        provider = "anthropic"
        [routing.slots.compact]
        model = "haiku"
        provider = "anthropic"
    "#;
    #[derive(serde::Deserialize)]
    struct W {
        routing: RoutingConfig,
    }
    let w: W = toml::from_str(toml).unwrap();
    let rules = RuleBasedRouter::new(w.routing.to_pricing_table());
    let (router, phases) = RecordingRouter::new(rules);

    // Caller uses RoutingPhase::Compaction when compaction starts.
    let ctx = RoutingContext::new(RoutingPhase::Compaction);
    let choice = router.route(&ctx);
    assert_eq!(choice.model_id, "haiku");
    let seen = phases.lock().unwrap();
    assert!(matches!(seen[0], RoutingPhase::Compaction));
}

// ── R4-AC-2 ──────────────────────────────────────────────────────────
#[test]
fn test_r4_ac_2_subagent_explorer_routes_to_explorer_slot() {
    let router = full_router();
    let ctx = RoutingContext::new(RoutingPhase::subagent(SubAgentRoleId::EXPLORER));
    let choice = router.route(&ctx);
    assert_eq!(choice.model_id, "haiku-explorer");
}

// ── R4-AC-3 ──────────────────────────────────────────────────────────
#[test]
fn test_r4_ac_3_subagent_missing_slot_falls_back_to_default() {
    let router = full_router();
    // Verifier has no configured slot in full_router().
    let ctx = RoutingContext::new(RoutingPhase::subagent(SubAgentRoleId::VERIFIER));
    let choice = router.route(&ctx);
    assert_eq!(
        choice.model_id, "sonnet",
        "missing slot must fall back to `default`"
    );
}

// ── R4-AC-4 ──────────────────────────────────────────────────────────
// "Compaction quality preserved under cheap model" — we validate here
// that the compact slot's choice carries the right coaching reason and
// token budget so the downstream compaction stage gets a usable call.
#[test]
fn test_r4_ac_4_compaction_carries_expected_budget_and_reason() {
    let router = full_router();
    let ctx = RoutingContext::new(RoutingPhase::Compaction);
    let choice = router.route(&ctx);
    assert_eq!(choice.max_output_tokens, 2048);
    assert_eq!(choice.routing_reason, "phase_compaction");
}

// ── R4-AC-5/6/7/8 are covered in theo-infra-llm::routing::config::tests.
// Here we add an end-to-end check that a RouterHandle built from the
// TOML config integrates with AgentConfig.

#[test]
fn test_r4_routing_config_flows_end_to_end() {
    let toml = r#"
        [routing]
        enabled = true
        strategy = "rules"
        [routing.slots.default]
        model = "sonnet"
        provider = "anthropic"
        [routing.slots.cheap]
        model = "haiku"
        provider = "anthropic"
    "#;
    #[derive(serde::Deserialize)]
    struct W {
        routing: RoutingConfig,
    }
    let w: W = toml::from_str(toml).unwrap();
    let router = RuleBasedRouter::new(w.routing.to_pricing_table());
    let handle = RouterHandle::new(Arc::new(router));

    // Simple prompt -> cheap; complex prompt -> default.
    let mut simple = RoutingContext::new(RoutingPhase::Normal);
    simple.latest_user_message = Some("list files");
    assert_eq!(handle.as_router().route(&simple).model_id, "haiku");

    let mut complex = RoutingContext::new(RoutingPhase::Normal);
    complex.latest_user_message = Some("refactor the agent loop");
    assert_eq!(handle.as_router().route(&complex).model_id, "sonnet");
}

#[test]
fn agent_spec_role_id_matches_slot_name_suffix() {
    // Proves the contract: router slots are `subagent_<role_id>` and
    // AgentSpec::role_id() returns the spec name as the role_id.
    assert_eq!(builtins::explorer().role_id(), SubAgentRoleId::EXPLORER);
    assert_eq!(builtins::implementer().role_id(), SubAgentRoleId::IMPLEMENTER);
    assert_eq!(builtins::verifier().role_id(), SubAgentRoleId::VERIFIER);
    assert_eq!(builtins::reviewer().role_id(), SubAgentRoleId::REVIEWER);
}

#[test]
fn config_example_file_exists_and_has_routing_block() {
    let example = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.theo/config.toml.example"
    );
    let source = std::fs::read_to_string(example).expect("example file must exist");
    assert!(source.contains("[routing]"));
    assert!(source.contains("routing.slots.cheap"));
    assert!(source.contains("routing.slots.compact"));
    assert!(source.contains("subagent_explorer"));
}
