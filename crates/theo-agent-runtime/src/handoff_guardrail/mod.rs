//! `HandoffGuardrail` — 3-tier validation pipeline executed by
//! `delegate_task` BEFORE spawning a sub-agent. Phase 18 — sota-gaps-plan.
//!
//! Tiers:
//! 1. **Built-in guardrails** — always active. Prevent obvious violations
//!    (e.g. read-only agent receiving a mutation objective).
//! 2. **Project guardrails** — registered via `GuardrailChain::add` from the
//!    parent runtime (e.g. .theo/handoff_guardrails.md or programmatic).
//! 3. **PreHandoff hook** — `lifecycle_hooks::HookEvent::PreHandoff` runs
//!    after the chain; user/operator gate.
//!
//! Each tier produces a `GuardrailDecision`. The first `Block` wins; the
//! handoff is refused with a clear `reason` returned to the caller. Every
//! evaluation emits a `EventType::HandoffEvaluated` audit record so the
//! dashboard can show why a handoff was approved or denied.
//!
//! References:
//! - OpenAI Agents SDK `Guardrail` (input/output classification)
//! - LangGraph `interrupt` / human-in-the-loop pattern
//! - Anthropic multi-agent paper §6 ("guardrails as composable validators")

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use theo_domain::agent_spec::AgentSpec;
use theo_domain::capability::CapabilitySet;
use theo_domain::tool::ToolCategory;

pub mod declarative;
pub use declarative::{
    parse_guardrails_toml, DeclarativeDecision, DeclarativeGuardrail,
    DeclarativeGuardrailSpec, DeclarativeGuardrailsFile, DeclarativeMatcher,
};

// ---------------------------------------------------------------------------
// HandoffContext — what every guardrail sees
// ---------------------------------------------------------------------------

/// Read-only view passed to each guardrail's `evaluate`.
#[derive(Debug, Clone)]
pub struct HandoffContext<'a> {
    /// Parent agent name (or `"main"` when the top-level loop delegates).
    pub source_agent: &'a str,
    /// Resolved sub-agent name (after registry lookup or on-demand fallback).
    pub target_agent: &'a str,
    /// Full target spec — gives access to capability_set, source, hooks, …
    pub target_spec: &'a AgentSpec,
    /// Verbatim objective string (case-sensitive — guardrails should
    /// normalize internally).
    pub objective: &'a str,
    /// Source agent's capabilities, when known. `None` for the main agent
    /// in headless mode (treat as `All` for evaluation purposes).
    pub source_capabilities: Option<&'a CapabilitySet>,
}

// ---------------------------------------------------------------------------
// GuardrailDecision — output
// ---------------------------------------------------------------------------

/// Outcome of a single guardrail's evaluation.
///
/// `GuardrailDecision` matches the OpenAI Agents SDK guardrail vocabulary
/// (Allow/Block plus the two mutating outcomes Redirect/RewriteObjective)
/// so dashboards and audit logs can serialise the verdict without lossy
/// label-only mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GuardrailDecision {
    /// Allow the handoff unchanged.
    Allow,
    /// Deny the handoff with a human-readable reason.
    Block { reason: String },
    /// Allow but emit a warning to the caller (logged + included in event payload).
    Warn { message: String },
    /// Substitute the target sub-agent with `new_agent_name`. The existing
    /// objective is forwarded unchanged. Used when the requested agent is
    /// the wrong tier for the task (e.g. a read-only target asked to mutate).
    Redirect { new_agent_name: String },
    /// Forward to the original target but with a new objective string.
    /// Used to coach the agent (e.g. inject context, narrow scope).
    RewriteObjective { new_objective: String },
}

