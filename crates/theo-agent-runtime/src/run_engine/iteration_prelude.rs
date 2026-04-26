//! Iteration-prelude helpers — sensor drain + context-loop nudge +
//! staged compaction.
//!
//! Split out of `run_engine/main_loop.rs` (REMEDIATION_PLAN T4.* —
//! production-LOC trim toward the per-file 500-line target). Both
//! methods stay `pub(super)` on `impl AgentRunEngine`, so callers in
//! `main_loop.rs` are unchanged.

use theo_domain::event::{DomainEvent, EventType};
use theo_infra_llm::types::Message;

use super::AgentRunEngine;

impl AgentRunEngine {
    /// Drain pending sensor results and push them as system messages so
    /// the next LLM call sees computational-verification feedback (e.g.
    /// clippy / cargo test output). Emits `SensorExecuted` per result.
    pub(super) fn drain_sensor_messages(
        &self,
        sensor_runner: Option<&crate::sensor::SensorRunner>,
        messages: &mut Vec<Message>,
    ) {
        let Some(sensor_runner) = sensor_runner else {
            return;
        };
        for result in sensor_runner.drain_pending() {
            let severity = if result.exit_code == 0 { "OK" } else { "ISSUE" };
            let preview = theo_domain::prompt_sanitizer::char_boundary_truncate(
                &result.output,
                crate::constants::SENSOR_OUTPUT_PREVIEW_BYTES,
            );
            messages.push(Message::system(format!(
                "[SENSOR {severity}] {} (via {}): {preview}",
                result.file_path, result.tool_name
            )));
            self.event_bus.publish(DomainEvent::new(
                EventType::SensorExecuted,
                self.run.run_id.as_str(),
                serde_json::json!({
                    "file": result.file_path,
                    "exit_code": result.exit_code,
                    "output_preview": &preview[..preview.len().min(crate::constants::TOOL_PREVIEW_BYTES)],
                    "duration_ms": result.duration_ms,
                    "tool_name": result.tool_name,
                }),
            ));
        }
    }

    /// Iteration prelude: context-loop injection + compaction. Returns
    /// the post-compaction estimated token count so the caller can pass
    /// it through to the router.
    ///
    /// Ordering (do not change without re-verifying the characterization
    /// snapshots): context-loop nudge → phase transitions → pre-compress
    /// memory hook → staged compaction → metrics record.
    pub(super) async fn inject_context_loop_and_compact(
        &mut self,
        iteration: usize,
        messages: &mut Vec<Message>,
    ) -> usize {
        // Context loop injection — fires every N iterations.
        if iteration > 1
            && iteration.is_multiple_of(self.config.context().context_loop_interval)
        {
            let task_objective = self
                .task_manager
                .get(&self.task_id)
                .map(|t| t.objective.clone())
                .unwrap_or_default();
            let ctx_msg = self.context_loop_state.build_context_loop(
                iteration,
                self.config.loop_cfg().max_iterations,
                &task_objective,
            );
            messages.push(Message::user(ctx_msg));
        }

        // Phase transitions (legacy, preserved for context loop diagnostics).
        self.context_loop_state
            .maybe_transition(iteration, self.config.loop_cfg().max_iterations);

        // Compaction: compress history with semantic progress context.
        let compaction_ctx = crate::compaction::CompactionContext {
            task_objective: messages
                .iter()
                .find(|m| m.role == theo_infra_llm::types::Role::User)
                .and_then(|m| m.content.clone())
                .unwrap_or_default()
                .chars()
                .take(100)
                .collect(),
            current_phase: format!("{:?}", self.run.state),
            target_files: self.context_loop_state.edits_files.clone(),
            recent_errors: self
                .context_loop_state
                .edit_failures
                .iter()
                .rev()
                .take(2)
                .cloned()
                .collect(),
        };

        // Pre-compression memory hook (persistence survives truncation).
        crate::memory_lifecycle::run_engine_hooks::pre_compress_push(
            &self.config,
            messages,
        )
        .await;

        crate::compaction_stages::compact_staged_with_policy(
            messages,
            self.config.context().context_window_tokens,
            Some(&compaction_ctx),
            self.config.context().compaction_policy,
        );

        // Record context size for metrics (estimated tokens ≈ chars/4).
        let estimated_context_tokens: usize = messages
            .iter()
            .filter_map(|m| m.content.as_ref())
            .map(|c| c.len().div_ceil(4))
            .sum();
        self.obs.context_metrics
            .record_context_size(iteration, estimated_context_tokens);
        estimated_context_tokens
    }
}
