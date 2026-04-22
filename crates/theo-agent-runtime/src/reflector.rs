//! Heuristic failure reflector for the Pilot loop.
//!
//! Classifies failure patterns from agent run results and generates
//! targeted corrective guidance. Pure functions — no IO, no async.
//!
//! Phase 1 of the self-improving pilot: heuristic classification.
//! Phase 2 adds learning persistence. Phase 4 adds LLM-based reflection.

// ---------------------------------------------------------------------------
// FailurePattern
// ---------------------------------------------------------------------------

/// Classified failure pattern from a pilot loop iteration.
///
/// Only patterns with concrete observable signals are included (YAGNI).
/// New variants are added when sensors to detect them are implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FailurePattern {
    /// FM-1: Agent ran multiple loops without editing any files.
    NoProgressLoop,
    /// FM-2: Agent keeps hitting the same error repeatedly.
    RepeatedSameError,
    /// FM-3: Agent converged without producing successful edits.
    PrematureTermination,
    /// FM-4: Agent made edits without verifying them (no bash/sensor).
    WeakVerification,
    /// FM-5: Agent stopped referencing initial-context files for 5+ calls.
    TaskDerailment,
    /// FM-6: Agent re-read pre-compaction hot files after ContextOverflowRecovery.
    ConversationHistoryLoss,
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Threshold: how many consecutive occurrences before triggering guidance.
const GUIDANCE_THRESHOLD: usize = 2;

/// Max characters of error preview in guidance text.
const ERROR_PREVIEW_MAX: usize = 200;

/// Classify the failure pattern from observable pilot loop state.
///
/// Pure function — no IO, no side effects.
///
/// Priority: NoProgressLoop > RepeatedSameError (checked first).
/// Returns None for successful runs or when below threshold.
pub fn classify_failure(
    consecutive_no_progress: usize,
    consecutive_same_error: usize,
    last_error: Option<&str>,
    success: bool,
) -> Option<FailurePattern> {
    // Successful runs don't need corrective guidance.
    if success {
        return None;
    }

    // NoProgressLoop takes priority — agent is stuck without making changes.
    if consecutive_no_progress >= GUIDANCE_THRESHOLD {
        return Some(FailurePattern::NoProgressLoop);
    }

    // RepeatedSameError — only when we have the actual error message.
    if consecutive_same_error >= GUIDANCE_THRESHOLD && last_error.is_some() {
        return Some(FailurePattern::RepeatedSameError);
    }

    None
}

