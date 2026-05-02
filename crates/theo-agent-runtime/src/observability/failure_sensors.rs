//! Post-run failure sensors — pure predicates over `ProjectedStep`s.
//!
//! Each sensor encodes a specific failure mode from the ADR:
//! - FM-3: PrematureTermination
//! - FM-4: WeakVerification
//! - FM-5: TaskDerailment
//! - FM-6: ConversationHistoryLoss

use std::collections::HashSet;

use crate::observability::projection::{ProjectedStep, StepOutcome};

/// Tool ids that mutate the workspace.
///
/// Bug 2026-04-27 (dogfood): the original list was `edit_file | write_file`,
/// which never matched the production registry (snapshot-pinned by
/// `default_registry_tool_id_snapshot_is_pinned`: `edit`, `write`,
/// `apply_patch`). The mismatch caused `detect_premature_termination` to
/// fire as a false positive on every successful run with ≥ 2 iterations
/// (edits were ignored ⇒ `successful_edits == 0`) and turned
/// `detect_weak_verification` into dead code (the window never opened).
fn is_edit_tool(step: &ProjectedStep) -> bool {
    matches!(
        step.tool_name.as_deref(),
        Some("edit" | "write" | "apply_patch")
    )
}

fn is_verification_tool(step: &ProjectedStep) -> bool {
    matches!(step.tool_name.as_deref(), Some("bash"))
        || step.event_type == "SensorExecuted"
}

/// FM-3: Agent converged with zero successful edits.
pub fn detect_premature_termination(steps: &[ProjectedStep]) -> bool {
    // Count successful edits.
    let successful_edits = steps
        .iter()
        .filter(|s| {
            s.event_type == "ToolCallCompleted"
                && is_edit_tool(s)
                && matches!(s.outcome, Some(StepOutcome::Success))
        })
        .count();

    // Count total iterations (proxy: LlmCallStart events).
    let iterations = steps
        .iter()
        .filter(|s| s.event_type == "LlmCallStart")
        .count();

    // Budget exceeded?
    let budget_exceeded = steps.iter().any(|s| s.event_type == "BudgetExceeded");

    // Convergence?
    let converged = steps.iter().any(|s| {
        s.event_type == "RunStateChanged"
            && s.payload_summary.to_lowercase().contains("converged")
    });

    converged && successful_edits == 0 && iterations >= 2 && !budget_exceeded
}

/// FM-4: An edit was performed without a subsequent verification within 3 steps.
pub fn detect_weak_verification(steps: &[ProjectedStep]) -> bool {
    for (i, s) in steps.iter().enumerate() {
        if s.event_type == "ToolCallCompleted"
            && is_edit_tool(s)
            && matches!(s.outcome, Some(StepOutcome::Success))
        {
            let window_end = (i + 4).min(steps.len());
            let has_verification = steps[i + 1..window_end].iter().any(is_verification_tool);
            if !has_verification {
                return true;
            }
        }
    }
    false
}

/// FM-5: 5 consecutive tool calls that never reference any file from the initial context.
pub fn detect_task_derailment(
    steps: &[ProjectedStep],
    initial_context: &HashSet<String>,
) -> bool {
    let mut consecutive = 0usize;
    let mut had_overflow_recovery = false;

    for s in steps {
        if s.event_type == "ContextOverflowRecovery" {
            had_overflow_recovery = true;
            consecutive = 0;
            continue;
        }
        if s.event_type != "ToolCallCompleted" {
            continue;
        }
        let mentions_initial = initial_context
            .iter()
            .any(|f| s.payload_summary.contains(f));
        if mentions_initial {
            consecutive = 0;
        } else {
            consecutive += 1;
            if consecutive >= 5 && !had_overflow_recovery {
                return true;
            }
        }
    }
    false
}

