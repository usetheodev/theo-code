//! `done` meta-tool handler.
//!
//! Fase 4 (REMEDIATION_PLAN T4.2). Extracted from the
//! `for call in tool_calls` monolith in `run_engine/mod.rs`. The
//! handler gates convergence behind three checks:
//!
//!   Gate 0 — attempts cap: after `MAX_DONE_ATTEMPTS` retries the
//!   gate force-accepts with a warning rather than burning the entire
//!   budget.
//!
//!   Gate 1 — convergence pre-filter: `git diff` must show real
//!   changes and `ConvergenceEvaluator` must agree.
//!
//!   Gate 2 — clean state sensor: `cargo test` (or `cargo check`) on
//!   the affected crate, executed under rlimits (T1.1). On failure,
//!   the gate returns a BLOCKED tool-result and the main loop
//!   replans.
//!
//! Later (T4.4) this file becomes a Chain-of-Responsibility pipeline;
//! for now the three gates are inline.

use theo_domain::agent_run::RunState;
use theo_domain::error_class::ErrorClass;
use theo_domain::task::TaskState;
use theo_infra_llm::types::{Message, ToolCall};

use super::DispatchOutcome;
use crate::agent_loop::AgentResult;
use crate::convergence::{ConvergenceContext, check_git_changes};
use crate::run_engine::AgentRunEngine;
use crate::run_engine_sandbox::spawn_done_gate_cargo;

impl AgentRunEngine {
    /// Process a `done` tool call. Pushes tool-result / user messages
    /// into `messages` when the gate blocks, and returns an outcome the
    /// caller uses to drive the main loop.
    pub(in crate::run_engine) async fn handle_done_call(
        &mut self,
        call: &ToolCall,
        iteration: usize,
        messages: &mut Vec<Message>,
    ) -> DispatchOutcome {
        use crate::constants::MAX_DONE_ATTEMPTS;

        self.transition_run(RunState::Evaluating);
        self.done_attempts += 1;

        let summary = call
            .parse_arguments()
            .ok()
            .and_then(|args| {
                args.get("summary")
                    .and_then(|s| s.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| "Task completed.".to_string());

        // Gate 0: attempts cap — force-accept rather than burn budget.
        if self.done_attempts > MAX_DONE_ATTEMPTS {
            self.transition_run(RunState::Converged);
            let _ = self
                .task_manager
                .transition(&self.task_id, TaskState::Completed);
            self.metrics.record_run_complete(true);
            let summary = format!(
                "{} [accepted after {} done attempts]",
                summary, self.done_attempts
            );
            return DispatchOutcome::Converged(AgentResult::from_engine_state(
                self,
                true,
                summary,
                false,
                ErrorClass::Solved,
            ));
        }

        // Gate 1: convergence pre-filter.
        let has_changes = check_git_changes(&self.project_dir).await;
        let convergence_ctx = ConvergenceContext {
            has_git_changes: has_changes,
            edits_succeeded: self.context_loop_state.edits_files.len(),
            done_requested: true,
            iteration,
            max_iterations: self.config.max_iterations,
        };
        if !self.convergence.evaluate(&convergence_ctx) {
            let pending = self.convergence.pending_criteria(&convergence_ctx);
            messages.push(Message::tool_result(
                &call.id,
                "done",
                format!(
                    "BLOCKED: convergence criteria not met: {}. Make real changes before calling done.",
                    pending.join(", ")
                ),
            ));
            self.transition_run(RunState::Replanning);
            return DispatchOutcome::Continue;
        }

        // Non-blocking review hint for large diffs.
        self.maybe_emit_large_diff_hint(messages).await;

        // Gate 2: clean state sensor.
        if self.project_dir.join("Cargo.toml").exists()
            && let Some(error_preview) = self.run_done_gate_tests().await
        {
            let cmd_str = self.pick_done_gate_test_args().join(" ");
            messages.push(Message::tool_result(
                &call.id,
                "done",
                format!(
                    "BLOCKED: `cargo {}` failed (attempt {}/{}). Fix the errors before calling done.\n\n{}",
                    cmd_str, self.done_attempts, MAX_DONE_ATTEMPTS, error_preview
                ),
            ));
            self.transition_run(RunState::Replanning);
            return DispatchOutcome::Continue;
        }

        // All gates passed — converge.
        self.transition_run(RunState::Converged);
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Completed);
        self.metrics.record_run_complete(true);
        DispatchOutcome::Converged(AgentResult::from_engine_state(
            self,
            true,
            summary,
            false,
            ErrorClass::Solved,
        ))
    }

    /// Emit an advisory user message when the diff touches > 3 files
    /// with > 100 lines changed. Purely informational — never blocks.
    async fn maybe_emit_large_diff_hint(&self, messages: &mut Vec<Message>) {
        if self.context_loop_state.edits_files.len() <= 3 {
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
                self.context_loop_state.edits_files.len(),
                lines_changed
            )));
        }
    }

    /// Decide which `cargo` invocation is appropriate for the done
    /// gate based on the first edited file. Falls back to `cargo check`
    /// when no files were edited.
    fn pick_done_gate_test_args(&self) -> Vec<String> {
        match self.context_loop_state.edits_files.first() {
            Some(first_file) => {
                let crate_name = std::path::Path::new(first_file)
                    .components()
                    .zip(std::path::Path::new(first_file).components().skip(1))
                    .find(|(a, _)| {
                        let s = a.as_os_str().to_string_lossy();
                        s == "crates" || s == "apps"
                    })
                    .map(|(_, b)| b.as_os_str().to_string_lossy().to_string());
                if let Some(name) = crate_name {
                    vec![
                        "test".to_string(),
                        "-p".to_string(),
                        name,
                        "--no-fail-fast".to_string(),
                    ]
                } else {
                    vec!["test".to_string(), "--no-fail-fast".to_string()]
                }
            }
            None => vec!["check".to_string(), "--message-format=short".to_string()],
        }
    }

    /// Run the done-gate Gate 2 command(s) and return `Some(preview)` on
    /// failure, `None` on success/skip.
    async fn run_done_gate_tests(&self) -> Option<String> {
        let test_args = self.pick_done_gate_test_args();

        let test_result = tokio::time::timeout(
            crate::constants::DONE_GATE_TEST_TIMEOUT,
            spawn_done_gate_cargo(&self.project_dir, &test_args),
        )
        .await;

        let raw_errors = match test_result {
            Ok(Ok(output)) if !output.status.success() => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                Some(format!("{}\n{}", stderr, stdout))
            }
            Ok(Ok(_)) => None,  // Tests passed
            Ok(Err(_)) => None, // Command not found — pass through
            Err(_) => {
                // Timeout — fallback to cargo check (also under rlimits).
                let fallback = tokio::time::timeout(
                    crate::constants::DONE_GATE_CHECK_FALLBACK_TIMEOUT,
                    spawn_done_gate_cargo(
                        &self.project_dir,
                        &[
                            "check".to_string(),
                            "--message-format=short".to_string(),
                        ],
                    ),
                )
                .await;
                match fallback {
                    Ok(Ok(output)) if !output.status.success() => {
                        Some(String::from_utf8_lossy(&output.stderr).to_string())
                    }
                    _ => None, // Fallback passed or timed out — accept
                }
            }
        };

        raw_errors.map(|errors| {
            theo_domain::prompt_sanitizer::char_boundary_truncate(
                &errors,
                crate::constants::DONE_GATE_ERROR_PREVIEW_BYTES,
            )
        })
    }
}
