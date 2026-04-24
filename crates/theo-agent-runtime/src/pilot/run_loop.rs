//! Helpers for `PilotLoop::run` — extracted from the monolithic loop body
//! so each section (pre-loop guards, evolution recording, loop-summary
//! publication) can be read and tested as an independent step.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Every helper is an `impl PilotLoop`
//! method so it retains direct access to private fields (no API surface
//! change for external callers).

use std::sync::Arc;

use crate::agent_loop::AgentResult;
use crate::event_bus::EventBus;
use crate::pilot::{parse_fix_plan, EventForwarder, ExitReason, PilotLoop};
use theo_infra_llm::types::Message;

impl PilotLoop {
    /// Core pre-loop guard-check shared by `run` and `run_from_roadmap`:
    /// returns `Some(ExitReason)` when the pilot must terminate immediately.
    /// Order is intentional — cheapest atomic check first, rate-limit/CB
    /// state reads last.
    pub(super) fn check_core_guards(&mut self) -> Option<ExitReason> {
        if self.interrupted.load(std::sync::atomic::Ordering::Acquire) {
            return Some(ExitReason::UserInterrupt);
        }

        if self.pilot_config.max_total_calls > 0
            && self.loop_count >= self.pilot_config.max_total_calls
        {
            return Some(ExitReason::MaxCallsReached);
        }

        if !self.check_rate_limit() {
            return Some(ExitReason::RateLimitExhausted);
        }

        if let Some(reason) = self.check_circuit_breaker() {
            return Some(ExitReason::CircuitBreakerOpen(reason));
        }

        None
    }

    /// Full pre-loop guard-check for `run`: core guards + fix-plan completion.
    /// Roadmap mode uses `check_core_guards` directly (fix-plan file is
    /// irrelevant when tasks come from a roadmap).
    pub(super) fn check_pre_loop_guards(&mut self) -> Option<ExitReason> {
        if let Some(reason) = self.check_core_guards() {
            return Some(reason);
        }

        let (completed, total) = parse_fix_plan(&self.project_dir);
        if total > 0 && completed == total {
            return Some(ExitReason::FixPlanComplete);
        }

        None
    }

    /// Build the per-iteration loop bus with the parent-bus forwarder
    /// subscribed. The fresh bus isolates intra-iteration events so an
    /// iteration can be dropped without leaking listeners.
    pub(super) fn build_iteration_bus(&self) -> Arc<EventBus> {
        let loop_bus = Arc::new(EventBus::new());
        let forwarder = Arc::new(EventForwarder {
            target: self.parent_event_bus.clone(),
        });
        loop_bus.subscribe(forwarder);
        loop_bus
    }

    /// Record an exchange in the rotating session buffer (prompt → reply)
    /// and trim it to `MAX_SESSION_MESSAGES` if it overflows.
    pub(super) fn record_exchange(&mut self, task: &str, result: &AgentResult) {
        self.session_messages.push(Message::user(task));
        self.session_messages
            .push(Message::assistant(&result.summary));
        if self.session_messages.len() > super::MAX_SESSION_MESSAGES {
            let excess = self.session_messages.len() - super::MAX_SESSION_MESSAGES;
            self.session_messages.drain(..excess);
        }
    }

    /// Record the attempt in the evolution subsystem and, on failure, inject
    /// a reflection message into the next iteration's session history.
    /// Encapsulates the outcome-classification + strategy-selection logic
    /// that was inline in `run`.
    pub(super) fn record_evolution_attempt(&mut self, result: &AgentResult) {
        let outcome = if result.success {
            theo_domain::evolution::AttemptOutcome::Success
        } else if result.files_edited.iter().any(|f| !f.is_empty()) {
            theo_domain::evolution::AttemptOutcome::Partial
        } else {
            theo_domain::evolution::AttemptOutcome::Failure
        };

        let strategy = if self.loop_count <= 1 {
            theo_domain::retry_policy::CorrectionStrategy::RetryLocal
        } else if let Some(r) = self.evolution.reflections().last() {
            r.recommended_strategy
        } else {
            theo_domain::retry_policy::CorrectionStrategy::RetryLocal
        };

        self.evolution.record_attempt(
            strategy,
            outcome,
            result.files_edited.clone(),
            if result.success { None } else { Some(result.summary.clone()) },
            result.duration_ms,
            result.tokens_used,
        );

        if !result.success
            && let Some(reflection) = self.evolution.reflect()
        {
            self.session_messages.push(Message::system(format!(
                "## Evolution Reflection (after attempt {})\n\
                 **What failed:** {}\n\
                 **Why:** {}\n\
                 **Change strategy to:** {} — {}\n",
                reflection.prior_attempt,
                reflection.what_failed,
                reflection.why_it_failed,
                reflection.recommended_strategy,
                reflection.what_to_change,
            )));
        }
    }

    /// Publish a `RunStateChanged` event with a `PilotLoopComplete:…` payload
    /// so the CLI can render a per-iteration summary line.
    pub(super) fn publish_loop_summary(&self, result: &AgentResult) {
        self.parent_event_bus
            .publish(theo_domain::event::DomainEvent::new(
                theo_domain::event::EventType::RunStateChanged,
                "pilot",
                serde_json::json!({
                    "from": "Executing",
                    "to": format!(
                        "PilotLoopComplete:{}:{}:{}:{}",
                        self.loop_count,
                        result.files_edited.len(),
                        result.tokens_used,
                        result.iterations_used
                    ),
                }),
            ));
    }

    /// Track tokens + accumulate the per-iteration file-edits into the
    /// pilot-wide unique set. Empty strings are filtered defensively.
    pub(super) fn track_tokens_and_files(&mut self, result: &AgentResult) {
        self.total_tokens += result.tokens_used;
        for file in &result.files_edited {
            if !file.is_empty() && !self.total_files_edited.contains(file) {
                self.total_files_edited.push(file.clone());
            }
        }
    }
}