/// FM-6: After a ContextOverflowRecovery, a pre-compaction hot file is re-read within 3 steps.
pub fn detect_conversation_history_loss(
    steps: &[ProjectedStep],
    pre_compaction_hot_files: &HashSet<String>,
) -> bool {
    let mut overflow_indices: Vec<usize> = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        if s.event_type == "ContextOverflowRecovery" {
            overflow_indices.push(i);
        }
    }
    for start in overflow_indices {
        let end = (start + 4).min(steps.len());
        for s in &steps[start + 1..end] {
            if s.event_type == "ToolCallCompleted"
                // Bug 2026-04-27 (dogfood): was `read_file`, but the
                // production registry exposes the read tool as `read`
                // (pinned by `default_registry_tool_id_snapshot_is_pinned`).
                && matches!(s.tool_name.as_deref(), Some("read"))
                && pre_compaction_hot_files
                    .iter()
                    .any(|f| s.payload_summary.contains(f))
                {
                    return true;
                }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::event::EventKind;

    fn step(
        seq: u64,
        ts: u64,
        et: &str,
        tool: Option<&str>,
        outcome: Option<StepOutcome>,
        summary: &str,
    ) -> ProjectedStep {
        ProjectedStep {
            sequence: seq,
            event_type: et.into(),
            event_kind: Some(EventKind::Tooling),
            timestamp: ts,
            entity_id: format!("e{}", seq),
            payload_summary: summary.into(),
            duration_ms: None,
            tool_name: tool.map(String::from),
            outcome,
        }
    }

    // --- FM-3 ---

    #[test]
    fn test_premature_termination_detected() {
        let s = vec![
            step(0, 0, "RunInitialized", None, None, ""),
            step(1, 1, "LlmCallStart", None, None, ""),
            step(2, 2, "LlmCallStart", None, None, ""),
            step(3, 3, "LlmCallStart", None, None, ""),
            step(4, 4, "RunStateChanged", None, None, "converged"),
        ];
        assert!(detect_premature_termination(&s));
    }

    #[test]
    fn test_not_premature_if_edits_made() {
        let s = vec![
            step(0, 0, "RunInitialized", None, None, ""),
            step(1, 1, "LlmCallStart", None, None, ""),
            step(2, 2, "LlmCallStart", None, None, ""),
            step(3, 3, "ToolCallCompleted", Some("edit"), Some(StepOutcome::Success), ""),
            step(4, 4, "RunStateChanged", None, None, "converged"),
        ];
        assert!(!detect_premature_termination(&s));
    }

    #[test]
    fn test_not_premature_if_budget_exceeded() {
        let s = vec![
            step(0, 0, "RunInitialized", None, None, ""),
            step(1, 1, "LlmCallStart", None, None, ""),
            step(2, 2, "LlmCallStart", None, None, ""),
            step(3, 3, "BudgetExceeded", None, None, ""),
            step(4, 4, "RunStateChanged", None, None, "converged"),
        ];
        assert!(!detect_premature_termination(&s));
    }

    #[test]
    fn test_not_premature_if_single_iteration() {
        let s = vec![
            step(0, 0, "RunInitialized", None, None, ""),
            step(1, 1, "LlmCallStart", None, None, ""),
            step(2, 2, "RunStateChanged", None, None, "converged"),
        ];
        assert!(!detect_premature_termination(&s));
    }

    // --- FM-4 ---

    #[test]
    fn test_weak_verification_detected() {
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("edit"), Some(StepOutcome::Success), ""),
            step(1, 1, "LlmCallStart", None, None, ""),
            step(2, 2, "LlmCallStart", None, None, ""),
            step(3, 3, "LlmCallStart", None, None, ""),
        ];
        assert!(detect_weak_verification(&s));
    }

    #[test]
    fn test_verification_present_clears_flag() {
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("edit"), Some(StepOutcome::Success), ""),
            step(1, 1, "ToolCallCompleted", Some("bash"), Some(StepOutcome::Success), ""),
        ];
        assert!(!detect_weak_verification(&s));
    }

    #[test]
    fn test_sensor_execution_counts_as_verification() {
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("edit"), Some(StepOutcome::Success), ""),
            step(1, 1, "SensorExecuted", None, None, ""),
        ];
        assert!(!detect_weak_verification(&s));
    }

    #[test]
    fn test_multiple_edits_each_checked() {
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("edit"), Some(StepOutcome::Success), ""),
            step(1, 1, "ToolCallCompleted", Some("bash"), Some(StepOutcome::Success), ""),
            step(2, 2, "ToolCallCompleted", Some("edit"), Some(StepOutcome::Success), ""),
            step(3, 3, "LlmCallStart", None, None, ""),
            step(4, 4, "LlmCallStart", None, None, ""),
            step(5, 5, "LlmCallStart", None, None, ""),
        ];
        assert!(detect_weak_verification(&s));
    }

    // --- FM-5 ---

    #[test]
    fn test_derailment_detected() {
        let mut ctx = HashSet::new();
        ctx.insert("file_a".to_string());
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("bash"), None, "unrelated 1"),
            step(1, 1, "ToolCallCompleted", Some("bash"), None, "unrelated 2"),
            step(2, 2, "ToolCallCompleted", Some("bash"), None, "unrelated 3"),
            step(3, 3, "ToolCallCompleted", Some("bash"), None, "unrelated 4"),
            step(4, 4, "ToolCallCompleted", Some("bash"), None, "unrelated 5"),
        ];
        assert!(detect_task_derailment(&s, &ctx));
    }

    #[test]
    fn test_no_derailment_when_context_files_used() {
        let mut ctx = HashSet::new();
        ctx.insert("file_a".to_string());
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("bash"), None, "file_a used"),
            step(1, 1, "ToolCallCompleted", Some("bash"), None, "file_a used"),
            step(2, 2, "ToolCallCompleted", Some("bash"), None, "file_a used"),
            step(3, 3, "ToolCallCompleted", Some("bash"), None, "file_a used"),
            step(4, 4, "ToolCallCompleted", Some("bash"), None, "file_a used"),
        ];
        assert!(!detect_task_derailment(&s, &ctx));
    }

    #[test]
    fn test_no_derailment_if_preceded_by_overflow_recovery() {
        let mut ctx = HashSet::new();
        ctx.insert("file_a".to_string());
        let s = vec![
            step(0, 0, "ContextOverflowRecovery", None, None, ""),
            step(1, 1, "ToolCallCompleted", Some("bash"), None, "x"),
            step(2, 2, "ToolCallCompleted", Some("bash"), None, "x"),
            step(3, 3, "ToolCallCompleted", Some("bash"), None, "x"),
            step(4, 4, "ToolCallCompleted", Some("bash"), None, "x"),
            step(5, 5, "ToolCallCompleted", Some("bash"), None, "x"),
        ];
        assert!(!detect_task_derailment(&s, &ctx));
    }

    #[test]
    fn test_initial_context_from_first_retrieval() {
        // Caller extracts files — sensor just checks against provided set.
        let mut ctx = HashSet::new();
        ctx.insert("src/a.rs".to_string());
        let s = vec![step(0, 0, "ToolCallCompleted", Some("read"), None, "src/a.rs")];
        assert!(!detect_task_derailment(&s, &ctx));
    }

    // --- FM-6 ---

    #[test]
    fn test_history_loss_detected() {
        let mut hot = HashSet::new();
        hot.insert("hot.rs".to_string());
        let s = vec![
            step(0, 0, "ContextOverflowRecovery", None, None, ""),
            step(1, 1, "ToolCallCompleted", Some("read"), None, "hot.rs"),
        ];
        assert!(detect_conversation_history_loss(&s, &hot));
    }

    #[test]
    fn test_no_loss_without_overflow() {
        let mut hot = HashSet::new();
        hot.insert("hot.rs".to_string());
        let s = vec![step(0, 0, "ToolCallCompleted", Some("read"), None, "hot.rs")];
        assert!(!detect_conversation_history_loss(&s, &hot));
    }

    #[test]
    fn test_no_loss_when_new_files_read() {
        let mut hot = HashSet::new();
        hot.insert("hot.rs".to_string());
        let s = vec![
            step(0, 0, "ContextOverflowRecovery", None, None, ""),
            step(1, 1, "ToolCallCompleted", Some("read"), None, "new.rs"),
        ];
        assert!(!detect_conversation_history_loss(&s, &hot));
    }

    // --- Regression — dogfood 2026-04-27 ---
    //
    // Before the fix, `is_edit_tool` matched only `edit_file | write_file`
    // and `detect_conversation_history_loss` matched only `read_file` —
    // names that haven't existed in the production registry since at least
    // the snapshot pin (`default_registry_tool_id_snapshot_is_pinned`).
    // The test below assumes the production names (`edit`, `write`, `read`)
    // and pins each sensor's contract against them so a future rename
    // can't silently turn `PrematureTermination` back into a false positive
    // or `WeakVerification` / `ConversationHistoryLoss` back into dead code.

    #[test]
    fn dogfood_premature_termination_recognises_production_edit_tool_id() {
        // Two LLM iterations + ONE successful `edit` (not `edit_file`)
        // + Converged → must NOT fire (edit is real progress).
        let s = vec![
            step(0, 0, "RunInitialized", None, None, ""),
            step(1, 1, "LlmCallStart", None, None, ""),
            step(2, 2, "LlmCallStart", None, None, ""),
            step(3, 3, "ToolCallCompleted", Some("edit"), Some(StepOutcome::Success), ""),
            step(4, 4, "RunStateChanged", None, None, "converged"),
        ];
        assert!(
            !detect_premature_termination(&s),
            "PrematureTermination must accept `edit` as a real edit (was: only `edit_file`)"
        );
    }

    #[test]
    fn dogfood_premature_termination_recognises_production_write_tool_id() {
        let s = vec![
            step(0, 0, "RunInitialized", None, None, ""),
            step(1, 1, "LlmCallStart", None, None, ""),
            step(2, 2, "LlmCallStart", None, None, ""),
            step(3, 3, "ToolCallCompleted", Some("write"), Some(StepOutcome::Success), ""),
            step(4, 4, "RunStateChanged", None, None, "converged"),
        ];
        assert!(!detect_premature_termination(&s));
    }

    #[test]
    fn dogfood_premature_termination_recognises_apply_patch_tool_id() {
        // `apply_patch` is also a production write-side tool — pin it too.
        let s = vec![
            step(0, 0, "RunInitialized", None, None, ""),
            step(1, 1, "LlmCallStart", None, None, ""),
            step(2, 2, "LlmCallStart", None, None, ""),
            step(3, 3, "ToolCallCompleted", Some("apply_patch"), Some(StepOutcome::Success), ""),
            step(4, 4, "RunStateChanged", None, None, "converged"),
        ];
        assert!(!detect_premature_termination(&s));
    }

    #[test]
    fn dogfood_weak_verification_window_opens_for_production_edit_tool_id() {
        // Successful `edit` followed by NOTHING → window opens, no
        // verification → fires.
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("edit"), Some(StepOutcome::Success), ""),
        ];
        assert!(
            detect_weak_verification(&s),
            "WeakVerification must open its window on `edit` (was: only `edit_file`)"
        );
    }

    #[test]
    fn dogfood_conversation_history_loss_matches_production_read_tool_id() {
        // ContextOverflowRecovery followed by `read` of a hot file → fires.
        let mut hot = HashSet::new();
        hot.insert("hot.rs".into());
        let s = vec![
            step(0, 0, "ContextOverflowRecovery", None, None, ""),
            step(1, 1, "ToolCallCompleted", Some("read"), None, "hot.rs"),
        ];
        assert!(
            detect_conversation_history_loss(&s, &hot),
            "ConversationHistoryLoss must match `read` (was: only `read_file`)"
        );
    }
}
