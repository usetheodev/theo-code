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
fn test_plan_task_id_serde_roundtrip() {
    let id = PlanTaskId(42);
    let json = serde_json::to_string(&id).unwrap();
    let back: PlanTaskId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

// ----- RED 2 -----

#[test]
fn test_plan_serde_roundtrip() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Completed, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    let json = serde_json::to_string_pretty(&plan).unwrap();
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(plan, back);
}

// ----- RED 3 -----

#[test]
fn test_plan_validate_rejects_duplicate_task_ids() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(1, PlanTaskStatus::Pending, vec![]),
        ],
    )]);
    let err = plan.validate().unwrap_err();
    assert_eq!(err, PlanValidationError::DuplicateTaskId(PlanTaskId(1)));
}

// ----- RED 4 -----

#[test]
fn test_plan_validate_rejects_orphan_dependency() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![99])],
    )]);
    let err = plan.validate().unwrap_err();
    assert_eq!(
        err,
        PlanValidationError::InvalidDependency {
            task_id: PlanTaskId(1),
            missing_dep: PlanTaskId(99),
        }
    );
}

// ----- RED 5 -----

#[test]
fn test_plan_validate_rejects_cycle() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![3]),
            task(2, PlanTaskStatus::Pending, vec![1]),
            task(3, PlanTaskStatus::Pending, vec![2]),
        ],
    )]);
    let err = plan.validate().unwrap_err();
    assert_eq!(err, PlanValidationError::CycleDetected);
}

// ----- RED 6 -----

#[test]
fn test_plan_topological_order_respects_deps() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![]),
            task(3, PlanTaskStatus::Pending, vec![1, 2]),
        ],
    )]);
    let order = plan.topological_order().unwrap();
    let pos = |id: u32| order.iter().position(|t| *t == PlanTaskId(id)).unwrap();
    assert!(pos(1) < pos(3));
    assert!(pos(2) < pos(3));
}

// ----- RED 7 -----

#[test]
fn test_plan_next_actionable_task_with_deps() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    let next = plan.next_actionable_task().unwrap();
    assert_eq!(next.id, PlanTaskId(1));

    let plan2 = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Completed, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    let next2 = plan2.next_actionable_task().unwrap();
    assert_eq!(next2.id, PlanTaskId(2));
}

// ----- RED 8 -----

#[test]
fn test_plan_next_actionable_task_all_done() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::Completed,
        vec![
            task(1, PlanTaskStatus::Completed, vec![]),
            task(2, PlanTaskStatus::Completed, vec![1]),
        ],
    )]);
    assert!(plan.next_actionable_task().is_none());
}

// ----- RED 9 -----

#[test]
fn test_plan_to_markdown_renders_phases_and_tasks() {
    let plan = make_plan(vec![
        phase(
            1,
            PhaseStatus::Completed,
            vec![task(1, PlanTaskStatus::Completed, vec![])],
        ),
        phase(
            2,
            PhaseStatus::InProgress,
            vec![
                task(2, PlanTaskStatus::InProgress, vec![1]),
                task(3, PlanTaskStatus::Pending, vec![2]),
            ],
        ),
    ]);
    let md = plan.to_markdown();
    assert!(md.contains("# Sample Plan"));
    assert!(md.contains("Phase 1"));
    assert!(md.contains("Phase 2"));
    assert!(md.contains("[x]"));
    assert!(md.contains("[>]"));
    assert!(md.contains("[ ]"));
    assert!(md.contains("T1"));
    assert!(md.contains("T2"));
    assert!(md.contains("T3"));
}

// ----- RED 10 -----

#[test]
fn test_plan_schema_evolution_missing_optional_field() {
    // JSON without `outcome`, `decisions`, `description` etc.
    let json = r#"{
            "version": 1,
            "title": "Compat",
            "goal": "test",
            "current_phase": 1,
            "phases": [{
                "id": 1,
                "title": "Phase 1",
                "status": "in_progress",
                "tasks": [{
                    "id": 1,
                    "title": "Task 1",
                    "status": "pending"
                }]
            }],
            "created_at": 0,
            "updated_at": 0
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let task = &plan.phases[0].tasks[0];
    assert!(task.outcome.is_none());
    assert!(task.depends_on.is_empty());
    assert!(task.files.is_empty());
    assert!(plan.decisions.is_empty());
}

// ----- RED 11 -----

#[test]
fn test_plan_task_status_serde_all_variants() {
    for variant in [
        PlanTaskStatus::Pending,
        PlanTaskStatus::InProgress,
        PlanTaskStatus::Completed,
        PlanTaskStatus::Skipped,
        PlanTaskStatus::Blocked,
        PlanTaskStatus::Failed,
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        let back: PlanTaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, back);
    }
}

