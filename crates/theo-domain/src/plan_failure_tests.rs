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
fn t61_record_failure_increments_count_and_returns_new_value() {
    let mut plan = plan_with_two_tasks();
    assert_eq!(plan.record_failure(PlanTaskId(1)), Some(1));
    assert_eq!(plan.record_failure(PlanTaskId(1)), Some(2));
    assert_eq!(plan.record_failure(PlanTaskId(1)), Some(3));
    let task = plan
        .all_tasks()
        .into_iter()
        .find(|t| t.id == PlanTaskId(1))
        .unwrap();
    assert_eq!(task.failure_count, 3);
}

#[test]
fn t61_record_failure_returns_none_for_unknown_task() {
    let mut plan = plan_with_two_tasks();
    assert!(plan.record_failure(PlanTaskId(999)).is_none());
}

#[test]
fn t61_record_failure_bumps_version_counter() {
    let mut plan = plan_with_two_tasks();
    let v_before = plan.version_counter;
    plan.record_failure(PlanTaskId(1));
    assert_eq!(
        plan.version_counter,
        v_before + 1,
        "record_failure must bump version_counter for CAS-aware persisters"
    );
}

#[test]
fn t61_record_failure_does_not_change_task_status() {
    // Some failures should keep the task pending for retry
    // (network glitch); others should mark Failed (logic bug).
    // Decoupling lets the caller choose.
    let mut plan = plan_with_two_tasks();
    plan.record_failure(PlanTaskId(1));
    let task = plan
        .all_tasks()
        .into_iter()
        .find(|t| t.id == PlanTaskId(1))
        .unwrap();
    assert_eq!(
        task.status,
        PlanTaskStatus::Pending,
        "record_failure must not auto-mark Failed"
    );
}

#[test]
fn t61_reset_failure_count_zeroes_and_returns_true_when_present() {
    let mut plan = plan_with_two_tasks();
    plan.record_failure(PlanTaskId(1));
    plan.record_failure(PlanTaskId(1));
    assert!(plan.reset_failure_count(PlanTaskId(1)));
    let task = plan
        .all_tasks()
        .into_iter()
        .find(|t| t.id == PlanTaskId(1))
        .unwrap();
    assert_eq!(task.failure_count, 0);
}

#[test]
fn t61_reset_failure_count_returns_false_for_unknown_task() {
    let mut plan = plan_with_two_tasks();
    assert!(!plan.reset_failure_count(PlanTaskId(999)));
}

#[test]
fn t61_reset_failure_count_bumps_version_only_when_count_changed() {
    // Idempotency-like behaviour: resetting an already-zero count
    // should not bump the version (saves churn for CAS persisters
    // that re-write whenever version changes).
    let mut plan = plan_with_two_tasks();
    let v_before = plan.version_counter;
    // Task starts at failure_count=0; reset is a no-op.
    plan.reset_failure_count(PlanTaskId(1));
    assert_eq!(
        plan.version_counter, v_before,
        "no-op reset must not bump version_counter"
    );
    // Now record a failure and reset — version should bump twice.
    plan.record_failure(PlanTaskId(1)); // +1
    plan.reset_failure_count(PlanTaskId(1)); // +1
    assert_eq!(plan.version_counter, v_before + 2);
}

#[test]
fn t61_tasks_exceeding_threshold_lists_only_offenders_in_order() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![]),
            task(3, PlanTaskStatus::Pending, vec![]),
        ],
    )]);
    for _ in 0..3 {
        plan.record_failure(PlanTaskId(1));
    }
    // Task 2: 2 failures (below threshold 3)
    plan.record_failure(PlanTaskId(2));
    plan.record_failure(PlanTaskId(2));
    for _ in 0..5 {
        plan.record_failure(PlanTaskId(3));
    }
    let offenders = plan.tasks_exceeding_failure_threshold(3);
    assert_eq!(offenders, vec![PlanTaskId(1), PlanTaskId(3)]);
}

#[test]
fn t61_tasks_exceeding_threshold_zero_returns_empty_not_everyone() {
    // Threshold 0 would technically match every task. Returning
    // empty here is a safety guard so a misconfigured threshold
    // doesn't trigger replan on every fresh task.
    let plan = plan_with_two_tasks();
    assert!(plan.tasks_exceeding_failure_threshold(0).is_empty());
}

#[test]
fn t61_tasks_exceeding_threshold_high_value_returns_empty() {
    let mut plan = plan_with_two_tasks();
    plan.record_failure(PlanTaskId(1));
    // Threshold higher than any task's count → no offenders.
    assert!(plan.tasks_exceeding_failure_threshold(99).is_empty());
}

#[test]
fn t61_failure_count_round_trips_through_serde() {
    // Critical for plan.json persistence — the field must
    // serialize/deserialize with the rest of PlanTask so
    // failure history survives across agent runs.
    let mut plan = plan_with_two_tasks();
    plan.record_failure(PlanTaskId(1));
    plan.record_failure(PlanTaskId(1));
    let json = serde_json::to_string(&plan).unwrap();
    let back: Plan = serde_json::from_str(&json).unwrap();
    let task = back
        .all_tasks()
        .into_iter()
        .find(|t| t.id == PlanTaskId(1))
        .unwrap();
    assert_eq!(task.failure_count, 2);
}

#[test]
fn t61_failure_count_omitted_in_legacy_json_defaults_to_zero() {
    // Backwards-compat: a plan.json written BEFORE T6.1 has no
    // failure_count field. Loading it must succeed and default
    // to 0 for every task. Otherwise upgrades would break every
    // agent's persisted plan.
    let legacy = r#"{
            "version": 1,
            "title": "old plan",
            "goal": "",
            "current_phase": 1,
            "phases": [
                {
                    "id": 1,
                    "title": "p1",
                    "status": "in_progress",
                    "tasks": [
                        {
                            "id": 1,
                            "title": "t1",
                            "status": "pending"
                        }
                    ]
                }
            ],
            "decisions": [],
            "created_at": 0,
            "updated_at": 0
    }"#;
    let plan: Plan = serde_json::from_str(legacy).expect("legacy plan must load");
    let task = &plan.phases[0].tasks[0];
    assert_eq!(task.failure_count, 0, "missing field must default to 0");
}

