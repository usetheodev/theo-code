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
async fn test_tool_plan_create_writes_valid_json() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    let tool = CreatePlanTool::new();
    let result = tool
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
    assert!(result.output.contains("plan.json"));

    let path = plan_path(dir.path());
    assert!(path.exists());
    let plan = read_plan(&path).unwrap();
    assert_eq!(plan.title, "Demo");
    assert_eq!(plan.phases.len(), 2);
    assert_eq!(plan.all_tasks().len(), 2);
    plan.validate().unwrap();
}

#[tokio::test]
async fn test_tool_plan_create_rejects_empty_phases() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    let tool = CreatePlanTool::new();
    let err = tool
        .execute(
            json!({
                "title": "Demo",
                "goal": "Demonstrate",
                "phases": [],
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn test_tool_plan_create_rejects_invalid_dependency() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    let tool = CreatePlanTool::new();
    let err = tool
        .execute(
            json!({
                "title": "Demo",
                "goal": "Demonstrate",
                "phases": [{
                    "id": 1,
                    "title": "P1",
                    "tasks": [{"id": 1, "title": "T1", "depends_on": [99]}]
                }],
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::Execution(_)));
}

// ---- RED 19 ----

