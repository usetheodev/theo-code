//! Rule-based `ModelRouter` implementation.
//!
//! Deterministic, offline-first. Consumes a `PricingTable` for slot
//! resolution so concrete model IDs never leak into the router logic.
//! The classifier function (`is_simple_turn`) uses the paraphrased
//! keyword set from `keywords.rs` plus a length heuristic.
//!
//! Design ref: `outputs/smart-model-routing-plan.md` §2 R2 + research
//! §4.2 (file citation in-line below).

use theo_domain::routing::{
    ModelChoice, ModelRouter, RoutingContext, RoutingFailureHint, RoutingPhase, SubAgentRoleId,
};

use super::keywords::{MAX_SIMPLE_CHARS, MAX_SIMPLE_WORDS, matches_complex_keyword};
use super::pricing::PricingTable;

/// Rule-based router backed by a slot-keyed pricing table.
#[derive(Debug, Clone)]
pub struct RuleBasedRouter {
    table: PricingTable,
}

impl RuleBasedRouter {
    pub fn new(table: PricingTable) -> Self {
        Self { table }
    }

    /// True when the prompt is short and lacks a complex-keyword signal.
    pub fn is_simple_turn(prompt: &str) -> bool {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return false;
        }
        if trimmed.chars().count() > MAX_SIMPLE_CHARS {
            return false;
        }
        if trimmed.split_whitespace().count() > MAX_SIMPLE_WORDS {
            return false;
        }
        !matches_complex_keyword(trimmed)
    }

    fn slot_for_role(role: &SubAgentRoleId) -> String {
        format!("subagent_{}", role.as_str())
    }

    fn default_or_error(&self, reason: &'static str) -> ModelChoice {
        // `resolve_or_default` only errors when `default` itself is
        // missing; callers ensure the table is minimally configured via
        // the builder. We fall back to a synthetic ModelChoice rather
        // than panicking to keep the router robust at runtime.
        self.table
            .resolve_or_default("default")
            .map(|mut c| {
                c.routing_reason = reason;
                c
            })
            .unwrap_or_else(|_| ModelChoice {
                provider_id: "unknown".to_string(),
                model_id: "unknown".to_string(),
                max_output_tokens: 0,
                reasoning_effort: None,
                routing_reason: "no_default_configured",
            })
    }

    fn resolve_slot(&self, slot: &str, reason: &'static str) -> ModelChoice {
        match self.table.resolve_or_default(slot) {
            Ok(mut choice) => {
                choice.routing_reason = reason;
                choice
            }
            Err(_) => self.default_or_error("missing_slot_fallback_default"),
        }
    }
}

impl ModelRouter for RuleBasedRouter {
    fn route(&self, ctx: &RoutingContext<'_>) -> ModelChoice {
        match &ctx.phase {
            RoutingPhase::Compaction => self.resolve_slot("compact", "phase_compaction"),
            RoutingPhase::Vision => self.resolve_slot("vision", "phase_vision"),
            RoutingPhase::Subagent { role } => {
                let slot = Self::slot_for_role(role);
                self.resolve_slot(&slot, "phase_subagent")
            }
            RoutingPhase::SelfCritique => self.resolve_slot("strong", "phase_self_critique"),
            RoutingPhase::Classifier => self.resolve_slot("cheap", "phase_classifier"),
            RoutingPhase::Normal => {
                if ctx.requires_vision {
                    return self.resolve_slot("vision", "vision_required");
                }
                if let Some(msg) = ctx.latest_user_message {
                    if Self::is_simple_turn(msg) {
                        return self.resolve_slot("cheap", "simple_turn");
                    }
                }
                self.default_or_error("complex_turn_default")
            }
            // `#[non_exhaustive]` on RoutingPhase forces this arm; new
            // variants should be mapped explicitly, not silently routed.
            _ => self.default_or_error("unknown_phase_default"),
        }
    }

