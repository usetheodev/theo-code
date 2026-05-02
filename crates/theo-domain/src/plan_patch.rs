//! `PlanPatch` — typed mutations to a `Plan`. T6.1 (Adaptive replanning).
//!
//! When a task fails repeatedly (`replan_threshold` reached in pilot loop),
//! the `replan_advisor` use case asks the LLM to propose a `PlanPatch` that
//! mutates the plan to unstick progress. Patches are typed (not free-form
//! JSON merge) so:
//!
//! - The wire format is schema-validated on the way in.
//! - Each variant has a clear domain meaning the LLM can reason about.
//! - `Plan::apply_patch` re-validates the plan after the mutation, so a
//!   bad patch never produces a corrupt plan.
//!
//! Philosophy: the patch is the *minimal* change. The LLM is encouraged to
//! produce a `SkipTask { rationale }` when it cannot fix the failure rather
//! than rewriting the whole plan.
//!
//! See `docs/plans/sota-tier1-tier2-plan.md` §T6.1 and ADR D4.

use serde::{Deserialize, Serialize};

use crate::identifiers::{PhaseId, PlanTaskId};
use crate::plan::{PlanTask, PlanTaskStatus, PlanValidationError};

/// One mutation to a `Plan`. Applied by `Plan::apply_patch`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum PlanPatch {
    /// Insert a new task into a phase.
    AddTask {
        phase: PhaseId,
        task: PlanTask,
        #[serde(default)]
        position: InsertPosition,
    },
    /// Remove a task. Rejected when other tasks depend on it (orphan check).
    RemoveTask { id: PlanTaskId },
    /// Edit selected fields of a task. Only the supplied fields are touched.
    EditTask { id: PlanTaskId, edits: TaskEdits },
    /// Replace the dependency list of a task.
    ReorderDeps {
        id: PlanTaskId,
        new_deps: Vec<PlanTaskId>,
    },
    /// Mark a task as `Skipped` with a rationale recorded in `outcome`.
    /// Equivalent to `EditTask { status: Skipped, outcome: Some(rationale) }`
    /// but signals intent more clearly in the patch log.
    SkipTask { id: PlanTaskId, rationale: String },
}

/// Where to insert when applying `AddTask`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InsertPosition {
    /// Append to the end of the phase's task list.
    #[default]
    End,
    /// Insert at the beginning of the phase's task list.
    Begin,
    /// Insert immediately after the task with this id (must be in the same phase).
    AfterTask { id: PlanTaskId },
}

/// Sparse update set for `EditTask`. Each `Option<T>` is `Some` only when
/// the corresponding field should change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskEdits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<PlanTaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dod: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Option<String>>, // double-Option: Some(None) clears
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

impl TaskEdits {
    /// Returns true if at least one field will change when applied.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.status.is_none()
            && self.description.is_none()
            && self.dod.is_none()
            && self.rationale.is_none()
            && self.outcome.is_none()
            && self.files.is_none()
    }
}

/// Errors that can arise while applying a `PlanPatch`.
///
/// These wrap `PlanValidationError` for invariant violations and add
/// patch-specific cases (target not found, invalid insert position).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PatchError {
    #[error("task {0} not found in plan")]
    TaskNotFound(PlanTaskId),
    #[error("phase {0} not found in plan")]
    PhaseNotFound(PhaseId),
    #[error("anchor task {anchor} for AfterTask is not in phase {phase}")]
    AnchorNotInPhase {
        anchor: PlanTaskId,
        phase: PhaseId,
    },
    #[error(
        "cannot remove task {0}: other tasks depend on it (orphan dependency would result)"
    )]
    RemoveWouldOrphan(PlanTaskId),
    #[error("plan validation failed after patch: {0}")]
    Validation(#[from] PlanValidationError),
    #[error("patch is a no-op")]
    Empty,
}

/// T6.1 — Trait the agent runtime uses to ask an external advisor
/// for a recovery `PlanPatch` when a task is stuck.
///
/// `theo-application::use_cases::replan_advisor` provides the
/// LLM-driven implementation; tests / offline mode supply mocks
/// that return canned patches. Pure-domain trait so
/// `theo-agent-runtime` can hold a `Box<dyn ReplanAdvisor>` without
/// taking on a dependency on `theo-application` (per ADR-016).
///
/// `propose` returns `None` when the advisor decided not to issue
/// a patch (LLM unavailable, no useful action). Callers fall back
/// to logging the threshold breach without crashing the pilot.
#[async_trait::async_trait]
pub trait ReplanAdvisor: Send + Sync {
    /// Ask the advisor to propose ONE patch that would unstick the
    /// given task. `failure_summary` is the agent's last `result.summary`
    /// for that task — typically the error message or the
    /// final-iteration outcome description.
    async fn propose(
        &self,
        plan: &crate::plan::Plan,
        failed_task_id: PlanTaskId,
        failure_summary: &str,
    ) -> Option<PlanPatch>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_edits_is_empty_when_all_none() {
        assert!(TaskEdits::default().is_empty());
    }

    #[test]
    fn task_edits_not_empty_when_any_field_set() {
        let e = TaskEdits {
            title: Some("x".into()),
            ..Default::default()
        };
        assert!(!e.is_empty());
    }

    #[test]
    fn task_edits_outcome_clearing_marked_non_empty() {
        // Some(None) means "clear the outcome"
        let e = TaskEdits {
            outcome: Some(None),
            ..Default::default()
        };
        assert!(!e.is_empty());
    }

    #[test]
    fn insert_position_default_is_end() {
        assert_eq!(InsertPosition::default(), InsertPosition::End);
    }

    #[test]
    fn plan_patch_serde_roundtrip_skip_task() {
        let p = PlanPatch::SkipTask {
            id: PlanTaskId(3),
            rationale: "Legacy code path".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"kind\":\"skip_task\""));
        let back: PlanPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn plan_patch_serde_roundtrip_add_task() {
        let p = PlanPatch::AddTask {
            phase: PhaseId(1),
            task: PlanTask {
                id: PlanTaskId(99),
                title: "New".into(),
                status: PlanTaskStatus::Pending,
                files: vec![],
                description: String::new(),
                dod: String::new(),
                depends_on: vec![],
                rationale: String::new(),
                outcome: None,
                assignee: None,
                failure_count: 0,
            },
            position: InsertPosition::Begin,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"kind\":\"add_task\""));
        let back: PlanPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn plan_patch_serde_roundtrip_edit_task() {
        let p = PlanPatch::EditTask {
            id: PlanTaskId(1),
            edits: TaskEdits {
                title: Some("Renamed".into()),
                status: Some(PlanTaskStatus::Blocked),
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: PlanPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn plan_patch_serde_roundtrip_reorder_deps() {
        let p = PlanPatch::ReorderDeps {
            id: PlanTaskId(2),
            new_deps: vec![PlanTaskId(1), PlanTaskId(3)],
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: PlanPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn plan_patch_serde_roundtrip_remove_task() {
        let p = PlanPatch::RemoveTask { id: PlanTaskId(5) };
        let json = serde_json::to_string(&p).unwrap();
        let back: PlanPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