#[test]
fn test_plan_task_status_serde_uses_snake_case() {
    let json = serde_json::to_string(&PlanTaskStatus::InProgress).unwrap();
    assert_eq!(json, "\"in_progress\"");
}

// ----- RED 12 -----

#[test]
fn test_plan_validate_accepts_valid_plan() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    plan.validate().unwrap();
}

// ----- additional sanity checks -----

#[test]
fn test_plan_validate_rejects_self_dependency() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![1])],
    )]);
    let err = plan.validate().unwrap_err();
    assert_eq!(err, PlanValidationError::SelfDependency(PlanTaskId(1)));
}

#[test]
fn test_plan_validate_rejects_duplicate_phase_ids() {
    let plan = make_plan(vec![
        phase(1, PhaseStatus::InProgress, vec![task(1, PlanTaskStatus::Pending, vec![])]),
        phase(1, PhaseStatus::Pending, vec![task(2, PlanTaskStatus::Pending, vec![])]),
    ]);
    let err = plan.validate().unwrap_err();
    assert_eq!(err, PlanValidationError::DuplicatePhaseId(PhaseId(1)));
}

#[test]
fn test_plan_validate_rejects_invalid_phase_ref() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    plan.current_phase = PhaseId(99);
    let err = plan.validate().unwrap_err();
    assert_eq!(err, PlanValidationError::InvalidPhaseRef(PhaseId(99)));
}

#[test]
fn test_plan_validate_rejects_empty_plan() {
    let plan = make_plan(vec![]);
    let err = plan.validate().unwrap_err();
    assert_eq!(err, PlanValidationError::EmptyPlan);
}

#[test]
fn test_plan_validate_rejects_empty_title() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    plan.title = String::new();
    let err = plan.validate().unwrap_err();
    assert_eq!(err, PlanValidationError::EmptyTitle);
}

#[test]
fn test_topological_order_is_deterministic_with_ties() {
    // Two ready tasks at every step — order must be by ID ascending.
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![]),
            task(3, PlanTaskStatus::Pending, vec![]),
        ],
    )]);
    let order = plan.topological_order().unwrap();
    assert_eq!(order, vec![PlanTaskId(1), PlanTaskId(2), PlanTaskId(3)]);
}

#[test]
fn test_next_actionable_task_skipped_dep_is_satisfied() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Skipped, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    let next = plan.next_actionable_task().unwrap();
    assert_eq!(next.id, PlanTaskId(2));
}

#[test]
fn test_next_actionable_task_failed_dep_blocks_downstream() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Failed, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    // T2 cannot run because T1 failed (and T1 is not actionable either,
    // it's terminal). Result: None.
    assert!(plan.next_actionable_task().is_none());
}

#[test]
fn test_find_task_round_trip() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(7, PlanTaskStatus::Pending, vec![])],
    )]);
    assert!(plan.find_task(PlanTaskId(7)).is_some());
    assert!(plan.find_task(PlanTaskId(99)).is_none());
}

#[test]
fn test_task_to_agent_prompt_contains_metadata() {
    let plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![PlanTask {
            id: PlanTaskId(1),
            title: "Implement X".into(),
            status: PlanTaskStatus::Pending,
            files: vec!["src/main.rs".into()],
            description: "Add struct".into(),
            dod: "Tests pass".into(),
            depends_on: vec![],
            rationale: "Because".into(),
            outcome: None,
            assignee: None,
            failure_count: 0,
        }],
    )]);
    let task = &plan.phases[0].tasks[0];
    let prompt = plan.task_to_agent_prompt(task);
    assert!(prompt.contains("T1: Implement X"));
    assert!(prompt.contains("src/main.rs"));
    assert!(prompt.contains("Tests pass"));
    assert!(prompt.contains("Because"));
    assert!(prompt.contains("Demonstrate planning"));
}

#[test]
fn plan_status_terminal_helpers_are_consistent() {
    assert!(PlanTaskStatus::Completed.is_terminal());
    assert!(PlanTaskStatus::Failed.is_terminal());
    assert!(PlanTaskStatus::Skipped.is_terminal());
    assert!(!PlanTaskStatus::Pending.is_terminal());
    assert!(!PlanTaskStatus::InProgress.is_terminal());
    assert!(!PlanTaskStatus::Blocked.is_terminal());

    assert!(PlanTaskStatus::Completed.satisfies_dependency());
    assert!(PlanTaskStatus::Skipped.satisfies_dependency());
    assert!(!PlanTaskStatus::Failed.satisfies_dependency());
    assert!(!PlanTaskStatus::Pending.satisfies_dependency());
}

// ---------------------------------------------------------------------
// T6.1 — PlanPatch + apply_patch
// ---------------------------------------------------------------------


