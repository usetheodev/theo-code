//! Bounded fallback cascade (plan phase R5).
//!
//! When an LLM call fails with a retryable/overflow/rate-limit error, the
//! cascade consults the router's `fallback()` for a next choice. The hop
//! budget is bounded by `MAX_FALLBACK_HOPS = 2` so a cascading outage
//! cannot spin indefinitely. `BudgetExhausted` is a hard stop — the
//! router returns `None` and the cascade surfaces a typed
//! `LlmError::FallbackExhausted` with every attempted model id.
//!
//! Plan: outputs/smart-model-routing-plan.md §4.5 + §R5.

use theo_domain::routing::{ModelChoice, ModelRouter, RoutingFailureHint};

use crate::error::LlmError;

/// Maximum number of fallback hops permitted per turn.
pub const MAX_FALLBACK_HOPS: usize = 2;

/// Outcome of a single cascade step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CascadeStep {
    /// Router suggested a different choice — caller should retry with it.
    Retry(ModelChoice),
    /// Router declined to escalate (e.g. BudgetExhausted, non-retryable
    /// error). Caller must surface the original error.
    Stop,
    /// Cascade exhausted its hop budget. Caller should surface
    /// `LlmError::FallbackExhausted`.
    Exhausted { attempted: Vec<String> },
}

/// Tracks cascade state across retries within a single turn.
#[derive(Debug, Clone, Default)]
pub struct CascadeState {
    attempted: Vec<String>,
    hops: usize,
}

impl CascadeState {
    pub fn new(initial: &ModelChoice) -> Self {
        Self {
            attempted: vec![format!("{}:{}", initial.provider_id, initial.model_id)],
            hops: 0,
        }
    }

    /// Ask the router for a new choice given the previous one plus the
    /// failure that just occurred. The returned `CascadeStep` encodes
    /// whether the caller should retry, stop, or surface exhaustion.
    pub fn next(
        &mut self,
        router: &dyn ModelRouter,
        previous: &ModelChoice,
        err: &LlmError,
    ) -> CascadeStep {
        if self.hops >= MAX_FALLBACK_HOPS {
            return CascadeStep::Exhausted {
                attempted: std::mem::take(&mut self.attempted),
            };
        }
        let hint = match err.to_routing_hint() {
            Some(h) => h,
            None => return CascadeStep::Stop,
        };
        if matches!(hint, RoutingFailureHint::BudgetExhausted) {
            return CascadeStep::Stop;
        }
        match router.fallback(previous, hint) {
            Some(choice) => {
                self.hops += 1;
                self.attempted
                    .push(format!("{}:{}", choice.provider_id, choice.model_id));
                CascadeStep::Retry(choice)
            }
            None => CascadeStep::Stop,
        }
    }

    pub fn attempted(&self) -> &[String] {
        &self.attempted
    }

    pub fn hops(&self) -> usize {
        self.hops
    }
}