impl GuardrailDecision {
    pub fn is_allow(&self) -> bool {
        matches!(self, GuardrailDecision::Allow)
    }
    pub fn is_block(&self) -> bool {
        matches!(self, GuardrailDecision::Block { .. })
    }
    pub fn is_warn(&self) -> bool {
        matches!(self, GuardrailDecision::Warn { .. })
    }
    pub fn is_redirect(&self) -> bool {
        matches!(self, GuardrailDecision::Redirect { .. })
    }
    pub fn is_rewrite(&self) -> bool {
        matches!(self, GuardrailDecision::RewriteObjective { .. })
    }
    /// True when the decision changes the spawn arguments (target or objective).
    pub fn is_mutating(&self) -> bool {
        self.is_redirect() || self.is_rewrite()
    }
    pub fn label(&self) -> &'static str {
        match self {
            GuardrailDecision::Allow => "allow",
            GuardrailDecision::Block { .. } => "block",
            GuardrailDecision::Warn { .. } => "warn",
            GuardrailDecision::Redirect { .. } => "redirect",
            GuardrailDecision::RewriteObjective { .. } => "rewrite",
        }
    }
}

// ---------------------------------------------------------------------------
// HandoffGuardrail trait
// ---------------------------------------------------------------------------

/// A handoff guardrail. Pure synchronous evaluation — guardrails are run on
/// the caller thread before spawn, so they MUST not block on I/O.
pub trait HandoffGuardrail: Send + Sync + std::fmt::Debug {
    /// Stable identifier (e.g. `"builtin.read_only_agent_must_not_mutate"`).
    fn id(&self) -> &str;
    fn evaluate(&self, ctx: &HandoffContext<'_>) -> GuardrailDecision;
}

// ---------------------------------------------------------------------------
// GuardrailChain — composable evaluator
// ---------------------------------------------------------------------------

/// Aggregated set of guardrails. `evaluate` runs every guardrail in order
/// and collects decisions; `is_blocked` short-circuits on the first Block.
#[derive(Debug, Default, Clone)]
pub struct GuardrailChain {
    guardrails: Vec<Arc<dyn HandoffGuardrail>>,
}

impl GuardrailChain {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a chain seeded with built-in guardrails.
    pub fn with_default_builtins() -> Self {
        let mut c = Self::new();
        c.add(Arc::new(ReadOnlyAgentMustNotMutate));
        c.add(Arc::new(ObjectiveMustNotBeEmpty));
        c
    }

    pub fn add(&mut self, g: Arc<dyn HandoffGuardrail>) {
        self.guardrails.push(g);
    }

    pub fn len(&self) -> usize {
        self.guardrails.len()
    }

    pub fn is_empty(&self) -> bool {
        self.guardrails.is_empty()
    }

    /// Names of every registered guardrail.
    pub fn ids(&self) -> Vec<String> {
        self.guardrails.iter().map(|g| g.id().to_string()).collect()
    }

    /// Run every guardrail. Returns paired (id, decision) tuples in order.
    pub fn evaluate(&self, ctx: &HandoffContext<'_>) -> Vec<(String, GuardrailDecision)> {
        self.guardrails
            .iter()
            .map(|g| (g.id().to_string(), g.evaluate(ctx)))
            .collect()
    }

    /// Short-circuit query: returns `Some((id, reason))` of the first
    /// blocking guardrail, else `None`.
    pub fn first_block(&self, ctx: &HandoffContext<'_>) -> Option<(String, String)> {
        for g in &self.guardrails {
            if let GuardrailDecision::Block { reason } = g.evaluate(ctx) {
                return Some((g.id().to_string(), reason));
            }
        }
        None
    }

