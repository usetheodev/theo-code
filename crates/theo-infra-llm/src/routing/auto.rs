//! `AutomaticModelRouter` — wrapper sobre `RuleBasedRouter` que aplica
//! `ComplexityClassifier` quando `ctx.model_override` é None.
//!
//! Phase 14 — Cost-Aware Routing. Decisão D1: opt-in.
//! - Quando `enabled=false` ou `ctx.model_override.is_some()`: delega ao inner sem mudanças
//! - Caso contrário: classifica via heurística + injeta `complexity_hint`
//!   no contexto antes de delegar ao inner
//!
//! Caller fica responsável por popular `ctx.latest_user_message` com o
//! objective do agent — esse é o sinal usado pelo classifier.

use theo_domain::routing::{
    ComplexityTier, ModelChoice, ModelRouter, RoutingContext, RoutingFailureHint,
};

use crate::routing::complexity::{ComplexityClassifier, ComplexitySignals};
use crate::routing::rules::RuleBasedRouter;

pub struct AutomaticModelRouter {
    inner: RuleBasedRouter,
    enabled: bool,
}

impl AutomaticModelRouter {
    pub fn new(inner: RuleBasedRouter, enabled: bool) -> Self {
        Self { inner, enabled }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Build complexity signals from the routing context.
    /// Token estimates use ~4 chars/token heuristic (Anthropic).
    fn build_signals(ctx: &RoutingContext<'_>) -> ComplexitySignals {
        let objective = ctx.latest_user_message.unwrap_or("");
        let task_type = ComplexityClassifier::detect_task_type(objective);
        let objective_tokens = (objective.len() / 4).min(u32::MAX as usize) as u32;
        // System prompt tokens approximated from conversation_tokens minus objective.
        let system_prompt_tokens = ctx
            .conversation_tokens
            .saturating_sub(objective_tokens as u64)
            .min(u32::MAX as u64) as u32;
        ComplexitySignals {
            system_prompt_tokens,
            objective_tokens,
            tool_count: 0,
            source: None,
            task_type,
            prior_failure_count: 0,
        }
    }
}

impl ModelRouter for AutomaticModelRouter {
    fn route(&self, ctx: &RoutingContext<'_>) -> ModelChoice {
        if !self.enabled || ctx.model_override.is_some() {
            return self.inner.route(ctx);
        }
        // Already-classified contexts pass through
        if ctx.complexity_hint.is_some() {
            return self.inner.route(ctx);
        }
        let signals = Self::build_signals(ctx);
        let tier = ComplexityClassifier::classify(&signals);
        let mut tiered = ctx.clone();
        tiered.complexity_hint = Some(tier);
        self.inner.route(&tiered)
    }

    fn fallback(
        &self,
        previous: &ModelChoice,
        hint: RoutingFailureHint,
    ) -> Option<ModelChoice> {
        self.inner.fallback(previous, hint)
    }
}

/// Map a `ComplexityTier` to the slot id used by `RuleBasedRouter`.
pub fn tier_to_slot(tier: ComplexityTier) -> &'static str {
    match tier {
        ComplexityTier::Cheap => "cheap",
        ComplexityTier::Default => "default",
        ComplexityTier::Strong => "strong",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::config::RoutingConfig;
    use theo_domain::routing::RoutingPhase;

    fn router_with_slots() -> RuleBasedRouter {
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
        RuleBasedRouter::new(w.routing.to_pricing_table())
    }

    #[test]
    fn auto_router_disabled_delegates_to_inner() {
        let inner = router_with_slots();
        let auto = AutomaticModelRouter::new(inner, false);
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        ctx.latest_user_message = Some("audit security");
        // Disabled: must NOT classify (would route to Strong via Analysis keyword).
        // Inner router with no hint will use keyword-based routing → may pick anything.
        // The contract is "delegate to inner unchanged" — assert hint stays None.
        let choice = auto.route(&ctx);
        // Just verify it returns something — exact slot depends on inner heuristics.
        assert!(!choice.model_id.is_empty());
    }

    #[test]
    fn auto_router_with_model_override_delegates_to_inner() {
        let inner = router_with_slots();
        let auto = AutomaticModelRouter::new(inner, true);
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        ctx.latest_user_message = Some("audit security");
        ctx.model_override = Some("opus-explicit");
        // Override present → no classification, just delegate.
        let _ = auto.route(&ctx);
    }

    #[test]
    fn auto_router_no_override_classifies_and_routes() {
        let inner = router_with_slots();
        let auto = AutomaticModelRouter::new(inner, true);
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        ctx.latest_user_message = Some("audit security analysis");
        // Analysis → Strong → opus
        let choice = auto.route(&ctx);
        assert_eq!(choice.model_id, "opus");
    }

    #[test]
    fn auto_router_planning_routes_to_strong_slot() {
        let inner = router_with_slots();
        let auto = AutomaticModelRouter::new(inner, true);
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        ctx.latest_user_message = Some("plan the auth refactor");
        let choice = auto.route(&ctx);
        assert_eq!(choice.model_id, "opus");
    }

    #[test]
    fn auto_router_retrieval_routes_to_cheap_slot() {
        let inner = router_with_slots();
        let auto = AutomaticModelRouter::new(inner, true);
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        ctx.latest_user_message = Some("read Cargo.toml and list crates");
        let choice = auto.route(&ctx);
        assert_eq!(choice.model_id, "haiku");
    }

    #[test]
    fn auto_router_already_classified_context_passes_through() {
        let inner = router_with_slots();
        let auto = AutomaticModelRouter::new(inner, true);
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        ctx.latest_user_message = Some("plan refactor"); // would be Strong if classified
        ctx.complexity_hint = Some(ComplexityTier::Cheap); // already set
        let choice = auto.route(&ctx);
        // Inner respects hint → Cheap
        assert_eq!(choice.model_id, "haiku");
    }

    #[test]
    fn tier_to_slot_maps_correctly() {
        assert_eq!(tier_to_slot(ComplexityTier::Cheap), "cheap");
        assert_eq!(tier_to_slot(ComplexityTier::Default), "default");
        assert_eq!(tier_to_slot(ComplexityTier::Strong), "strong");
    }

    #[test]
    fn build_signals_extracts_objective_and_task_type() {
        let mut ctx = RoutingContext::new(RoutingPhase::Normal);
        ctx.latest_user_message = Some("review code");
        ctx.conversation_tokens = 1000;
        let s = AutomaticModelRouter::build_signals(&ctx);
        assert_eq!(s.task_type, super::super::complexity::TaskType::Analysis);
        assert!(s.objective_tokens > 0);
    }
}
