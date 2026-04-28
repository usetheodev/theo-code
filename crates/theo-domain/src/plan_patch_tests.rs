//! Sibling test body of `plan.rs` — split per-feature (T3.4 of code-hygiene-5x5).
//!
//! Test-only file; gates use the inner `cfg(test)` attribute below to
//! classify every line as test code.

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::plan_test_helpers::*;
use crate::plan_patch::{InsertPosition, PatchError, PlanPatch, TaskEdits};

#[test]
fn t61_apply_patch_skip_task_marks_skipped_with_outcome() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    plan.apply_patch(&PlanPatch::SkipTask {
        id: PlanTaskId(1),
        rationale: "Out of scope".into(),
    })
    .unwrap();
    let t = plan.find_task(PlanTaskId(1)).unwrap();
    assert_eq!(t.status, PlanTaskStatus::Skipped);
    assert_eq!(t.outcome.as_deref(), Some("Out of scope"));
}

#[test]
fn t61_apply_patch_skip_unknown_id_returns_not_found() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    let err = plan
        .apply_patch(&PlanPatch::SkipTask {
            id: PlanTaskId(99),
            rationale: "x".into(),
        })
        .unwrap_err();
    assert_eq!(err, PatchError::TaskNotFound(PlanTaskId(99)));
}

#[test]
fn t61_apply_patch_remove_task_with_dependents_rejected() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    let err = plan
        .apply_patch(&PlanPatch::RemoveTask { id: PlanTaskId(1) })
        .unwrap_err();
    assert_eq!(err, PatchError::RemoveWouldOrphan(PlanTaskId(1)));
    // Plan unchanged on error (atomicity).
    assert_eq!(plan.all_tasks().len(), 2);
}

#[test]
fn t61_apply_patch_remove_leaf_task_succeeds() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    plan.apply_patch(&PlanPatch::RemoveTask { id: PlanTaskId(2) })
        .unwrap();
    assert_eq!(plan.all_tasks().len(), 1);
    assert!(plan.find_task(PlanTaskId(2)).is_none());
}

#[test]
fn t61_apply_patch_add_task_at_end_preserves_validity() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    let new_task = task(2, PlanTaskStatus::Pending, vec![1]);
    plan.apply_patch(&PlanPatch::AddTask {
        phase: PhaseId(1),
        task: new_task,
        position: InsertPosition::End,
    })
    .unwrap();
    assert_eq!(plan.phases[0].tasks.len(), 2);
    assert_eq!(plan.phases[0].tasks[1].id, PlanTaskId(2));
}

#[test]
fn t61_apply_patch_add_task_with_invalid_dep_rolls_back() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    let bad_task = task(2, PlanTaskStatus::Pending, vec![99]); // dep doesn't exist
    let err = plan
        .apply_patch(&PlanPatch::AddTask {
            phase: PhaseId(1),
            task: bad_task,
            position: InsertPosition::End,
        })
        .unwrap_err();
    match err {
        PatchError::Validation(PlanValidationError::InvalidDependency { .. }) => {}
        other => panic!("expected InvalidDependency: {other:?}"),
    }
    // Plan unchanged: only original task survives.
    assert_eq!(plan.all_tasks().len(), 1);
}

#[test]
fn t61_apply_patch_add_task_unknown_phase_returns_phase_not_found() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    let err = plan
        .apply_patch(&PlanPatch::AddTask {
            phase: PhaseId(99),
            task: task(2, PlanTaskStatus::Pending, vec![]),
            position: InsertPosition::End,
        })
        .unwrap_err();
    assert_eq!(err, PatchError::PhaseNotFound(PhaseId(99)));
}

#[test]
fn t61_apply_patch_add_task_after_anchor_inserts_correctly() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(3, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    plan.apply_patch(&PlanPatch::AddTask {
        phase: PhaseId(1),
        task: task(2, PlanTaskStatus::Pending, vec![1]),
        position: InsertPosition::AfterTask { id: PlanTaskId(1) },
    })
    .unwrap();
    let ids: Vec<u32> = plan.phases[0].tasks.iter().map(|t| t.id.0).collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn t61_apply_patch_edit_task_changes_only_specified_fields() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    plan.apply_patch(&PlanPatch::EditTask {
        id: PlanTaskId(1),
        edits: TaskEdits {
            title: Some("Renamed".into()),
            status: Some(PlanTaskStatus::Blocked),
            ..Default::default()
        },
    })
    .unwrap();
    let t = plan.find_task(PlanTaskId(1)).unwrap();
    assert_eq!(t.title, "Renamed");
    assert_eq!(t.status, PlanTaskStatus::Blocked);
    // Untouched fields preserved.
    assert!(t.dod.is_empty());
}

#[test]
fn t61_apply_patch_edit_empty_returns_empty_error() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    let err = plan
        .apply_patch(&PlanPatch::EditTask {
            id: PlanTaskId(1),
            edits: TaskEdits::default(),
        })
        .unwrap_err();
    assert_eq!(err, PatchError::Empty);
}

#[test]
fn t61_apply_patch_reorder_deps_introducing_cycle_rolls_back() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    let err = plan
        .apply_patch(&PlanPatch::ReorderDeps {
            id: PlanTaskId(1),
            new_deps: vec![PlanTaskId(2)], // creates cycle 1→2→1
        })
        .unwrap_err();
    assert_eq!(err, PatchError::Validation(PlanValidationError::CycleDetected));
    // Plan unchanged.
    let t1 = plan.find_task(PlanTaskId(1)).unwrap();
    assert!(t1.depends_on.is_empty());
}

// ---------------------------------------------------------------------
// T7.1 — Multi-agent claim/release + version_counter
// ---------------------------------------------------------------------

#[test]
fn t61_apply_patch_atomicity_rejected_patch_leaves_plan_intact() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    let snapshot_before = plan.clone();
    let _ = plan.apply_patch(&PlanPatch::ReorderDeps {
        id: PlanTaskId(1),
        new_deps: vec![PlanTaskId(2)],
    });
    assert_eq!(plan, snapshot_before);
}

// ── T6.1 — failure_count + auto-replan trigger helpers ────────