    /// Walk the chain returning the first non-`Allow` decision (Block,
    /// Redirect, RewriteObjective, or Warn) paired with its guardrail id.
    /// Returns `None` if every guardrail allowed.
    ///
    /// Phase 18: chains stop at the first opinionated decision so a custom
    /// guardrail registered after a built-in does not silently override
    /// the built-in's verdict.
    pub fn first_decision(
        &self,
        ctx: &HandoffContext<'_>,
    ) -> Option<(String, GuardrailDecision)> {
        for g in &self.guardrails {
            let decision = g.evaluate(ctx);
            if !decision.is_allow() {
                return Some((g.id().to_string(), decision));
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Built-in guardrails
// ---------------------------------------------------------------------------

/// Block when the target sub-agent has no write/edit capability but the
/// objective contains explicit mutation intent. Heuristic — false positives
/// are acceptable because the user can always re-issue with an
/// implementation agent.
#[derive(Debug)]
pub struct ReadOnlyAgentMustNotMutate;

impl ReadOnlyAgentMustNotMutate {
    /// Detects mutation keywords. Word-boundary matching on lowercased
    /// objective. Conservative: only flags clearly imperative verbs.
    pub fn objective_implies_mutation(objective: &str) -> bool {
        let lower = objective.to_lowercase();
        const VERBS: &[&str] = &[
            "implement ", "edit ", "write ",
            "modify ", "create ", "patch ",
            "refactor ", "fix bug", "delete ",
            "add new ", "rewrite ", "remove ",
        ];
        VERBS.iter().any(|w| lower.contains(w))
    }

    /// True when a CapabilitySet permits no file mutation tools.
    pub fn is_capability_set_read_only(caps: &CapabilitySet) -> bool {
        // Read-only ⇔ neither edit nor write nor bash usable.
        let can_edit = caps.can_use_tool("edit", ToolCategory::FileOps);
        let can_write = caps.can_use_tool("write", ToolCategory::FileOps);
        let can_bash = caps.can_use_tool("bash", ToolCategory::Execution);
        !can_edit && !can_write && !can_bash
    }
}

impl HandoffGuardrail for ReadOnlyAgentMustNotMutate {
    fn id(&self) -> &str {
        "builtin.read_only_agent_must_not_mutate"
    }
    fn evaluate(&self, ctx: &HandoffContext<'_>) -> GuardrailDecision {
        if !Self::objective_implies_mutation(ctx.objective) {
            return GuardrailDecision::Allow;
        }
        if Self::is_capability_set_read_only(&ctx.target_spec.capability_set) {
            // Plan §18 default: redirect to `implementer` rather than block —
            // the LLM rarely benefits from a refusal here; transparently
            // upgrading the target preserves intent. The handle_delegate_task
            // path emits a `HandoffEvaluated` audit event so the operator can
            // see exactly which redirection happened.
            return GuardrailDecision::Redirect {
                new_agent_name: "implementer".to_string(),
            };
        }
        GuardrailDecision::Allow
    }
}

/// Reject empty objectives. Cheap sanity check that catches LLM hallucination
/// of a `delegate_task` call without the required argument string.
#[derive(Debug)]
pub struct ObjectiveMustNotBeEmpty;

impl HandoffGuardrail for ObjectiveMustNotBeEmpty {
    fn id(&self) -> &str {
        "builtin.objective_must_not_be_empty"
    }
    fn evaluate(&self, ctx: &HandoffContext<'_>) -> GuardrailDecision {
        if ctx.objective.trim().is_empty() {
            GuardrailDecision::Block {
                reason: format!(
                    "Empty objective for handoff to '{}'. Provide a concrete instruction.",
                    ctx.target_agent
                ),
            }
        } else {
            GuardrailDecision::Allow
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Char-aware truncation used by built-in guardrail messages.
#[allow(dead_code)]
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('…');
        t
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn explorer_spec() -> AgentSpec {
        crate::subagent::builtins::explorer()
    }

    fn implementer_spec() -> AgentSpec {
        crate::subagent::builtins::implementer()
    }

    fn ctx<'a>(target: &'a AgentSpec, objective: &'a str) -> HandoffContext<'a> {
        HandoffContext {
            source_agent: "main",
            target_agent: &target.name,
            target_spec: target,
            objective,
            source_capabilities: None,
        }
    }

    // ── GuardrailDecision ──

    #[test]
    fn decision_helpers_classify_correctly() {
        assert!(GuardrailDecision::Allow.is_allow());
        assert!(!GuardrailDecision::Allow.is_block());
        let b = GuardrailDecision::Block { reason: "x".into() };
        assert!(b.is_block());
        assert!(!b.is_allow());
        let w = GuardrailDecision::Warn { message: "x".into() };
        assert!(w.is_warn());
    }

    #[test]
    fn decision_label_returns_canonical_strings() {
        assert_eq!(GuardrailDecision::Allow.label(), "allow");
        assert_eq!(
            GuardrailDecision::Block { reason: "r".into() }.label(),
            "block"
        );
        assert_eq!(
            GuardrailDecision::Warn { message: "m".into() }.label(),
            "warn"
        );
    }

    // ── ReadOnlyAgentMustNotMutate ──

    #[test]
    fn objective_implies_mutation_detects_implement() {
        assert!(ReadOnlyAgentMustNotMutate::objective_implies_mutation(
            "implement the foo function"
        ));
    }

    #[test]
    fn objective_implies_mutation_detects_refactor() {
        assert!(ReadOnlyAgentMustNotMutate::objective_implies_mutation(
            "Refactor the parser to use Tree-Sitter"
        ));
    }

    #[test]
    fn objective_implies_mutation_detects_fix_bug() {
        assert!(ReadOnlyAgentMustNotMutate::objective_implies_mutation(
            "fix bug in retry logic"
        ));
    }

    #[test]
    fn objective_implies_mutation_ignores_read_verbs() {
        assert!(!ReadOnlyAgentMustNotMutate::objective_implies_mutation(
            "read the config file and explain it"
        ));
        assert!(!ReadOnlyAgentMustNotMutate::objective_implies_mutation(
            "summarize the architecture"
        ));
        assert!(!ReadOnlyAgentMustNotMutate::objective_implies_mutation(
            "scan for foo references"
        ));
    }

    #[test]
    fn is_capability_set_read_only_explorer_returns_true() {
        assert!(ReadOnlyAgentMustNotMutate::is_capability_set_read_only(
            &explorer_spec().capability_set
        ));
    }

    #[test]
    fn is_capability_set_read_only_implementer_returns_false() {
        assert!(!ReadOnlyAgentMustNotMutate::is_capability_set_read_only(
            &implementer_spec().capability_set
        ));
    }

    #[test]
    fn read_only_guardrail_redirects_explorer_implementing_to_implementer() {
        let target = explorer_spec();
        let g = ReadOnlyAgentMustNotMutate;
        let d = g.evaluate(&ctx(&target, "implement caching layer"));
        assert!(d.is_redirect(), "must redirect read-only target with mutation objective; got {:?}", d);
        let GuardrailDecision::Redirect { new_agent_name } = d else {
            unreachable!()
        };
        assert_eq!(new_agent_name, "implementer");
    }

    #[test]
    fn read_only_guardrail_redirect_targets_implementer_for_implement_keyword() {
        let target = explorer_spec();
        let g = ReadOnlyAgentMustNotMutate;
        let d = g.evaluate(&ctx(&target, "implement OAuth"));
        assert!(d.is_redirect(), "implement keyword must redirect; got {:?}", d);
    }

    #[test]
    fn read_only_guardrail_redirect_targets_implementer_for_write_keyword() {
        let target = explorer_spec();
        let g = ReadOnlyAgentMustNotMutate;
        let d = g.evaluate(&ctx(&target, "write a new module"));
        assert!(d.is_redirect(), "write keyword must redirect; got {:?}", d);
    }

    #[test]
    fn read_only_guardrail_allows_implementer_implementing() {
        let target = implementer_spec();
        let g = ReadOnlyAgentMustNotMutate;
        assert_eq!(
            g.evaluate(&ctx(&target, "implement caching layer")),
            GuardrailDecision::Allow
        );
    }

    #[test]
    fn read_only_guardrail_allows_read_only_for_read_objective() {
        let target = explorer_spec();
        let g = ReadOnlyAgentMustNotMutate;
        assert_eq!(
            g.evaluate(&ctx(&target, "read Cargo.toml and list workspace crates")),
            GuardrailDecision::Allow
        );
    }

    #[test]
    fn read_only_guardrail_id_is_stable() {
        let g = ReadOnlyAgentMustNotMutate;
        assert_eq!(g.id(), "builtin.read_only_agent_must_not_mutate");
    }

    // ── ObjectiveMustNotBeEmpty ──

    #[test]
    fn objective_must_not_be_empty_blocks_empty_string() {
        let target = explorer_spec();
        let g = ObjectiveMustNotBeEmpty;
        assert!(g.evaluate(&ctx(&target, "")).is_block());
    }

    #[test]
    fn objective_must_not_be_empty_blocks_whitespace_only() {
        let target = explorer_spec();
        let g = ObjectiveMustNotBeEmpty;
        assert!(g.evaluate(&ctx(&target, "   \n\t  ")).is_block());
    }

    #[test]
    fn objective_must_not_be_empty_allows_meaningful_text() {
        let target = explorer_spec();
        let g = ObjectiveMustNotBeEmpty;
        assert_eq!(
            g.evaluate(&ctx(&target, "find foo references")),
            GuardrailDecision::Allow
        );
    }

    // ── GuardrailChain ──

    #[test]
    fn chain_new_is_empty() {
        let c = GuardrailChain::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn chain_with_default_builtins_has_two_guardrails() {
        let c = GuardrailChain::with_default_builtins();
        assert_eq!(c.len(), 2);
        let ids = c.ids();
        assert!(ids.contains(&"builtin.read_only_agent_must_not_mutate".to_string()));
        assert!(ids.contains(&"builtin.objective_must_not_be_empty".to_string()));
    }

    #[test]
    fn chain_evaluate_runs_every_guardrail_in_order() {
        let c = GuardrailChain::with_default_builtins();
        let target = implementer_spec();
        let decisions = c.evaluate(&ctx(&target, "do something"));
        assert_eq!(decisions.len(), 2);
        assert!(decisions.iter().all(|(_, d)| d.is_allow()));
    }

    #[test]
    fn chain_first_decision_returns_redirect_for_explorer_implementing() {
        let c = GuardrailChain::with_default_builtins();
        let target = explorer_spec();
        let (id, decision) = c
            .first_decision(&ctx(&target, "implement OAuth"))
            .expect("must return a decision");
        assert_eq!(id, "builtin.read_only_agent_must_not_mutate");
        assert!(decision.is_redirect());
    }

    #[test]
    fn chain_first_block_returns_none_for_redirect_decision() {
        // Redirect is non-blocking, so first_block ignores it.
        let c = GuardrailChain::with_default_builtins();
        let target = explorer_spec();
        assert!(c.first_block(&ctx(&target, "implement OAuth")).is_none());
    }

    #[test]
    fn chain_first_block_returns_none_when_all_allow() {
        let c = GuardrailChain::with_default_builtins();
        let target = implementer_spec();
        assert!(c.first_block(&ctx(&target, "implement caching")).is_none());
    }

    #[test]
    fn chain_first_block_short_circuits_on_empty_objective() {
        // Empty objective is checked first by ObjectiveMustNotBeEmpty? No,
        // ReadOnlyAgentMustNotMutate is added first — but it returns Allow
        // for empty objective (no mutation verb). So ObjectiveMustNotBeEmpty
        // catches it second.
        let c = GuardrailChain::with_default_builtins();
        let target = explorer_spec();
        let r = c.first_block(&ctx(&target, "")).unwrap();
        assert_eq!(r.0, "builtin.objective_must_not_be_empty");
    }

    // ── Plan-named chain semantic tests (sota-gaps-plan.md §18 RED list) ──

    #[test]
    fn guardrail_chain_empty_returns_allow() {
        let c = GuardrailChain::new();
        let target = implementer_spec();
        // Empty chain → first_decision returns None (= allow) and
        // first_block returns None.
        assert!(c.first_decision(&ctx(&target, "do anything")).is_none());
        assert!(c.first_block(&ctx(&target, "do anything")).is_none());
    }

    #[test]
    fn guardrail_chain_first_block_wins() {
        // First Block decision wins; subsequent guardrails are not consulted.
        #[derive(Debug)]
        struct BlockA;
        impl HandoffGuardrail for BlockA {
            fn id(&self) -> &str { "first.blocker" }
            fn evaluate(&self, _: &HandoffContext<'_>) -> GuardrailDecision {
                GuardrailDecision::Block { reason: "first".into() }
            }
        }
        #[derive(Debug)]
        struct BlockB;
        impl HandoffGuardrail for BlockB {
            fn id(&self) -> &str { "second.blocker" }
            fn evaluate(&self, _: &HandoffContext<'_>) -> GuardrailDecision {
                GuardrailDecision::Block { reason: "second".into() }
            }
        }
        let mut c = GuardrailChain::new();
        c.add(Arc::new(BlockA));
        c.add(Arc::new(BlockB));
        let target = implementer_spec();
        let (id, reason) = c.first_block(&ctx(&target, "x")).unwrap();
        assert_eq!(id, "first.blocker");
        assert_eq!(reason, "first");
    }

    #[test]
    fn guardrail_chain_first_redirect_wins() {
        #[derive(Debug)]
        struct RedirectA;
        impl HandoffGuardrail for RedirectA {
            fn id(&self) -> &str { "first.redirect" }
            fn evaluate(&self, _: &HandoffContext<'_>) -> GuardrailDecision {
                GuardrailDecision::Redirect {
                    new_agent_name: "alpha".into(),
                }
            }
        }
        #[derive(Debug)]
        struct RedirectB;
        impl HandoffGuardrail for RedirectB {
            fn id(&self) -> &str { "second.redirect" }
            fn evaluate(&self, _: &HandoffContext<'_>) -> GuardrailDecision {
                GuardrailDecision::Redirect {
                    new_agent_name: "beta".into(),
                }
            }
        }
        let mut c = GuardrailChain::new();
        c.add(Arc::new(RedirectA));
        c.add(Arc::new(RedirectB));
        let target = implementer_spec();
        let (id, decision) = c.first_decision(&ctx(&target, "x")).unwrap();
        assert_eq!(id, "first.redirect");
        if let GuardrailDecision::Redirect { new_agent_name } = decision {
            assert_eq!(new_agent_name, "alpha");
        } else {
            panic!("expected Redirect");
        }
    }

    #[test]
    fn guardrail_chain_skips_allow_continues_to_next() {
        // First guardrail allows, second blocks: chain reports the second.
        #[derive(Debug)]
        struct AllowFirst;
        impl HandoffGuardrail for AllowFirst {
            fn id(&self) -> &str { "first.allow" }
            fn evaluate(&self, _: &HandoffContext<'_>) -> GuardrailDecision {
                GuardrailDecision::Allow
            }
        }
        #[derive(Debug)]
        struct BlockSecond;
        impl HandoffGuardrail for BlockSecond {
            fn id(&self) -> &str { "second.blocker" }
            fn evaluate(&self, _: &HandoffContext<'_>) -> GuardrailDecision {
                GuardrailDecision::Block { reason: "deny".into() }
            }
        }
        let mut c = GuardrailChain::new();
        c.add(Arc::new(AllowFirst));
        c.add(Arc::new(BlockSecond));
        let target = implementer_spec();
        let (id, _) = c.first_block(&ctx(&target, "x")).unwrap();
        assert_eq!(id, "second.blocker");
    }

    // ── Custom guardrail (project tier) ──

    #[derive(Debug)]
    struct AlwaysBlock(&'static str);
    impl HandoffGuardrail for AlwaysBlock {
        fn id(&self) -> &str {
            self.0
        }
        fn evaluate(&self, _ctx: &HandoffContext<'_>) -> GuardrailDecision {
            GuardrailDecision::Block {
                reason: "test".into(),
            }
        }
    }

    #[test]
    fn chain_custom_guardrail_can_block() {
        let mut c = GuardrailChain::new();
        c.add(Arc::new(AlwaysBlock("project.test_blocker")));
        let target = implementer_spec();
        let r = c.first_block(&ctx(&target, "ok")).unwrap();
        assert_eq!(r.0, "project.test_blocker");
    }

    #[test]
    fn chain_runs_custom_guardrail_after_builtins() {
        let mut c = GuardrailChain::with_default_builtins();
        c.add(Arc::new(AlwaysBlock("project.last_resort")));
        let target = implementer_spec();
        // Builtins all allow (good objective + impl agent), so custom blocks.
        let r = c.first_block(&ctx(&target, "implement foo")).unwrap();
        assert_eq!(r.0, "project.last_resort");
    }

    // ── serde roundtrip (audit trail uses these) ──

    #[test]
    fn handoff_decision_serde_roundtrip_allow() {
        let d = GuardrailDecision::Allow;
        let json = serde_json::to_string(&d).unwrap();
        let back: GuardrailDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
        assert!(json.contains("\"kind\":\"allow\""));
    }

    #[test]
    fn handoff_decision_serde_roundtrip_block() {
        let d = GuardrailDecision::Block { reason: "boom".into() };
        let json = serde_json::to_string(&d).unwrap();
        let back: GuardrailDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
        assert!(json.contains("\"kind\":\"block\""));
        assert!(json.contains("boom"));
    }

    #[test]
    fn handoff_decision_serde_roundtrip_redirect() {
        let d = GuardrailDecision::Redirect {
            new_agent_name: "implementer".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: GuardrailDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
        assert!(json.contains("\"kind\":\"redirect\""));
        assert!(json.contains("implementer"));
    }

    #[test]
    fn handoff_decision_serde_roundtrip_rewrite() {
        let d = GuardrailDecision::RewriteObjective {
            new_objective: "scoped objective".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: GuardrailDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
        assert!(json.contains("\"kind\":\"rewrite_objective\""));
        assert!(json.contains("scoped objective"));
    }

    #[test]
    fn decision_classifier_methods_handle_all_variants() {
        assert!(GuardrailDecision::Allow.is_allow());
        assert!(GuardrailDecision::Block { reason: "x".into() }.is_block());
        assert!(GuardrailDecision::Warn { message: "x".into() }.is_warn());
        assert!(GuardrailDecision::Redirect { new_agent_name: "y".into() }.is_redirect());
        assert!(GuardrailDecision::RewriteObjective { new_objective: "z".into() }.is_rewrite());
    }

    #[test]
    fn decision_is_mutating_only_for_redirect_and_rewrite() {
        assert!(!GuardrailDecision::Allow.is_mutating());
        assert!(!GuardrailDecision::Block { reason: "x".into() }.is_mutating());
        assert!(!GuardrailDecision::Warn { message: "x".into() }.is_mutating());
        assert!(GuardrailDecision::Redirect { new_agent_name: "y".into() }.is_mutating());
        assert!(GuardrailDecision::RewriteObjective { new_objective: "z".into() }.is_mutating());
    }

    #[test]
    fn decision_label_returns_canonical_strings_for_redirect_and_rewrite() {
        assert_eq!(
            GuardrailDecision::Redirect { new_agent_name: "x".into() }.label(),
            "redirect"
        );
        assert_eq!(
            GuardrailDecision::RewriteObjective { new_objective: "y".into() }.label(),
            "rewrite"
        );
    }

    // ── chain.first_decision returns mutating decisions ──

    #[derive(Debug)]
    struct AlwaysRewrite(&'static str);
    impl HandoffGuardrail for AlwaysRewrite {
        fn id(&self) -> &str { self.0 }
        fn evaluate(&self, _ctx: &HandoffContext<'_>) -> GuardrailDecision {
            GuardrailDecision::RewriteObjective {
                new_objective: "scoped".into(),
            }
        }
    }

    #[test]
    fn chain_first_decision_returns_rewrite_when_first_guardrail_rewrites() {
        let mut c = GuardrailChain::new();
        c.add(Arc::new(AlwaysRewrite("project.scope_tightener")));
        let target = implementer_spec();
        let (id, d) = c.first_decision(&ctx(&target, "do something")).unwrap();
        assert_eq!(id, "project.scope_tightener");
        assert!(d.is_rewrite());
    }

    #[test]
    fn chain_first_decision_returns_none_when_all_allow() {
        let c = GuardrailChain::with_default_builtins();
        let target = implementer_spec();
        // implementer + non-mutation objective → all allow
        assert!(c.first_decision(&ctx(&target, "review existing tests")).is_none());
    }

    // ── HandoffContext ──

    #[test]
    fn handoff_context_carries_all_fields() {
        let target = explorer_spec();
        let c = HandoffContext {
            source_agent: "main",
            target_agent: "explorer",
            target_spec: &target,
            objective: "search src/",
            source_capabilities: Some(&target.capability_set),
        };
        assert_eq!(c.source_agent, "main");
        assert_eq!(c.target_agent, "explorer");
        assert_eq!(c.objective, "search src/");
        assert!(c.source_capabilities.is_some());
    }
}
