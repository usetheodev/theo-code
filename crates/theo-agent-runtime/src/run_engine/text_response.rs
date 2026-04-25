//! Text-only response handler — the "no tool calls" branch of the
//! main loop.
//!
//! Split out of `run_engine/main_loop.rs` (REMEDIATION_PLAN T4.* —
//! production-LOC trim toward the per-file 500-line target). The
//! method stays `pub(super)` on `impl AgentRunEngine` so the caller in
//! `main_loop.rs` is unchanged.

use theo_domain::agent_run::RunState;
use theo_domain::error_class::ErrorClass;
use theo_domain::task::TaskState;
use theo_infra_llm::types::Message;

use super::AgentRunEngine;
use super::dispatch::DispatchOutcome;
use crate::agent_loop::AgentResult;

impl AgentRunEngine {
    /// Handle the "no tool calls" branch of the main loop: the LLM
    /// returned text only and wants to converge. Three sub-flows:
    ///
    ///   1. Follow-up queue drain — if the user queued a message mid-run,
    ///      inject it and continue.
    ///   2. Plan-mode nudge — in Plan mode the agent must end with tool
    ///      calls. If it emitted text without persisting a plan file,
    ///      inject a one-shot corrective reminder and continue.
    ///   3. Converge — sync final turn to memory, fire reviewers nudge,
    ///      transition task to Completed, and return the result.
    ///
    /// Returns:
    ///   - `DispatchOutcome::Continue` for (1) / (2) — caller continues
    ///     the main loop.
    ///   - `DispatchOutcome::Converged(result)` for (3) — caller breaks
    ///     with this result.
    pub(super) async fn handle_text_only_response(
        &mut self,
        content: String,
        messages: &mut Vec<Message>,
    ) -> DispatchOutcome {
        // 1. Follow-up queue drain.
        if let Some(ref follow_up_fn) = self.message_queues.follow_up {
            let follow_ups = follow_up_fn().await;
            if !follow_ups.is_empty() {
                messages.push(Message::assistant(&content));
                for fu_msg in follow_ups {
                    messages.push(fu_msg);
                }
                return DispatchOutcome::Continue;
            }
        }

        // 2. Plan-mode nudge (one-shot).
        if self.config.loop_cfg().mode == crate::config::AgentMode::Plan
            && !self.plan_mode_nudged
            && !content.is_empty()
        {
            let plans_dir = self.project_dir.join(".theo/plans");
            let plan_written = plans_dir
                .read_dir()
                .ok()
                .map(|mut it| it.next().is_some())
                .unwrap_or(false);
            if !plan_written {
                self.plan_mode_nudged = true;
                messages.push(Message::assistant(&content));
                messages.push(Message::user(
                    "REMINDER: You wrote a plan as text but did not persist it. \
                     You MUST now call the `write` tool to save the plan to \
                     `.theo/plans/01-<slug>.md` (use a kebab-case slug derived from \
                     the task), then call `done` with a one-line summary. Do this in \
                     your next response. Do not write more prose — just call the tools.",
                ));
                return DispatchOutcome::Continue;
            }
        }

        // 3. Converge — persist, nudge reviewers, transition, return.
        // Persist the user→assistant exchange INLINE (not fire-and-forget) —
        // durability > latency.
        crate::memory_lifecycle::run_engine_hooks::sync_final_turn(
            &self.config,
            messages,
            &content,
        )
        .await;

        // Reviewers nudge. Relaxed is sufficient: the flag is a pure
        // per-task counter with no happens-before dependency on any
        // other load; written and read on the same task inside the
        // serial main loop (T5.4).
        let tool_calls_this_task = self.metrics.snapshot().total_tool_calls as usize;
        let skill_created = self
            .skill_created_this_task
            .load(std::sync::atomic::Ordering::Relaxed);
        crate::memory_lifecycle::maybe_spawn_reviewers(
            &self.config,
            &self.memory_nudge_counter,
            &self.skill_nudge_counter,
            messages,
            tool_calls_this_task,
            skill_created,
        );
        self.skill_created_this_task
            .store(false, std::sync::atomic::Ordering::Relaxed);

        self.transition_run(RunState::Converged);
        self.try_task_transition(TaskState::Completed);
        self.metrics.record_run_complete(true);
        DispatchOutcome::Converged(AgentResult::from_engine_state(
            self,
            true,
            content,
            true,
            ErrorClass::Solved,
        ))
    }
}
