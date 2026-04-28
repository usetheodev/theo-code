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
async fn t61_plan_failure_status_id_and_category() {
    let t = PlanFailureStatusTool::new();
    assert_eq!(t.id(), "plan_failure_status");
    assert_eq!(t.category(), ToolCategory::Orchestration);
}

#[tokio::test]
async fn t61_plan_failure_status_schema_validates() {
    let t = PlanFailureStatusTool::new();
    let schema = t.schema();
    schema.validate().unwrap();
    let threshold = schema
        .params
        .iter()
        .find(|p| p.name == "threshold")
        .unwrap();
    assert!(!threshold.required, "threshold must be optional");
}

#[tokio::test]
async fn t61_plan_failure_status_no_plan_returns_zero_stuck_tasks() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();
    let result = PlanFailureStatusTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    assert_eq!(result.metadata["stuck_count"], 0);
    assert!(result.title.contains("no plan"));
}

#[tokio::test]
async fn t61_plan_failure_status_default_threshold_is_3() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let _ = create_plan_with_failures(dir.path(), &ctx).await;
    let mut perms = PermissionCollector::new();
    let result = PlanFailureStatusTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    // Task 1 (4 failures) is at-or-above threshold 3 → listed.
    // Task 2 (1 failure) is below → omitted.
    assert_eq!(result.metadata["threshold"], 3);
    assert_eq!(result.metadata["stuck_count"], 1);
    let stuck = result.metadata["stuck_tasks"].as_array().unwrap();
    assert_eq!(stuck[0]["task_id"], 1);
    assert_eq!(stuck[0]["failure_count"], 4);
}

#[tokio::test]
async fn t61_plan_failure_status_threshold_1_lists_every_failed_task() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let _ = create_plan_with_failures(dir.path(), &ctx).await;
    let mut perms = PermissionCollector::new();
    let result = PlanFailureStatusTool::new()
        .execute(json!({"threshold": 1}), &ctx, &mut perms)
        .await
        .unwrap();
    // Both task 1 (4) and task 2 (1) reach >= 1 failure.
    assert_eq!(result.metadata["stuck_count"], 2);
}

#[tokio::test]
async fn t61_plan_failure_status_high_threshold_returns_empty() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let _ = create_plan_with_failures(dir.path(), &ctx).await;
    let mut perms = PermissionCollector::new();
    let result = PlanFailureStatusTool::new()
        .execute(json!({"threshold": 99}), &ctx, &mut perms)
        .await
        .unwrap();
    assert_eq!(result.metadata["stuck_count"], 0);
    // The "healthy plan" message points the agent at plan_next_task.
    assert!(result.output.contains("plan_next_task"));
}

#[tokio::test]
async fn t61_plan_failure_status_output_includes_actionable_next_step() {
    // Output must mention plan_replan + the available patch
    // shapes so the agent can self-replan without prompting.
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let _ = create_plan_with_failures(dir.path(), &ctx).await;
    let mut perms = PermissionCollector::new();
    let result = PlanFailureStatusTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    assert!(result.output.contains("plan_replan"));
    assert!(result.output.contains("SkipTask"));
    assert!(result.output.contains("EditTask"));
}

#[tokio::test]
async fn t61_plan_failure_status_includes_task_outcome_when_present() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let _ = create_plan_with_failures(dir.path(), &ctx).await;
    // Inject an outcome on task 1 so the rendered output should
    // surface it.
    let path = plan_path(dir.path());
    let mut plan = read_plan(&path).unwrap();
    if let Some(task) = plan.find_task_mut(theo_domain::identifiers::PlanTaskId(1)) {
        task.outcome = Some("compilation failed: undefined symbol foo".into());
    }
    write_plan(&path, &plan).unwrap();
    let mut perms = PermissionCollector::new();
    let result = PlanFailureStatusTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    assert!(result.output.contains("last outcome"));
    assert!(result.output.contains("compilation failed"));
}

