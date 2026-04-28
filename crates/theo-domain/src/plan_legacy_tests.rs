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
fn pre_t6_t7_legacy_plan_json_loads_with_all_new_fields_at_default() {
    // Wire shape from a hypothetical theo build at HEAD~ before T6.1
    // and T7.1. Only canonical pre-bump fields included.
    let json = r#"{
            "version": 1,
            "title": "pre-bump",
            "goal": "regression guard",
            "current_phase": 1,
            "phases": [
                {
                    "id": 1,
                    "title": "Phase 1",
                    "status": "in_progress",
                    "tasks": [
                        {"id": 1, "title": "Task 1", "status": "pending"},
                        {"id": 2, "title": "Task 2", "status": "completed",
                         "depends_on": [1]}
                    ]
                }
            ],
            "created_at": 1700000000,
            "updated_at": 1700000000
    }"#;
    let plan: Plan = serde_json::from_str(json).expect(
        "pre-T6/T7 plan JSON must remain loadable under the current schema",
    );
    // T7.1 — version_counter defaults to 0 on legacy plans.
    assert_eq!(plan.version_counter, 0);
    // T6.1 + T7.1 — every task gets defaulted new fields.
    for task in plan.phases.iter().flat_map(|p| p.tasks.iter()) {
        assert!(
            task.assignee.is_none(),
            "T7.1 assignee must default to None on legacy plans"
        );
        assert_eq!(
            task.failure_count, 0,
            "T6.1 failure_count must default to 0 on legacy plans"
        );
    }
    // Plan must still validate end-to-end.
    plan.validate().expect("legacy plan must still pass validate()");
}

