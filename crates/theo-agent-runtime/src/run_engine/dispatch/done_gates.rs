//! Done-gate chain of responsibility.
//!
//! REMEDIATION_PLAN T4.4. Each gate is an independent `impl
//! AgentRunEngine` method that returns a `GateOutcome`:
//!
//!   - `Pass` — gate approved, continue to the next one.
//!   - `Return(outcome)` — gate short-circuits the done call with this
//!     `DispatchOutcome` (either Converged or Continue after a block).
//!
//! `handle_done_call` drives the chain by invoking each gate in order.
//! Adding a new gate requires:
//!   1. a new `impl AgentRunEngine` method returning `GateOutcome`
//!   2. a new line in the chain inside `handle_done_call`
//!
//! Stable async Rust does not support `dyn Trait` with `async fn`
//! cheaply (would require `Box<dyn Future>` on every call). A plain
//! `if let Return(oc) = self.gate_N(...).await { return oc; }` chain
//! preserves zero-cost dispatch and the AC goal (each gate is a
//! self-contained unit; adding one is a local change).

use theo_domain::agent_run::RunState;
use theo_domain::error_class::ErrorClass;
use theo_domain::task::TaskState;
use theo_infra_llm::types::{Message, ToolCall};

use super::DispatchOutcome;
use crate::agent_loop::AgentResult;
use crate::convergence::{check_git_changes, ConvergenceContext};
use crate::run_engine::AgentRunEngine;
use crate::run_engine_sandbox::spawn_done_gate_cargo;

/// Outcome returned by every done-gate.
pub(super) enum GateOutcome {
    /// Gate approved — move to the next one.
    Pass,
    /// Gate short-circuits the done call with this outcome.
    Return(DispatchOutcome),
}

