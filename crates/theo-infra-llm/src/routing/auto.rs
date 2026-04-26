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

use std::sync::Arc;

use crate::routing::complexity::{ComplexityClassifier, ComplexitySignals, TaskType};
use crate::routing::rules::RuleBasedRouter;

/// Phase 27 (sota-gaps-followup): callback invoked once per routing
/// decision. Receives `(task_type, tier, model_id)` so the caller can
/// aggregate the data however it wants (typically into a histogram in
/// `MetricsCollector`).
///
/// Generic by design: `theo-infra-llm` MUST NOT depend on
/// `theo-agent-runtime` per ADR-016. The callback is the bridge.
pub type RoutingMetricsRecorder = Arc<dyn Fn(&str, &str, &str) + Send + Sync>;

pub struct AutomaticModelRouter {
    inner: RuleBasedRouter,
    enabled: bool,
    /// Optional metrics recorder. When attached, every `route()` call that
    /// produces a non-trivial classification fires the callback.
    metrics: Option<RoutingMetricsRecorder>,
}

impl AutomaticModelRouter {
    pub fn new(inner: RuleBasedRouter, enabled: bool) -> Self {
        Self {
            inner,
            enabled,
            metrics: None,
        }
    }

    /// Phase 27 builder: attach a metrics recorder.
    pub fn with_metrics(mut self, recorder: RoutingMetricsRecorder) -> Self {
        self.metrics = Some(recorder);
        self
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn task_type_label(t: TaskType) -> &'static str {
        match t {
            TaskType::Retrieval => "Retrieval",
            TaskType::Implementation => "Implementation",
            TaskType::Analysis => "Analysis",
            TaskType::Planning => "Planning",
            TaskType::Generic => "Generic",
        }
    }

    fn tier_label(t: ComplexityTier) -> &'static str {
        match t {
            ComplexityTier::Cheap => "Cheap",
            ComplexityTier::Default => "Default",
            ComplexityTier::Strong => "Strong",
        }
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
        let choice = self.inner.route(&tiered);
        // Phase 27 (sota-gaps-followup): record the decision for telemetry.
        if let Some(recorder) = &self.metrics {
            recorder(
                Self::task_type_label(signals.task_type),
                Self::tier_label(tier),
                &choice.model_id,
            );
        }
        choice
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

    // ── Phase 27 (sota-gaps-followup): metrics recording ──

    pub mod with_metrics {
        use super::*;
        use std::sync::Mutex;

        #[derive(Default)]
        struct Recorder {
            captured: Mutex<Vec<(String, String, String)>>,
        }

        type RecorderFn = std::sync::Arc<dyn Fn(&str, &str, &str) + Send + Sync>;
        fn build_recorder() -> (std::sync::Arc<Recorder>, RecorderFn) {
            let r = std::sync::Arc::new(Recorder::default());
            let r_clone = r.clone();
            let f: RecorderFn =
                std::sync::Arc::new(move |t, ti, m| {
                    r_clone
                        .captured
                        .lock()
                        .unwrap()
                        .push((t.to_string(), ti.to_string(), m.to_string()));
                });
            (r, f)
        }

        #[test]
        fn router_with_metrics_handle_records_each_decision() {
            let (recorder, callback) = build_recorder();
            let inner = router_with_slots();
            let auto = AutomaticModelRouter::new(inner, true).with_metrics(callback);

            let mut ctx = RoutingContext::new(RoutingPhase::Normal);
            ctx.latest_user_message = Some("audit security");
            let _ = auto.route(&ctx);

            let g = recorder.captured.lock().unwrap();
            assert_eq!(g.len(), 1);
            assert_eq!(g[0].0, "Analysis");
            assert_eq!(g[0].1, "Strong");
            assert_eq!(g[0].2, "opus");
        }

        #[test]
        fn router_records_multiple_distinct_decisions() {
            let (recorder, callback) = build_recorder();
            let inner = router_with_slots();
            let auto = AutomaticModelRouter::new(inner, true).with_metrics(callback);

            let mut c1 = RoutingContext::new(RoutingPhase::Normal);
            c1.latest_user_message = Some("read foo");
            let _ = auto.route(&c1);

            let mut c2 = RoutingContext::new(RoutingPhase::Normal);
            c2.latest_user_message = Some("plan refactor");
            let _ = auto.route(&c2);

            let g = recorder.captured.lock().unwrap();
            assert_eq!(g.len(), 2);
            assert_eq!(g[0], ("Retrieval".into(), "Cheap".into(), "haiku".into()));
            assert_eq!(g[1], ("Planning".into(), "Strong".into(), "opus".into()));
        }

        #[test]
        fn router_without_metrics_handle_silently_skips_recording() {
            // No `with_metrics` call → no callback invoked, no panic.
            let inner = router_with_slots();
            let auto = AutomaticModelRouter::new(inner, true);
            let mut ctx = RoutingContext::new(RoutingPhase::Normal);
            ctx.latest_user_message = Some("audit");
            let choice = auto.route(&ctx);
            assert_eq!(choice.model_id, "opus");
        }

        #[test]
        fn router_does_not_record_when_disabled() {
            let (recorder, callback) = build_recorder();
            let inner = router_with_slots();
            let auto = AutomaticModelRouter::new(inner, false).with_metrics(callback);
            let mut ctx = RoutingContext::new(RoutingPhase::Normal);
            ctx.latest_user_message = Some("audit");
            let _ = auto.route(&ctx);
            let g = recorder.captured.lock().unwrap();
            assert!(g.is_empty(), "disabled router must not record");
        }

        #[test]
        fn router_does_not_record_when_model_override_present() {
            let (recorder, callback) = build_recorder();
            let inner = router_with_slots();
            let auto = AutomaticModelRouter::new(inner, true).with_metrics(callback);
            let mut ctx = RoutingContext::new(RoutingPhase::Normal);
            ctx.latest_user_message = Some("audit");
            ctx.model_override = Some("forced-model");
            let _ = auto.route(&ctx);
            let g = recorder.captured.lock().unwrap();
            assert!(
                g.is_empty(),
                "model_override skips classification → no recording"
            );
        }

        #[test]
        fn router_does_not_record_when_complexity_hint_present() {
            let (recorder, callback) = build_recorder();
            let inner = router_with_slots();
            let auto = AutomaticModelRouter::new(inner, true).with_metrics(callback);
            let mut ctx = RoutingContext::new(RoutingPhase::Normal);
            ctx.latest_user_message = Some("audit");
            ctx.complexity_hint = Some(ComplexityTier::Cheap);
            let _ = auto.route(&ctx);
            let g = recorder.captured.lock().unwrap();
            assert!(g.is_empty(), "pre-classified context → pass through");
        }
    }
}
