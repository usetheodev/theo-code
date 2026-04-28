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
async fn test_tool_plan_next_task_follows_deps() {
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

    let result = GetNextTaskTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    assert_eq!(result.metadata["task_id"], 1);

    UpdateTaskTool::new()
        .execute(
            json!({"task_id": 1, "status": "completed"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    let result = GetNextTaskTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    assert_eq!(result.metadata["task_id"], 2);
}

#[tokio::test]
async fn test_tool_plan_next_task_returns_none_when_all_done() {
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

    for id in [1u32, 2u32] {
        UpdateTaskTool::new()
            .execute(
                json!({"task_id": id, "status": "completed"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
    }
    let result = GetNextTaskTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    assert_eq!(result.metadata["found"], false);
}

#[tokio::test]
async fn test_tool_plan_next_task_when_no_plan_returns_not_found_meta() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    let result = GetNextTaskTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    assert_eq!(result.metadata["found"], false);
}

// ---- RED 21 ----