    fn fallback(
        &self,
        previous: &ModelChoice,
        hint: RoutingFailureHint,
    ) -> Option<ModelChoice> {
        match hint {
            RoutingFailureHint::BudgetExhausted => None,
            RoutingFailureHint::ContextOverflow => {
                // Escalate to a larger-window tier.
                let strong = self.table.resolve("strong").ok()?;
                if strong == *previous {
                    return None;
                }
                Some(ModelChoice {
                    routing_reason: "fallback_context_overflow",
                    ..strong
                })
            }
            RoutingFailureHint::RateLimit => {
                // Swap to a same-tier sibling provider if the table
                // holds one under `default_alt` / `cheap_alt` / etc.
                let alt_slot = format!("{}_alt", infer_tier(&self.table, previous));
                let alt = self.table.resolve(&alt_slot).ok()?;
                if alt == *previous {
                    return None;
                }
                Some(ModelChoice {
                    routing_reason: "fallback_rate_limit_sibling",
                    ..alt
                })
            }
            RoutingFailureHint::Transient => {
                // Caller retries once on the same model; if it reaches
                // the router for a second hop we hop to the default.
                let default = self.table.resolve("default").ok()?;
                if default == *previous {
                    // Already on default — try strong instead.
                    let strong = self.table.resolve("strong").ok()?;
                    if strong == *previous {
                        return None;
                    }
                    return Some(ModelChoice {
                        routing_reason: "fallback_transient_escalate",
                        ..strong
                    });
                }
                Some(ModelChoice {
                    routing_reason: "fallback_transient_default",
                    ..default
                })
            }
            _ => None,
        }
    }
}

