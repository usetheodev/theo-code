//! Sibling test body of `plan/mod.rs` — split per-tool (T3.6 of code-hygiene-5x5).

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::plan_test_helpers::*;
use super::*;
use std::path::PathBuf;
use tempfile::tempdir;
use theo_domain::clock::now_millis;
use theo_domain::error::ToolError;
use theo_domain::identifiers::{PhaseId, PlanTaskId};
use theo_domain::plan::{
    Phase, PhaseStatus, Plan, PlanDecision, PlanError, PlanTask, PlanTaskStatus,
};
use theo_domain::session::{MessageId, SessionId};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput,
};
use crate::plan::shared::{
    findings_path, plan_path, progress_path, read_plan, write_plan,
};
use crate::plan::side_files::{
    FindingsFile, ProgressFile, append_decision, append_error_entry, append_finding,
    append_requirement, append_resource,
};

#[tokio::test]
async fn t61_replan_tool_skip_task_persists_changes() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    // Seed a plan with one Pending task.
    CreatePlanTool::new()
        .execute(
            json!({
                "title": "T61",
                "goal": "test replan",
                "phases": sample_phase_args(),
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    // Skip task 1.
    let result = ReplanTool::new()
        .execute(
            json!({"patch": {"kind": "skip_task", "id": 1, "rationale": "Out of scope"}}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();
    assert_eq!(result.metadata["kind"], "skip_task");

    // Re-read plan and confirm task 1 is now Skipped with the rationale.
    let plan = read_plan(&plan_path(dir.path())).unwrap();
    let t = plan.find_task(PlanTaskId(1)).unwrap();
    assert_eq!(t.status, PlanTaskStatus::Skipped);
    assert_eq!(t.outcome.as_deref(), Some("Out of scope"));
}

#[tokio::test]
async fn t61_replan_tool_unknown_task_returns_not_found() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    CreatePlanTool::new()
        .execute(
            json!({
                "title": "T61",
                "goal": "test replan",
                "phases": sample_phase_args(),
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    let err = ReplanTool::new()
        .execute(
            json!({"patch": {"kind": "skip_task", "id": 99, "rationale": "x"}}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::NotFound(_)));
}

#[tokio::test]
async fn t61_replan_tool_invalid_patch_shape_returns_invalid_args() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    CreatePlanTool::new()
        .execute(
            json!({
                "title": "T61",
                "goal": "test replan",
                "phases": sample_phase_args(),
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    let err = ReplanTool::new()
        .execute(
            json!({"patch": {"kind": "skip_task"}}), // missing id + rationale
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t61_replan_tool_missing_plan_returns_not_found() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    let err = ReplanTool::new()
        .execute(
            json!({"patch": {"kind": "skip_task", "id": 1, "rationale": "x"}}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::NotFound(_)));
}

#[tokio::test]
async fn t61_replan_tool_cycle_introducing_patch_rolls_back() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    // Plan: t1 → t2 (t2 depends on t1).
    CreatePlanTool::new()
        .execute(
            json!({
                "title": "T61",
                "goal": "test rollback",
                "phases": sample_phase_args(),
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    // ReorderDeps to make t1 depend on t2 → cycle 1↔2.
    let err = ReplanTool::new()
        .execute(
            json!({
                "patch": {
                    "kind": "reorder_deps",
                    "id": 1,
                    "new_deps": [2]
                }
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::Execution(_)));

    // Disk state unchanged: t1 still has empty deps.
    let plan = read_plan(&plan_path(dir.path())).unwrap();
    let t1 = plan.find_task(PlanTaskId(1)).unwrap();
    assert!(t1.depends_on.is_empty());
}

#[tokio::test]
async fn t61_replan_tool_records_increment_in_metadata() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    CreatePlanTool::new()
        .execute(
            json!({
                "title": "T61",
                "goal": "metadata check",
                "phases": sample_phase_args(),
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    let result = ReplanTool::new()
        .execute(
            json!({"patch": {"kind": "skip_task", "id": 1, "rationale": "skip"}}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();
    // version_counter is part of the metadata so callers can correlate
    // log lines with the saved plan.
    assert!(result.metadata.get("version_counter").is_some());
}

// ── T6.1 part 4 — plan_failure_status ─────────────────────────

