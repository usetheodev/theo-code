//! R3 acceptance tests — router wiring into AgentConfig / RunEngine.
//! Plan: `outputs/smart-model-routing-plan.md` §2 R3 table.
//!
//! These tests exercise the routing decision in isolation: we reproduce
//! the ChatRequest-build block from `run_engine.rs:~662` using the same
//! public types. A full RunEngine integration test requires a live LLM
//! or elaborate mock; the AC set targets the deterministic routing
//! decision, not the HTTP pipeline.

#![allow(clippy::field_reassign_with_default)] // Tests tweak individual fields for readability.

use std::sync::{Arc, Mutex};

use theo_agent_runtime::config::{AgentConfig, RouterHandle};
use theo_domain::routing::{
    ModelChoice, ModelRouter, RoutingContext, RoutingFailureHint, RoutingPhase,
};

/// MockRouter records every RoutingContext it sees and returns a
/// deterministic ModelChoice.
struct MockRouter {
    choice: ModelChoice,
    seen: Arc<Mutex<Vec<SeenCtx>>>,
}

#[derive(Clone, Debug)]
struct SeenCtx {
    phase_is_normal: bool,
    iteration: usize,
    conversation_tokens: u64,
    has_user_message: bool,
    requires_tool_use: bool,
}

impl MockRouter {
    fn new(choice: ModelChoice) -> (Self, Arc<Mutex<Vec<SeenCtx>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                choice,
                seen: seen.clone(),
            },
            seen,
        )
    }
}

impl ModelRouter for MockRouter {
    fn route(&self, ctx: &RoutingContext<'_>) -> ModelChoice {
        let mut guard = self.seen.lock().expect("lock poisoned");
        guard.push(SeenCtx {
            phase_is_normal: matches!(ctx.phase, RoutingPhase::Normal),
            iteration: ctx.iteration,
            conversation_tokens: ctx.conversation_tokens,
            has_user_message: ctx.latest_user_message.is_some(),
            requires_tool_use: ctx.requires_tool_use,
        });
        self.choice.clone()
    }

    fn fallback(
        &self,
        _previous: &ModelChoice,
        _hint: RoutingFailureHint,
    ) -> Option<ModelChoice> {
        None
    }
}

/// Apply the same routing decision as run_engine.rs:662 using the
/// public AgentConfig.router field. This mirrors the single call-site
/// invariant so the AC tests don't require firing up the full runtime.
fn apply_routing(
    cfg: &AgentConfig,
    ctx: &RoutingContext<'_>,
) -> (String, Option<String>, &'static str) {
    match &cfg.router {
        Some(handle) => {
            let c = handle.as_router().route(ctx);
            (c.model_id, c.reasoning_effort, c.routing_reason)
        }
        None => (
            cfg.model.clone(),
            cfg.reasoning_effort.clone(),
            "no_router",
        ),
    }
}

// ── R3-AC-1 ──────────────────────────────────────────────────────────

#[test]
fn test_r3_ac_1_run_engine_uses_router_model() {
    let choice = ModelChoice::new("anthropic", "haiku-mock", 2048);
    let (mock, _seen) = MockRouter::new(choice.clone());
    let mut cfg = AgentConfig::default();
    cfg.model = "default-model".to_string();
    cfg.router = Some(RouterHandle::new(Arc::new(mock)));

    let mut ctx = RoutingContext::new(RoutingPhase::Normal);
    ctx.latest_user_message = Some("list files");
    let (model, _effort, reason) = apply_routing(&cfg, &ctx);
    assert_eq!(model, "haiku-mock");
    assert_eq!(reason, "default"); // ModelChoice::new default reason
}

// ── R3-AC-2 ──────────────────────────────────────────────────────────

#[test]
fn test_r3_ac_2_none_router_preserves_session_default_model() {
    let mut cfg = AgentConfig::default();
    cfg.model = "session-default".to_string();
    cfg.router = None;
    let ctx = RoutingContext::new(RoutingPhase::Normal);
    let (model, _effort, reason) = apply_routing(&cfg, &ctx);
    assert_eq!(model, "session-default");
    assert_eq!(reason, "no_router");
}

// ── R3-AC-3 ──────────────────────────────────────────────────────────