/// Infer which tier name `previous` currently sits in by scanning the
/// table. Returns `"default"` if no exact match is found.
fn infer_tier(table: &PricingTable, previous: &ModelChoice) -> String {
    for name in ["cheap", "default", "strong", "vision", "compact"] {
        if let Ok(c) = table.resolve(name) {
            if c.provider_id == previous.provider_id && c.model_id == previous.model_id {
                return name.to_string();
            }
        }
    }
    "default".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::routing::SubAgentRoleId;

    fn table() -> PricingTable {
        let mut t = PricingTable::new();
        t.insert("cheap", ModelChoice::new("anthropic", "haiku", 4096));
        t.insert("default", ModelChoice::new("anthropic", "sonnet", 8192));
        t.insert("strong", ModelChoice::new("anthropic", "opus", 16384));
        t.insert("compact", ModelChoice::new("anthropic", "haiku", 2048));
        t.insert("vision", ModelChoice::new("anthropic", "sonnet-vl", 4096));
        t.insert(
            "subagent_explorer",
            ModelChoice::new("anthropic", "haiku", 2048),
        );
        t
    }

    fn ctx_normal(msg: &str) -> RoutingContext<'_> {
        let mut c = RoutingContext::new(RoutingPhase::Normal);
        c.latest_user_message = Some(msg);
        c
    }

    // ── R2-AC-1 ─────────────────────────────────────────────────
    #[test]
    fn test_simple_prompt_returns_cheap_tier() {
        let router = RuleBasedRouter::new(table());
        let simples = [
            "list files",
            "show git status",
            "print hello world",
            "echo version",
            "cat README",
            "what is the current directory",
            "wc -l Cargo.toml",
            "show env vars",
            "ls -la",
            "rustc --version",
        ];
        for prompt in simples {
            let choice = router.route(&ctx_normal(prompt));
            assert_eq!(
                choice.model_id, "haiku",
                "prompt {prompt:?} must route to cheap tier (got {choice:?})"
            );
        }
    }

    // ── R2-AC-2 ─────────────────────────────────────────────────
    #[test]
    fn test_complex_prompt_returns_default_tier() {
        let router = RuleBasedRouter::new(table());
        let complexes = [
            "debug this traceback",
            "refactor the RunEngine so the builder splits cleanly",
            "implement retry with exponential backoff",
            "analyze the performance regression in bench",
            "design a sharded cache for the embedder",
            "review batch_execute for race conditions",
            "optimize BM25 scoring with SIMD intrinsics",
            "propose an architecture for streaming JSON",
            "debug the pytest failure in hf_hub on macOS",
            "benchmark the routing harness against baseline",
        ];
        for prompt in complexes {
            let choice = router.route(&ctx_normal(prompt));
            assert_eq!(
                choice.model_id, "sonnet",
                "prompt {prompt:?} must route to default tier (got {choice:?})"
            );
        }
    }

    // ── R2-AC-3 ─────────────────────────────────────────────────
    #[test]
    fn test_vision_phase_forces_vision_slot() {
        let router = RuleBasedRouter::new(table());
        let ctx = RoutingContext::new(RoutingPhase::Vision);
        let choice = router.route(&ctx);
        assert_eq!(choice.model_id, "sonnet-vl");
    }

    // ── R2-AC-4 ─────────────────────────────────────────────────
    #[test]
    fn test_compaction_phase_forces_compact_slot() {
        let router = RuleBasedRouter::new(table());
        let ctx = RoutingContext::new(RoutingPhase::Compaction);
        let choice = router.route(&ctx);
        assert_eq!(choice.model_id, "haiku");
        assert_eq!(choice.max_output_tokens, 2048);
        assert_eq!(choice.routing_reason, "phase_compaction");
    }

    // ── R2-AC-5 ─────────────────────────────────────────────────
    #[test]
    fn test_keyword_list_derivation_documented() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/routing/keywords.rs");
        let source = std::fs::read_to_string(path).expect("keywords.rs must be readable");
        assert!(
            source.contains("paraphrased-from:"),
            "keywords.rs header must cite `paraphrased-from:` for licensing traceability"
        );
    }

    // ── R2-AC-6 ─────────────────────────────────────────────────
    #[test]
    fn test_pricing_table_resolves_tier_alias() {
        let t = table();
        let (provider, model) = {
            let choice = t.resolve("cheap").unwrap();
            (choice.provider_id, choice.model_id)
        };
        assert_eq!(provider, "anthropic");
        assert_eq!(model, "haiku");
        assert!(t.resolve("no_such_tier").is_err());
    }

    // ── R2-AC-7 ─────────────────────────────────────────────────
    #[test]
    fn test_router_is_pure_function() {
        let router = RuleBasedRouter::new(table());
        let ctx = ctx_normal("debug the test failure");
        let first = router.route(&ctx);
        for _ in 0..1000 {
            let next = router.route(&ctx);
            assert_eq!(first, next, "router must be a pure function");
        }
    }

    // ── R2-AC-8 ─────────────────────────────────────────────────
    #[test]
    fn test_rule_router_fallback_returns_sibling_provider_when_configured() {
        let mut t = table();
        t.insert(
            "default_alt",
            ModelChoice::new("openrouter", "sonnet-mirror", 8192),
        );
        let router = RuleBasedRouter::new(t);
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let alt = router
            .fallback(&previous, RoutingFailureHint::RateLimit)
            .expect("rate limit must produce a sibling when default_alt is configured");
        assert_eq!(alt.provider_id, "openrouter");
        assert_eq!(alt.model_id, "sonnet-mirror");
    }

    // ── Bonus: fallback semantics ───────────────────────────────
    #[test]
    fn fallback_budget_exhausted_returns_none() {
        let router = RuleBasedRouter::new(table());
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        assert!(
            router
                .fallback(&previous, RoutingFailureHint::BudgetExhausted)
                .is_none()
        );
    }

    #[test]
    fn fallback_context_overflow_escalates_to_strong() {
        let router = RuleBasedRouter::new(table());
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let strong = router
            .fallback(&previous, RoutingFailureHint::ContextOverflow)
            .expect("overflow should escalate");
        assert_eq!(strong.model_id, "opus");
    }

    #[test]
    fn subagent_phase_uses_role_slot() {
        let router = RuleBasedRouter::new(table());
        let ctx = RoutingContext::new(RoutingPhase::subagent(SubAgentRoleId::EXPLORER));
        let choice = router.route(&ctx);
        assert_eq!(choice.model_id, "haiku");
        assert_eq!(choice.routing_reason, "phase_subagent");
    }

    #[test]
    fn unconfigured_subagent_falls_back_to_default() {
        let router = RuleBasedRouter::new(table());
        let ctx = RoutingContext::new(RoutingPhase::subagent(SubAgentRoleId::REVIEWER));
        let choice = router.route(&ctx);
        assert_eq!(choice.model_id, "sonnet");
    }

    #[test]
    fn is_simple_turn_rejects_long_prompts() {
        let long = "a ".repeat(100);
        assert!(!RuleBasedRouter::is_simple_turn(&long));
    }
}
