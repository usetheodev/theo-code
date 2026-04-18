use std::sync::Arc;
use std::time::Instant;

use theo_domain::budget::{Budget, BudgetUsage, BudgetViolation};
use theo_domain::event::{DomainEvent, EventType};

use crate::event_bus::EventBus;

/// Enforces budget limits during agent execution.
///
/// Invariant 8: no execution can run without budget limits.
///
/// Tracks wall-clock time via Instant, and tokens/iterations/tool_calls
/// via explicit recording. Publishes BudgetExceeded event on violation.
pub struct BudgetEnforcer {
    budget: Budget,
    tokens_used: u64,
    iterations_used: usize,
    tool_calls_used: usize,
    start_time: Instant,
    event_bus: Arc<EventBus>,
    entity_id: String,
}

impl BudgetEnforcer {
    pub fn new(budget: Budget, event_bus: Arc<EventBus>, entity_id: impl Into<String>) -> Self {
        Self {
            budget,
            tokens_used: 0,
            iterations_used: 0,
            tool_calls_used: 0,
            start_time: Instant::now(),
            event_bus,
            entity_id: entity_id.into(),
        }
    }

    /// Checks if budget limits are exceeded.
    ///
    /// Returns Ok(()) if within limits, Err(BudgetViolation) if exceeded.
    /// Publishes BudgetExceeded event on violation.
    pub fn check(&self) -> Result<(), BudgetViolation> {
        let usage = self.usage();
        if let Some(violation) = usage.exceeds(&self.budget) {
            self.event_bus.publish(DomainEvent::new(
                EventType::BudgetExceeded,
                &self.entity_id,
                serde_json::json!({
                    "violation": format!("{}", violation),
                    "usage": {
                        "elapsed_secs": usage.elapsed_secs,
                        "tokens_used": usage.tokens_used,
                        "iterations_used": usage.iterations_used,
                        "tool_calls_used": usage.tool_calls_used,
                    },
                }),
            ));
            Err(violation)
        } else {
            Ok(())
        }
    }

    pub fn record_tokens(&mut self, tokens: u64) {
        self.tokens_used += tokens;
    }

    pub fn record_iteration(&mut self) {
        self.iterations_used += 1;
    }

    pub fn record_tool_call(&mut self) {
        self.tool_calls_used += 1;
    }

    /// Returns current usage snapshot.
    pub fn usage(&self) -> BudgetUsage {
        BudgetUsage {
            elapsed_secs: self.start_time.elapsed().as_secs(),
            tokens_used: self.tokens_used,
            iterations_used: self.iterations_used,
            tool_calls_used: self.tool_calls_used,
        }
    }

