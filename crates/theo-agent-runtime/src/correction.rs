use std::sync::Arc;

use theo_domain::event::{DomainEvent, EventType};
use theo_domain::retry_policy::CorrectionStrategy;
use theo_domain::tool_call::ToolCallState;

use crate::event_bus::EventBus;

/// Engine that selects and applies correction strategies for failed operations.
///
/// Every correction must reduce at least one uncertainty:
/// - Scope (narrow the problem)
/// - Error (fix the specific issue)
/// - Data (gather missing information)
///
/// If a correction cannot reduce uncertainty → abort or escalate.
pub struct CorrectionEngine {
    event_bus: Arc<EventBus>,
}

impl CorrectionEngine {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }

    /// Selects a correction strategy based on failure characteristics.
    ///
    /// Rules:
    /// - attempt <= 1 and transient failure → RetryLocal
    /// - semantic failure (wrong output, not error) → Replan
    /// - attempt > max_local_retries → Subtask (break down the problem)
    /// - multiple replans without progress → AgentSwap
    pub fn select_strategy(
        &self,
        status: ToolCallState,
        attempt: u32,
        max_local_retries: u32,
        replans_without_progress: u32,
    ) -> CorrectionStrategy {
        // Too many replans → agent swap
        if replans_without_progress >= 3 {
            return CorrectionStrategy::AgentSwap;
        }

        // Exhausted local retries → subtask
        if attempt > max_local_retries {
            return CorrectionStrategy::Subtask;
        }

        // Transient failure → retry
        match status {
            ToolCallState::Timeout | ToolCallState::Failed => {
                if attempt <= 1 {
                    CorrectionStrategy::RetryLocal
                } else {
                    CorrectionStrategy::Replan
                }
            }
            _ => CorrectionStrategy::Replan,
        }
    }

    /// Records the correction decision as an event.
    pub fn record_correction(&self, entity_id: &str, strategy: CorrectionStrategy, reason: &str) {
        self.event_bus.publish(DomainEvent::new(
            EventType::Error,
            entity_id,
            serde_json::json!({
                "type": "correction_applied",
                "strategy": format!("{}", strategy),
                "reason": reason,
            }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::CapturingListener;

    fn setup() -> (CorrectionEngine, Arc<CapturingListener>) {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());
        let engine = CorrectionEngine::new(bus);
        (engine, listener)
    }

    #[test]
    fn transient_failure_first_attempt_returns_retry_local() {
        let (engine, _) = setup();
        let strategy = engine.select_strategy(ToolCallState::Failed, 0, 2, 0);
        assert_eq!(strategy, CorrectionStrategy::RetryLocal);
    }

    #[test]
    fn transient_failure_second_attempt_returns_replan() {
        let (engine, _) = setup();
        let strategy = engine.select_strategy(ToolCallState::Failed, 2, 2, 0);
        assert_eq!(strategy, CorrectionStrategy::Replan);
    }

    #[test]
    fn timeout_first_attempt_returns_retry_local() {
        let (engine, _) = setup();
        let strategy = engine.select_strategy(ToolCallState::Timeout, 1, 3, 0);
        assert_eq!(strategy, CorrectionStrategy::RetryLocal);
    }

    #[test]
    fn exhausted_retries_returns_subtask() {
        let (engine, _) = setup();
        let strategy = engine.select_strategy(ToolCallState::Failed, 4, 3, 0);
        assert_eq!(strategy, CorrectionStrategy::Subtask);
    }

    #[test]
    fn many_replans_returns_agent_swap() {
        let (engine, _) = setup();
        let strategy = engine.select_strategy(ToolCallState::Failed, 0, 3, 3);
        assert_eq!(strategy, CorrectionStrategy::AgentSwap);
    }

    #[test]
    fn record_correction_publishes_event() {
        let (engine, listener) = setup();
        engine.record_correction("run-1", CorrectionStrategy::Replan, "semantic error");

        let events = listener.captured();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Error);
        assert_eq!(events[0].payload["type"], "correction_applied");
        assert_eq!(events[0].payload["strategy"], "Replan");
        assert_eq!(events[0].payload["reason"], "semantic error");
    }

    #[test]
    fn succeeded_status_returns_replan() {
        // Edge case: if called with non-failure status, default to replan
        let (engine, _) = setup();
        let strategy = engine.select_strategy(ToolCallState::Succeeded, 0, 3, 0);
        assert_eq!(strategy, CorrectionStrategy::Replan);
    }
}