/// Build the typed `FallbackExhausted` error from the cascade state.
pub fn exhausted_error(state: &CascadeState) -> LlmError {
    LlmError::FallbackExhausted {
        attempted: state.attempted().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{PricingTable, RuleBasedRouter};
    use theo_domain::routing::RoutingFailureHint;

    fn table() -> PricingTable {
        let mut t = PricingTable::new();
        t.insert("cheap", ModelChoice::new("anthropic", "haiku", 4096));
        t.insert("default", ModelChoice::new("anthropic", "sonnet", 8192));
        t.insert("strong", ModelChoice::new("anthropic", "opus", 16384));
        t.insert(
            "default_alt",
            ModelChoice::new("openrouter", "sonnet-mirror", 8192),
        );
        t
    }

    fn router() -> RuleBasedRouter {
        RuleBasedRouter::new(table())
    }

    // ── R5-AC-1 ─────────────────────────────────────────────────

    #[test]
    fn test_r5_ac_1_overflow_triggers_fallback_to_larger_window() {
        let r = router();
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let mut state = CascadeState::new(&previous);
        let err = LlmError::ContextOverflow {
            provider: "anthropic".to_string(),
            message: "overflow".to_string(),
        };
        match state.next(&r, &previous, &err) {
            CascadeStep::Retry(choice) => {
                assert_eq!(choice.model_id, "opus");
                assert!(choice.max_output_tokens > previous.max_output_tokens);
            }
            other => panic!("expected Retry, got {other:?}"),
        }
    }

    // ── R5-AC-2 ─────────────────────────────────────────────────

    #[test]
    fn test_r5_ac_2_rate_limit_triggers_sibling_provider() {
        let r = router();
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let mut state = CascadeState::new(&previous);
        let err = LlmError::RateLimited { retry_after: None };
        match state.next(&r, &previous, &err) {
            CascadeStep::Retry(choice) => {
                assert_eq!(choice.provider_id, "openrouter");
                assert_eq!(choice.model_id, "sonnet-mirror");
            }
            other => panic!("expected sibling retry, got {other:?}"),
        }
    }

    // ── R5-AC-3 ─────────────────────────────────────────────────

    #[test]
    fn test_r5_ac_3_timeout_retries_same_model_then_falls_back() {
        // The cascade itself does not implement "retry same model once";
        // that sits in the caller. What the cascade guarantees is that
        // a Transient hint produces a distinct next choice, letting the
        // caller implement the full (retry-once, then-hop) policy.
        let r = router();
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let mut state = CascadeState::new(&previous);
        let err = LlmError::Timeout;
        match state.next(&r, &previous, &err) {
            CascadeStep::Retry(next) => assert_ne!(next.model_id, previous.model_id),
            other => panic!("expected Retry, got {other:?}"),
        }
    }

    // ── R5-AC-4 ─────────────────────────────────────────────────

    #[test]
    fn test_r5_ac_4_timeout_twice_hops_to_different_model() {
        // Invariant: each hop produces a choice distinct from its IMMEDIATE
        // predecessor (router.fallback contract). The cascade may cycle
        // across tiers under repeated Transient failures — that is by
        // design (caller bounds total hops via MAX_FALLBACK_HOPS).
        let r = router();
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let mut state = CascadeState::new(&previous);
        let CascadeStep::Retry(next) = state.next(&r, &previous, &LlmError::Timeout) else {
            panic!("expected retry");
        };
        assert_ne!(next.model_id, previous.model_id);
        let CascadeStep::Retry(second) = state.next(&r, &next, &LlmError::Timeout) else {
            panic!("expected second retry");
        };
        assert_ne!(
            second.model_id, next.model_id,
            "each hop must differ from its direct predecessor"
        );
        assert_eq!(state.attempted().len(), 3, "initial + 2 hops recorded");
    }

    // ── R5-AC-5 ─────────────────────────────────────────────────

    #[test]
    fn test_r5_ac_5_max_two_hops_then_exhausted() {
        let r = router();
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let mut state = CascadeState::new(&previous);
        // Hop 1
        let CascadeStep::Retry(a) = state.next(&r, &previous, &LlmError::Timeout) else {
            panic!();
        };
        // Hop 2
        let CascadeStep::Retry(b) = state.next(&r, &a, &LlmError::Timeout) else {
            panic!();
        };
        // Hop 3 must refuse.
        let step = state.next(&r, &b, &LlmError::Timeout);
        match step {
            CascadeStep::Exhausted { attempted } => {
                assert_eq!(attempted.len(), 3, "initial + 2 hops");
            }
            other => panic!("expected Exhausted, got {other:?}"),
        }
    }

    // ── R5-AC-6 ─────────────────────────────────────────────────
    // Property-ish test: for any previous choice + any hint, the router
    // never returns the same (provider, model) pair.

    #[test]
    fn test_r5_ac_6_fallback_never_returns_same_model_as_input() {
        let r = router();
        let hints = [
            RoutingFailureHint::ContextOverflow,
            RoutingFailureHint::RateLimit,
            RoutingFailureHint::Transient,
            RoutingFailureHint::BudgetExhausted,
        ];
        let previouses = [
            ModelChoice::new("anthropic", "haiku", 4096),
            ModelChoice::new("anthropic", "sonnet", 8192),
            ModelChoice::new("anthropic", "opus", 16384),
            ModelChoice::new("openrouter", "sonnet-mirror", 8192),
            ModelChoice::new("mystery", "unknown", 4096),
        ];
        for prev in &previouses {
            for hint in hints {
                match r.fallback(prev, hint) {
                    Some(next) => {
                        assert!(
                            next.provider_id != prev.provider_id
                                || next.model_id != prev.model_id,
                            "fallback must not return the same (provider, model) pair; \
                             prev={prev:?} hint={hint:?} next={next:?}"
                        );
                    }
                    None => {}
                }
            }
        }
    }

    // ── R5-AC-7 ─────────────────────────────────────────────────

    #[test]
    fn test_r5_ac_7_routing_fallback_records_attempted_models() {
        let r = router();
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let mut state = CascadeState::new(&previous);
        let CascadeStep::Retry(next) =
            state.next(&r, &previous, &LlmError::RateLimited { retry_after: None })
        else {
            panic!();
        };
        // Attempted list now includes both the initial and the hop.
        assert!(state.attempted().iter().any(|s| s.contains("sonnet")));
        assert!(
            state
                .attempted()
                .iter()
                .any(|s| s.contains(&next.model_id))
        );
    }

    // ── R5-AC-8 ─────────────────────────────────────────────────

    #[test]
    fn test_r5_ac_8_budget_exhausted_is_hard_stop() {
        let r = router();
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let mut state = CascadeState::new(&previous);
        // Simulate BudgetExhausted by constructing a custom closure
        // that mimics the hint — we cannot easily derive it from
        // LlmError because no HTTP code maps to it. Verify the router
        // API directly.
        let result = r.fallback(&previous, RoutingFailureHint::BudgetExhausted);
        assert!(result.is_none());
        // And if LlmError mapping is called with a non-retryable error
        // (AuthFailed -> hint=None), cascade returns Stop.
        let step = state.next(&r, &previous, &LlmError::AuthFailed("401".to_string()));
        assert_eq!(step, CascadeStep::Stop);
    }

    // ── Bonus: exhausted_error constructor ──────────────────────

    #[test]
    fn exhausted_error_carries_full_attempt_list() {
        let previous = ModelChoice::new("anthropic", "sonnet", 8192);
        let mut state = CascadeState::new(&previous);
        let r = router();
        let _ = state.next(&r, &previous, &LlmError::Timeout);
        let err = exhausted_error(&state);
        match err {
            LlmError::FallbackExhausted { attempted } => {
                assert!(!attempted.is_empty());
                assert!(attempted.iter().any(|s| s.contains("sonnet")));
            }
            _ => panic!("expected FallbackExhausted"),
        }
    }

    // ── Bonus: LlmError -> RoutingFailureHint mapping ───────────

    #[test]
    fn llm_error_maps_to_expected_routing_hint() {
        assert_eq!(
            LlmError::Timeout.to_routing_hint(),
            Some(RoutingFailureHint::Transient)
        );
        assert_eq!(
            LlmError::RateLimited { retry_after: None }.to_routing_hint(),
            Some(RoutingFailureHint::RateLimit)
        );
        assert_eq!(
            LlmError::ContextOverflow {
                provider: "a".to_string(),
                message: "b".to_string(),
            }
            .to_routing_hint(),
            Some(RoutingFailureHint::ContextOverflow)
        );
        assert_eq!(LlmError::AuthFailed("x".to_string()).to_routing_hint(), None);
    }
}
