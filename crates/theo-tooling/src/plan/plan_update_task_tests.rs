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
async fn test_tool_plan_update_task_changes_status() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    // First create a plan.
    CreatePlanTool::new()
        .execute(
            json!({
                "title": "Demo",
                "goal": "Demonstrate",
                "phases": sample_phase_args(),
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    // Now update task 1 → completed.
    UpdateTaskTool::new()
        .execute(
            json!({"task_id": 1, "status": "completed", "outcome": "Done"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    let plan = read_plan(&plan_path(dir.path())).unwrap();
    let task = plan.find_task(PlanTaskId(1)).unwrap();
    assert_eq!(task.status, PlanTaskStatus::Completed);
    assert_eq!(task.outcome.as_deref(), Some("Done"));
}

#[tokio::test]
async fn test_tool_plan_update_task_unknown_id_returns_not_found() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    CreatePlanTool::new()
        .execute(
            json!({
                "title": "Demo",
                "goal": "Demonstrate",
                "phases": sample_phase_args(),
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    let err = UpdateTaskTool::new()
        .execute(
            json!({"task_id": 99, "status": "completed"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::NotFound(_)));
}

#[tokio::test]
async fn test_tool_plan_update_task_invalid_status() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    CreatePlanTool::new()
        .execute(
            json!({
                "title": "Demo",
                "goal": "Demonstrate",
                "phases": sample_phase_args(),
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    let err = UpdateTaskTool::new()
        .execute(
            json!({"task_id": 1, "status": "wrong_value"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

// ---- RED 20 ----