    /// Returns remaining budget (clamped to 0).
    pub fn remaining(&self) -> Budget {
        let usage = self.usage();
        Budget {
            max_time_secs: self.budget.max_time_secs.saturating_sub(usage.elapsed_secs),
            max_tokens: self.budget.max_tokens.saturating_sub(usage.tokens_used),
            max_iterations: self
                .budget
                .max_iterations
                .saturating_sub(usage.iterations_used),
            max_tool_calls: self
                .budget
                .max_tool_calls
                .saturating_sub(usage.tool_calls_used),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::CapturingListener;

    fn setup(budget: Budget) -> (BudgetEnforcer, Arc<CapturingListener>) {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());
        let enforcer = BudgetEnforcer::new(budget, bus, "test-run");
        (enforcer, listener)
    }

    #[test]
    fn check_passes_when_under_budget() {
        let (enforcer, _) = setup(Budget::default());
        assert!(enforcer.check().is_ok());
    }

    #[test]
    fn check_fails_iterations_exceeded() {
        let budget = Budget {
            max_iterations: 2,
            ..Budget::default()
        };
        let (mut enforcer, _) = setup(budget);
        enforcer.record_iteration();
        enforcer.record_iteration();
        assert!(enforcer.check().is_ok()); // 2 == limit, not exceeded
        enforcer.record_iteration();
        let err = enforcer.check().unwrap_err();
        assert!(matches!(
            err,
            BudgetViolation::IterationsExceeded {
                limit: 2,
                actual: 3
            }
        ));
    }

    #[test]
    fn check_fails_tokens_exceeded() {
        let budget = Budget {
            max_tokens: 100,
            ..Budget::default()
        };
        let (mut enforcer, _) = setup(budget);
        enforcer.record_tokens(50);
        assert!(enforcer.check().is_ok());
        enforcer.record_tokens(60);
        let err = enforcer.check().unwrap_err();
        assert!(matches!(
            err,
            BudgetViolation::TokensExceeded {
                limit: 100,
                actual: 110
            }
        ));
    }

    #[test]
    fn check_fails_tool_calls_exceeded() {
        let budget = Budget {
            max_tool_calls: 1,
            ..Budget::default()
        };
        let (mut enforcer, _) = setup(budget);
        enforcer.record_tool_call();
        assert!(enforcer.check().is_ok());
        enforcer.record_tool_call();
        let err = enforcer.check().unwrap_err();
        assert!(matches!(
            err,
            BudgetViolation::ToolCallsExceeded {
                limit: 1,
                actual: 2
            }
        ));
    }

    #[test]
    fn check_fails_time_exceeded() {
        // Use max_time_secs=0 to guarantee immediate violation.
        // elapsed().as_secs() rounds down, so sleep 1.1s to guarantee > 0.
        // Instead, we test via BudgetUsage directly (deterministic).
        let budget = Budget {
            max_time_secs: 5,
            ..Budget::default()
        };
        let usage = BudgetUsage {
            elapsed_secs: 6,
            tokens_used: 0,
            iterations_used: 0,
            tool_calls_used: 0,
        };
        let violation = usage.exceeds(&budget).unwrap();
        assert!(matches!(
            violation,
            BudgetViolation::TimeExceeded {
                limit: 5,
                actual: 6
            }
        ));
    }

    #[test]
    fn record_tokens_accumulates() {
        let (mut enforcer, _) = setup(Budget::default());
        enforcer.record_tokens(100);
        enforcer.record_tokens(200);
        assert_eq!(enforcer.usage().tokens_used, 300);
    }

    #[test]
    fn record_iteration_increments() {
        let (mut enforcer, _) = setup(Budget::default());
        enforcer.record_iteration();
        enforcer.record_iteration();
        enforcer.record_iteration();
        assert_eq!(enforcer.usage().iterations_used, 3);
    }

    #[test]
    fn record_tool_call_increments() {
        let (mut enforcer, _) = setup(Budget::default());
        enforcer.record_tool_call();
        assert_eq!(enforcer.usage().tool_calls_used, 1);
    }

    #[test]
    fn remaining_clamps_to_zero() {
        let budget = Budget {
            max_tokens: 100,
            max_iterations: 5,
            ..Budget::default()
        };
        let (mut enforcer, _) = setup(budget);
        enforcer.record_tokens(150); // over limit
        enforcer.record_iteration();
        let remaining = enforcer.remaining();
        assert_eq!(remaining.max_tokens, 0); // clamped, not underflow
        assert_eq!(remaining.max_iterations, 4);
    }

    #[test]
    fn budget_exceeded_event_published_on_violation() {
        let budget = Budget {
            max_iterations: 1,
            ..Budget::default()
        };
        let (mut enforcer, listener) = setup(budget);
        enforcer.record_iteration();
        enforcer.record_iteration();
        let _ = enforcer.check();

        let events = listener.captured();
        let exceeded: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::BudgetExceeded)
            .collect();
        assert_eq!(exceeded.len(), 1);
        assert_eq!(exceeded[0].entity_id, "test-run");
        assert!(
            exceeded[0].payload["violation"]
                .as_str()
                .unwrap()
                .contains("iterations exceeded")
        );
    }
}