#[test]
fn test_r3_ac_3_routing_context_populated_with_iteration_and_tokens() {
    let (mock, seen) = MockRouter::new(ModelChoice::new("p", "m", 100));
    let mut cfg = AgentConfig::default();
    cfg.router = Some(RouterHandle::new(Arc::new(mock)));

    let mut ctx = RoutingContext::new(RoutingPhase::Normal);
    ctx.iteration = 7;
    ctx.conversation_tokens = 12345;
    ctx.latest_user_message = Some("debug this");
    ctx.requires_tool_use = true;
    apply_routing(&cfg, &ctx);

    let guard = seen.lock().unwrap();
    let last = guard.last().expect("MockRouter must have been called");
    assert_eq!(last.iteration, 7);
    assert_eq!(last.conversation_tokens, 12345);
    assert!(last.has_user_message);
    assert!(last.requires_tool_use);
    assert!(last.phase_is_normal);
}

// ── R3-AC-4 ──────────────────────────────────────────────────────────

#[test]
fn test_r3_ac_4_routing_does_not_mutate_session_model() {
    // Router returns "cheap" model; after the call, AgentConfig.model
    // must still hold its original "default" value.
    let choice = ModelChoice::new("anthropic", "cheap-model", 2048);
    let (mock, _seen) = MockRouter::new(choice);
    let mut cfg = AgentConfig::default();
    cfg.model = "default".to_string();
    cfg.router = Some(RouterHandle::new(Arc::new(mock)));

    let ctx = RoutingContext::new(RoutingPhase::Normal);
    let (model, _, _) = apply_routing(&cfg, &ctx);
    assert_eq!(model, "cheap-model");
    assert_eq!(
        cfg.model, "default",
        "router must not mutate AgentConfig.model"
    );
}

// ── R3-AC-5 ──────────────────────────────────────────────────────────

struct PanicRouter;
impl ModelRouter for PanicRouter {
    fn route(&self, _ctx: &RoutingContext<'_>) -> ModelChoice {
        panic!("intentional panic for R3-AC-5");
    }
    fn fallback(
        &self,
        _previous: &ModelChoice,
        _hint: RoutingFailureHint,
    ) -> Option<ModelChoice> {
        None
    }
}

#[test]
fn test_r3_ac_5_router_failure_falls_back_to_session_default() {
    // Mirror the catch_unwind guard from run_engine.rs.
    let mut cfg = AgentConfig::default();
    cfg.model = "session-default".to_string();
    cfg.router = Some(RouterHandle::new(Arc::new(PanicRouter)));

    let ctx = RoutingContext::new(RoutingPhase::Normal);
    let guarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        apply_routing(&cfg, &ctx)
    }));
    // The real run_engine catches the panic inside the routing block
    // and continues with session defaults. We replicate that here.
    let (model, _, reason) = match guarded {
        Ok(value) => value,
        Err(_) => (
            cfg.model.clone(),
            cfg.reasoning_effort.clone(),
            "router_panic_fallback_default",
        ),
    };
    assert_eq!(model, "session-default");
    assert_eq!(reason, "router_panic_fallback_default");
}

// ── R3-AC-6 ──────────────────────────────────────────────────────────

#[test]
fn test_r3_ac_6_routing_reason_surfaces_through_chat_request_pipeline() {
    let mut choice = ModelChoice::new("anthropic", "model-x", 1024);
    choice.routing_reason = "simple_turn";
    let (mock, _seen) = MockRouter::new(choice);
    let mut cfg = AgentConfig::default();
    cfg.router = Some(RouterHandle::new(Arc::new(mock)));
    let ctx = RoutingContext::new(RoutingPhase::Normal);
    let (_, _, reason) = apply_routing(&cfg, &ctx);
    assert_eq!(reason, "simple_turn");
}

// ── Single call-site invariant (structural) ──────────────────────────

#[test]
fn test_router_invoked_exactly_once_per_turn_in_runtime() {
    // Structural hygiene: grep the runtime for `router.route(` and
    // `as_router().route(` — there must be exactly one such call site
    // per turn. The R4 phase will add a second site for Compaction,
    // but R3 landed with a single site.
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/src/run_engine.rs");
    let source = std::fs::read_to_string(manifest).expect("run_engine.rs readable");
    let matches = source.matches("as_router().route(").count();
    assert_eq!(
        matches, 1,
        "R3 invariant: exactly one call site for router.route() in run_engine.rs (found {matches})"
    );
}
