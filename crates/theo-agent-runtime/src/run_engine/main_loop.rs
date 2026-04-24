//! Main-loop side helpers — `AgentRunEngine` methods that support
//! the `for iteration { ... }` body without owning it yet.
//!
//! Fase 4 (REMEDIATION_PLAN T4.2). Start of the main-loop extraction.
//! This module currently hosts:
//!   - `choose_model` — routing decision + panic-safe fallback
//!   - `handle_context_overflow` — emergency compaction + event
//!
//! Future iterations will move the full loop body here, decomposed
//! into `prepare_iteration`, `call_llm`, `dispatch_tools`, and
//! `finalize_iteration` methods.

use theo_domain::agent_run::RunState;
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::task::TaskState;
use theo_infra_llm::LlmError;
use theo_infra_llm::types::Message;

use super::AgentRunEngine;
use crate::agent_loop::AgentResult;
use crate::run_engine_helpers::llm_error_to_class;

/// Routing decision: `(chosen_model, chosen_effort, routing_reason)`.
pub(super) type RoutingDecision = (String, Option<String>, &'static str);

impl AgentRunEngine {
    /// Consult the configured router (if any) to pick the model and
    /// reasoning effort for this turn. Panic-safe: if the router's
    /// `route()` panics, falls back to the session defaults and
    /// tags the decision as `"router_panic_fallback_default"`.
    ///
    /// The router is expected to be called exactly once per turn at
    /// the single site inside the main loop — this method preserves
    /// that invariant.
    pub(super) fn choose_model(
        &self,
        messages: &[Message],
        iteration: usize,
        estimated_context_tokens: u64,
        requires_tool_use: bool,
    ) -> RoutingDecision {
        let latest_user_msg = messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, theo_infra_llm::types::Role::User))
            .and_then(|m| m.content.as_deref());
        let mut routing_ctx = theo_domain::routing::RoutingContext::new(
            theo_domain::routing::RoutingPhase::Normal,
        );
        routing_ctx.latest_user_message = latest_user_msg;
        routing_ctx.conversation_tokens = estimated_context_tokens;
        routing_ctx.iteration = iteration;
        routing_ctx.requires_tool_use = requires_tool_use;

        match &self.config.router {
            Some(handle) => {
                let choice = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handle.as_router().route(&routing_ctx)
                }));
                match choice {
                    Ok(c) => (c.model_id, c.reasoning_effort, c.routing_reason),
                    Err(_) => (
                        self.config.model.clone(),
                        self.config.reasoning_effort.clone(),
                        "router_panic_fallback_default",
                    ),
                }
            }
            None => (
                self.config.model.clone(),
                self.config.reasoning_effort.clone(),
                "no_router",
            ),
        }
    }

    /// Build the abort `AgentResult` for a non-retryable LLM failure.
    /// Handles state-machine transitions, metrics bump, and ErrorClass
    /// mapping. The caller `return`s this from the main loop.
    pub(super) fn build_llm_abort_result(&mut self, err: &LlmError) -> AgentResult {
        self.transition_run(RunState::Aborted);
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Failed);
        self.metrics.record_run_complete(false);
        let class = llm_error_to_class(err);
        AgentResult::from_engine_state(
            self,
            false,
            format!("LLM error: {err}"),
            false,
            class,
        )
    }

    /// Reactive context-overflow recovery. Invoked when the LLM call
    /// returned `LlmError::ContextOverflow`. Takes a snapshot of hot
    /// files (FM-6 sensor), emits a `ContextOverflowRecovery` event,
    /// and compacts `messages` to `EMERGENCY_COMPACT_RATIO` of the
    /// context window. The caller is expected to `continue` to the
    /// next main-loop iteration.
    pub(super) fn handle_context_overflow(
        &mut self,
        err: &LlmError,
        messages: &mut Vec<Message>,
    ) {
        // Snapshot hot files BEFORE compaction destroys them (FM-6).
        for f in &self.working_set.hot_files {
            self.pre_compaction_hot_files.insert(f.clone());
        }

        self.event_bus.publish(DomainEvent::new(
            EventType::ContextOverflowRecovery,
            self.run.run_id.as_str(),
            serde_json::json!({
                "error": err.to_string(),
                "action": "emergency_compaction",
                "target_ratio": crate::constants::EMERGENCY_COMPACT_RATIO,
            }),
        ));

        // Emergency compaction: keep only a bounded fraction of context.
        let model_ctx = self.config.context_window_tokens;
        let target =
            (model_ctx as f64 * crate::constants::EMERGENCY_COMPACT_RATIO) as usize;
        let before_len = messages.len();
        crate::compaction::compact_messages_to_target(
            messages,
            target,
            "", // No task objective at this level
        );
        eprintln!(
            "[theo] Context overflow recovery: compacted {} → {} messages (target {})",
            before_len,
            messages.len(),
            target
        );
    }
}
