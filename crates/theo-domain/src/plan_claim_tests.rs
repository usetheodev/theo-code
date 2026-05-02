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
fn t71_claim_succeeds_when_unclaimed() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    let r = plan.claim_task(PlanTaskId(1), "agent-A");
    assert_eq!(r, ClaimResult::Claimed);
    assert_eq!(
        plan.find_task(PlanTaskId(1)).unwrap().assignee.as_deref(),
        Some("agent-A")
    );
    assert!(plan.version_counter > 0);
}

#[test]
fn t71_claim_already_held_returns_already_claimed() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    plan.claim_task(PlanTaskId(1), "agent-A");
    let r = plan.claim_task(PlanTaskId(1), "agent-B");
    match r {
        ClaimResult::AlreadyClaimed { by } => assert_eq!(by, "agent-A"),
        other => panic!("expected AlreadyClaimed, got {other:?}"),
    }
}

#[test]
fn t71_claim_self_is_idempotent() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    let _ = plan.claim_task(PlanTaskId(1), "agent-A");
    let v_after_first = plan.version_counter;
    let r = plan.claim_task(PlanTaskId(1), "agent-A");
    assert_eq!(r, ClaimResult::Claimed);
    // Second claim by same agent does NOT bump counter (no mutation).
    assert_eq!(plan.version_counter, v_after_first);
}

#[test]
fn t71_claim_unknown_id_returns_not_found() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    let r = plan.claim_task(PlanTaskId(99), "agent-A");
    assert_eq!(r, ClaimResult::NotFound);
}

#[test]
fn t71_claim_terminal_task_returns_terminal() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Completed, vec![])],
    )]);
    let r = plan.claim_task(PlanTaskId(1), "agent-A");
    assert_eq!(r, ClaimResult::Terminal);
}

#[test]
fn t71_release_clears_assignee_when_owner_matches() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    plan.claim_task(PlanTaskId(1), "agent-A");
    let v_before_release = plan.version_counter;
    assert!(plan.release_task(PlanTaskId(1), "agent-A"));
    assert!(plan.find_task(PlanTaskId(1)).unwrap().assignee.is_none());
    assert!(plan.version_counter > v_before_release);
}

#[test]
fn t71_release_by_different_agent_is_noop() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    plan.claim_task(PlanTaskId(1), "agent-A");
    let v_before = plan.version_counter;
    assert!(!plan.release_task(PlanTaskId(1), "agent-B"));
    assert_eq!(
        plan.find_task(PlanTaskId(1)).unwrap().assignee.as_deref(),
        Some("agent-A")
    );
    assert_eq!(plan.version_counter, v_before);
}

#[test]
fn t71_release_unknown_or_unclaimed_is_noop() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    assert!(!plan.release_task(PlanTaskId(99), "agent-A"));
    assert!(!plan.release_task(PlanTaskId(1), "agent-A"));
}

#[test]
fn t71_next_unclaimed_actionable_skips_assigned_tasks() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![]),
        ],
    )]);
    plan.claim_task(PlanTaskId(1), "agent-A");
    let next = plan.next_unclaimed_actionable_task().unwrap();
    assert_eq!(next.id, PlanTaskId(2));
}

#[test]
fn t71_next_unclaimed_returns_none_when_all_claimed() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![
            task(1, PlanTaskStatus::Pending, vec![]),
            task(2, PlanTaskStatus::Pending, vec![1]),
        ],
    )]);
    plan.claim_task(PlanTaskId(1), "a");
    // T2 has unsatisfied dep (T1 is in_progress not completed) → None
    assert!(plan.next_unclaimed_actionable_task().is_none());
}

#[test]
fn t71_version_counter_serde_roundtrip() {
    let mut plan = make_plan(vec![phase(
        1,
        PhaseStatus::InProgress,
        vec![task(1, PlanTaskStatus::Pending, vec![])],
    )]);
    plan.version_counter = 42;
    let json = serde_json::to_string(&plan).unwrap();
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(back.version_counter, 42);
}

#[test]
fn t71_legacy_plan_without_version_counter_loads_with_default() {
    let json = r#"{
            "version": 1,
            "title": "Legacy",
            "goal": "test",
            "current_phase": 1,
            "phases": [{
                "id": 1,
                "title": "P",
                "status": "in_progress",
                "tasks": [{"id": 1, "title": "T", "status": "pending"}]
            }],
            "created_at": 0,
            "updated_at": 0
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    assert_eq!(plan.version_counter, 0);
    assert!(plan.phases[0].tasks[0].assignee.is_none());
}

/// Backward-compat regression guard for the `sota-tier1-tier2-plan`
/// global DoD: a plan.json written before T6.1 (no `failure_count` on
/// PlanTask) and before T7.1 (no `assignee` on PlanTask, no
/// `version_counter` on Plan) MUST load under the current schema and
/// every new field MUST default deterministically. Locks the
/// `#[serde(default)]` contract on the bumped types.

#[test]
fn t71_claim_result_is_owned_predicate() {
    assert!(ClaimResult::Claimed.is_owned());
    assert!(!ClaimResult::NotFound.is_owned());
    assert!(!ClaimResult::Terminal.is_owned());
    assert!(!ClaimResult::AlreadyClaimed { by: "x".into() }.is_owned());
}