/// Generate corrective guidance text for a classified failure pattern.
///
/// The guidance is injected into the pilot loop prompt to steer the agent
/// toward a different approach.
pub fn guidance_for_pattern(
    pattern: FailurePattern,
    consecutive_no_progress: usize,
    consecutive_same_error: usize,
    last_error: Option<&str>,
) -> String {
    match pattern {
        FailurePattern::NoProgressLoop => {
            format!(
                "WARNING: You have not made file changes in {} consecutive loops. \
                 Focus on EDITING code, not just reading. Make concrete changes.",
                consecutive_no_progress
            )
        }
        FailurePattern::RepeatedSameError => {
            let err = last_error.unwrap_or("unknown error");
            let err_preview: String = err.chars().take(ERROR_PREVIEW_MAX).collect();
            format!(
                "WARNING: You keep getting the same error ({} times): {}...\n\
                 Stop retrying the same approach. Try something DIFFERENT.",
                consecutive_same_error, err_preview
            )
        }
        FailurePattern::PrematureTermination => {
            "WARNING: You converged without producing any edits. The task is not complete \
             without concrete file changes. Review what the task is asking and make edits."
                .to_string()
        }
        FailurePattern::WeakVerification => {
            "WARNING: You made edits without running verification (bash/sensor). \
             Always verify edits by running tests or checking types."
                .to_string()
        }
        FailurePattern::TaskDerailment => {
            "WARNING: You have performed multiple tool calls without referencing any \
             file from the initial context. Revisit the task and the files in scope."
                .to_string()
        }
        FailurePattern::ConversationHistoryLoss => {
            "WARNING: Context was compacted and you immediately re-read files that were \
             already hot before compaction. Summarize existing knowledge instead of re-reading."
                .to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// HeuristicReflector (stateless wrapper for PilotLoop integration)
// ---------------------------------------------------------------------------

/// Heuristic reflector that classifies failures and generates guidance.
///
/// Stateless for the legacy `corrective_guidance` API, but holds an internal
/// `LoopDetector` (Phase 4, T4.4) that consumes normalized tool calls and
/// returns corrective guidance when the detector reports a verdict other
/// than `Ok`.
#[derive(Default)]
pub struct HeuristicReflector {
    loop_detector: std::sync::Mutex<crate::observability::LoopDetector>,
}

impl std::fmt::Debug for HeuristicReflector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeuristicReflector").finish()
    }
}

impl HeuristicReflector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Classify failure and generate guidance if applicable.
    ///
    /// Drop-in replacement for PilotLoop::build_corrective_guidance().
    pub fn corrective_guidance(
        &self,
        consecutive_no_progress: usize,
        consecutive_same_error: usize,
        last_error: Option<&str>,
        success: bool,
    ) -> Option<String> {
        let pattern = classify_failure(
            consecutive_no_progress,
            consecutive_same_error,
            last_error,
            success,
        )?;

        Some(guidance_for_pattern(
            pattern,
            consecutive_no_progress,
            consecutive_same_error,
            last_error,
        ))
    }

    /// T4.4: Record a tool invocation through the loop detector and return
    /// guidance if the verdict is `Correct` or `HardStop`.
    pub fn record_tool_call(
        &self,
        tool_name: &str,
        normalized_args: &serde_json::Value,
        normalized_output: &str,
    ) -> Option<String> {
        use crate::observability::LoopVerdict;
        let verdict = self
            .loop_detector
            .lock()
            .expect("loop detector mutex poisoned")
            .record(tool_name, normalized_args, normalized_output);
        match verdict {
            LoopVerdict::Ok | LoopVerdict::Warning => None,
            LoopVerdict::Correct => Some(format!(
                "WARNING: You have called `{}` with the same arguments multiple times \
                 in a row. The output appears to be identical. Try a DIFFERENT approach.",
                tool_name
            )),
            LoopVerdict::HardStop => Some(format!(
                "HARD STOP: You have called `{}` identically 5+ times. Abort this \
                 approach and reconsider the task.",
                tool_name
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- classify_failure tests ---

    #[test]
    fn success_returns_none() {
        assert_eq!(classify_failure(5, 5, Some("err"), true), None);
    }

    #[test]
    fn below_threshold_returns_none() {
        assert_eq!(classify_failure(1, 1, Some("err"), false), None);
        assert_eq!(classify_failure(0, 0, None, false), None);
    }

    #[test]
    fn no_progress_detected() {
        assert_eq!(
            classify_failure(2, 0, None, false),
            Some(FailurePattern::NoProgressLoop)
        );
        assert_eq!(
            classify_failure(5, 0, None, false),
            Some(FailurePattern::NoProgressLoop)
        );
    }

    #[test]
    fn same_error_detected() {
        assert_eq!(
            classify_failure(0, 2, Some("compile error"), false),
            Some(FailurePattern::RepeatedSameError)
        );
    }

    #[test]
    fn same_error_without_message_returns_none() {
        // No error message → can't generate useful guidance.
        assert_eq!(classify_failure(0, 2, None, false), None);
    }

    #[test]
    fn no_progress_has_priority_over_same_error() {
        // Both conditions met → NoProgressLoop wins (checked first).
        assert_eq!(
            classify_failure(2, 2, Some("err"), false),
            Some(FailurePattern::NoProgressLoop)
        );
    }

    // --- guidance_for_pattern tests ---

    #[test]
    fn guidance_no_progress_contains_count() {
        let text = guidance_for_pattern(FailurePattern::NoProgressLoop, 3, 0, None);
        assert!(text.contains("3 consecutive loops"));
        assert!(text.contains("not made file changes"));
    }

    #[test]
    fn guidance_same_error_contains_error_preview() {
        let text = guidance_for_pattern(
            FailurePattern::RepeatedSameError,
            0,
            4,
            Some("compile error: missing semicolon"),
        );
        assert!(text.contains("4 times"));
        assert!(text.contains("compile error"));
        assert!(text.contains("DIFFERENT"));
    }

    #[test]
    fn guidance_same_error_truncates_long_error() {
        let long_error = "x".repeat(500);
        let text = guidance_for_pattern(FailurePattern::RepeatedSameError, 0, 2, Some(&long_error));
        // Should not contain the full 500-char error.
        assert!(text.len() < 500);
    }

    // --- HeuristicReflector integration ---

    #[test]
    fn reflector_backward_compat_no_progress() {
        let r = HeuristicReflector::new();
        let guidance = r.corrective_guidance(2, 0, None, false);
        assert!(guidance.is_some());
        assert!(guidance.unwrap().contains("not made file changes"));
    }

    #[test]
    fn reflector_backward_compat_same_error() {
        let r = HeuristicReflector::new();
        let guidance = r.corrective_guidance(0, 2, Some("compile error"), false);
        assert!(guidance.is_some());
        assert!(guidance.unwrap().contains("same error"));
    }

    #[test]
    fn reflector_returns_none_for_success() {
        let r = HeuristicReflector::new();
        assert!(r.corrective_guidance(5, 5, Some("err"), true).is_none());
    }

    // --- T4.4: LoopDetector integration ---

    #[test]
    fn test_reflector_emits_corrective_guidance_on_loop() {
        let r = HeuristicReflector::new();
        assert!(r.record_tool_call("grep", &serde_json::json!({"q": "x"}), "").is_none());
        assert!(r.record_tool_call("grep", &serde_json::json!({"q": "x"}), "").is_none());
        let out = r.record_tool_call("grep", &serde_json::json!({"q": "x"}), "");
        assert!(out.is_some());
    }

    #[test]
    fn test_reflector_loop_verdict_correct_contains_message() {
        let r = HeuristicReflector::new();
        r.record_tool_call("grep", &serde_json::json!({"q": "x"}), "");
        r.record_tool_call("grep", &serde_json::json!({"q": "x"}), "");
        let out = r.record_tool_call("grep", &serde_json::json!({"q": "x"}), "").unwrap();
        assert!(out.contains("grep"));
    }

    // --- T5.5: exhaustive match over 6 failure variants ---

    #[test]
    fn test_failure_pattern_has_6_variants() {
        fn match_all(p: FailurePattern) -> &'static str {
            match p {
                FailurePattern::NoProgressLoop => "no_progress",
                FailurePattern::RepeatedSameError => "repeated_error",
                FailurePattern::PrematureTermination => "premature",
                FailurePattern::WeakVerification => "weak_verify",
                FailurePattern::TaskDerailment => "derailment",
                FailurePattern::ConversationHistoryLoss => "history_loss",
            }
        }
        assert_eq!(match_all(FailurePattern::NoProgressLoop), "no_progress");
        assert_eq!(match_all(FailurePattern::PrematureTermination), "premature");
    }
}
