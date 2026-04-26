//! `done` meta-tool handler.
//!
//! The handler is a Chain of Responsibility driver (T4.4): each gate
//! lives in `done_gates.rs` as an independent `impl AgentRunEngine`
//! method returning [`GateOutcome`]. `handle_done_call` invokes the
//! gates in order and short-circuits on the first `Return(outcome)`.
//!
//! Current gates:
//!   0. `check_attempt_limit_gate` — force-accept after
//!      `MAX_DONE_ATTEMPTS` retries so the run never burns the entire
//!      budget on repeat `done` calls.
//!   1. `check_convergence_gate` — `git diff` must show real changes
//!      AND `ConvergenceEvaluator` must agree the task is done.
//!   2. `check_test_gate` — `cargo test` (or `cargo check`) on the
//!      affected crate, executed under rlimits (T1.1). Failure returns
//!      a BLOCKED tool-result and the main loop replans.
//!
//! Adding a new gate requires:
//!   1. a new `impl AgentRunEngine` method returning `GateOutcome`
//!   2. a new line in the chain below
//!
//! The per-gate logic (Ok/Block responses, task transitions, metrics,
//! sandbox spawn) stays local to `done_gates.rs`.

use theo_domain::agent_run::RunState;
use theo_infra_llm::types::{Message, ToolCall};

use super::done_gates::GateOutcome;
use super::DispatchOutcome;
use crate::run_engine::AgentRunEngine;

impl AgentRunEngine {
    /// Process a `done` tool call. Pushes tool-result / user messages
    /// into `messages` when a gate blocks, and returns an outcome the
    /// caller uses to drive the main loop.
    pub(in crate::run_engine) async fn handle_done_call(
        &mut self,
        call: &ToolCall,
        iteration: usize,
        messages: &mut Vec<Message>,
    ) -> DispatchOutcome {
        self.transition_run(RunState::Evaluating);
        self.tracking.done_attempts += 1;

        let summary = call
            .parse_arguments()
            .ok()
            .and_then(|args| args.get("summary").and_then(|s| s.as_str()).map(String::from))
            .unwrap_or_else(|| "Task completed.".to_string());

        // ── Gate chain ────────────────────────────────────────────────
        if let GateOutcome::Return(oc) = self.check_attempt_limit_gate(&summary) {
            return oc;
        }
        if let GateOutcome::Return(oc) =
            self.check_convergence_gate(call, iteration, messages).await
        {
            return oc;
        }
        // Non-blocking review hint for large diffs — runs between gates,
        // not a gate itself (never short-circuits).
        self.maybe_emit_large_diff_hint(messages).await;
        if let GateOutcome::Return(oc) = self.check_test_gate(call, messages).await {
            return oc;
        }

        // All gates passed — converge.
        self.accept_done(summary)
    }

    /// Emit an advisory user message when the diff touches > 3 files
    /// with > 100 lines changed. Purely informational — never blocks.
    async fn maybe_emit_large_diff_hint(&self, messages: &mut Vec<Message>) {
        if self.rt.context_loop_state.edits_files.len() <= 3 {
            return;
        }
        let Ok(output) = tokio::process::Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&self.project_dir)
            .output()
            .await
        else {
            return;
        };
        let stat = String::from_utf8_lossy(&output.stdout);
        let lines_changed: usize = stat
            .lines()
            .filter_map(|l| {
                if l.contains("insertion") || l.contains("deletion") {
                    l.split_whitespace()
                        .filter_map(|w| w.parse::<usize>().ok())
                        .sum::<usize>()
                        .into()
                } else {
                    None
                }
            })
            .sum();
        if lines_changed > 100 {
            messages.push(Message::user(format!(
                "Note: This change touches {} files with ~{} lines changed. \
                 Consider reviewing the diff carefully before finalizing.",
                self.rt.context_loop_state.edits_files.len(),
                lines_changed
            )));
        }
    }
}