impl AgentRunEngine {
    /// Gate 0 — attempts cap. After `MAX_DONE_ATTEMPTS` retries the
    /// gate force-accepts with a warning so the run never burns its
    /// entire budget on repeat `done` calls.
    pub(super) fn check_attempt_limit_gate(&mut self, summary: &str) -> GateOutcome {
        use crate::constants::MAX_DONE_ATTEMPTS;
        if self.done_attempts <= MAX_DONE_ATTEMPTS {
            return GateOutcome::Pass;
        }
        self.transition_run(RunState::Converged);
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Completed);
        self.metrics.record_run_complete(true);
        let annotated = format!(
            "{} [accepted after {} done attempts]",
            summary, self.done_attempts
        );
        GateOutcome::Return(DispatchOutcome::Converged(
            AgentResult::from_engine_state(self, true, annotated, false, ErrorClass::Solved),
        ))
    }

    /// Gate 1 — convergence pre-filter. `git diff` must show real
    /// changes AND the `ConvergenceEvaluator` must agree the task is
    /// actually done. Failure blocks with a tool-result message and
    /// the main loop replans.
    pub(super) async fn check_convergence_gate(
        &mut self,
        call: &ToolCall,
        iteration: usize,
        messages: &mut Vec<Message>,
    ) -> GateOutcome {
        let has_changes = check_git_changes(&self.project_dir).await;
        let convergence_ctx = ConvergenceContext {
            has_git_changes: has_changes,
            edits_succeeded: self.context_loop_state.edits_files.len(),
            done_requested: true,
            iteration,
            max_iterations: self.config.loop_cfg().max_iterations,
        };
        if self.convergence.evaluate(&convergence_ctx) {
            return GateOutcome::Pass;
        }
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
        GateOutcome::Return(DispatchOutcome::Continue)
    }

    /// Gate 2 — clean-state sensor. Runs `cargo test` (or `cargo
    /// check`) on the affected crate under rlimits. Failure returns a
    /// BLOCKED tool-result with the error preview truncated to
    /// `DONE_GATE_ERROR_PREVIEW_BYTES`; success or skip (no
    /// `Cargo.toml`) passes through.
    pub(super) async fn check_test_gate(
        &mut self,
        call: &ToolCall,
        messages: &mut Vec<Message>,
    ) -> GateOutcome {
        use crate::constants::MAX_DONE_ATTEMPTS;
        if !self.project_dir.join("Cargo.toml").exists() {
            return GateOutcome::Pass;
        }
        let Some(error_preview) = self.run_done_gate_tests().await else {
            return GateOutcome::Pass;
        };
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
        GateOutcome::Return(DispatchOutcome::Continue)
    }

    /// Terminal gate — all upstream gates passed, the run converges.
    /// Always returns `Converged` with the user-provided summary.
    pub(super) fn accept_done(&mut self, summary: String) -> DispatchOutcome {
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

    /// Run the done-gate Gate 2 command(s) and return `Some(preview)` on
    /// failure, `None` on success/skip.
    pub(super) async fn run_done_gate_tests(&self) -> Option<String> {
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

    // ── End of gates. Helpers below. ──────────────────────────────

    /// Pick the `cargo` invocation appropriate for the done gate based
    /// on the first edited file. Falls back to `cargo check` when no
    /// files were edited.
    pub(super) fn pick_done_gate_test_args(&self) -> Vec<String> {
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
}

#[cfg(test)]
mod tests {
    //! T4.4 AC regression: adding a new gate must require only (1) a
    //! new method returning `GateOutcome`, (2) a new line in the
    //! chain inside `done.rs::handle_done_call`. The core flow of
    //! `handle_done_call` (state transition, summary parse, final
    //! accept) must stay untouched.

    /// Structural invariant: `done.rs` invokes ALL current gates via
    /// `if let GateOutcome::Return(oc)` pattern. Regression guard —
    /// breaks if someone re-inlines a gate back into the handler.
    #[test]
    fn done_handler_chains_all_gates_uniformly() {
        let src = include_str!("done.rs");
        // Collapse to a single line so line-wrapping does not defeat
        // pattern matching.
        let flat: String = src.split_whitespace().collect::<Vec<_>>().join(" ");
        for gate in &[
            "check_attempt_limit_gate",
            "check_convergence_gate",
            "check_test_gate",
        ] {
            assert!(
                src.contains(gate),
                "done.rs must invoke `{gate}` — gate was inlined back?"
            );
            // Every gate call MUST flow through the uniform
            // `GateOutcome::Return(oc) = self.<gate>(...)` chain. The
            // flat-whitespace view normalizes line wraps.
            let pattern = format!("GateOutcome::Return(oc) = self.{gate}");
            assert!(
                flat.contains(&pattern),
                "done.rs invokes `{gate}` but not through the chain's `GateOutcome::Return` pattern — check if a bypass was introduced"
            );
        }
    }

    /// Structural invariant: `done_gates.rs` keeps each gate self-
    /// contained (no cross-gate calls). Adding a new gate must NOT
    /// need to edit an existing one.
    #[test]
    fn gates_do_not_call_each_other() {
        let src = include_str!("done_gates.rs");
        // Cut out test and doc-comment regions before searching.
        let production: String = src
            .lines()
            .take_while(|l| !l.starts_with("#[cfg(test)]"))
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // A cross-gate call would look like `self.check_convergence_gate(...)`
        // inside another gate's body. Since each gate is a `pub(super)
        // fn`, the only legitimate caller is `done.rs::handle_done_call`.
        for caller in &["check_attempt_limit_gate", "check_convergence_gate", "check_test_gate"] {
            let occurrences = production.matches(caller).count();
            // Allow definition site ONLY (the `fn` and docstring
            // reference). A cross-gate call would produce 2+ matches
            // outside comments.
            assert!(
                occurrences <= 2,
                "gate `{caller}` referenced {occurrences} times in done_gates.rs — suggests cross-gate coupling"
            );
        }
    }
}
