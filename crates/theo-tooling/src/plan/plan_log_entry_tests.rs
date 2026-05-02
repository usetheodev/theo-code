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
async fn test_tool_log_finding_writes_findings_json() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    LogEntryTool::new()
        .execute(
            json!({
                "kind": "finding",
                "content": "X uses Y",
                "source": "https://x.example",
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    let path = findings_path(dir.path());
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("X uses Y"));
    assert!(content.contains("x.example"));
}

#[tokio::test]
async fn test_tool_log_resource_requires_source() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    let err = LogEntryTool::new()
        .execute(
            json!({"kind": "resource", "content": "ADR-016"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn test_tool_log_error_increments_attempt() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    for _ in 0..3 {
        LogEntryTool::new()
            .execute(
                json!({
                    "kind": "error",
                    "content": "compile fail",
                    "rationale": "missing import",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
    }
    let path = progress_path(dir.path());
    let content = std::fs::read_to_string(&path).unwrap();
    let progress: ProgressFile = serde_json::from_str(&content).unwrap();
    assert_eq!(progress.errors.len(), 3);
    assert_eq!(progress.errors[0].attempt, 1);
    assert_eq!(progress.errors[1].attempt, 2);
    assert_eq!(progress.errors[2].attempt, 3);
}

#[tokio::test]
async fn test_tool_log_decision_requires_existing_plan() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    let err = LogEntryTool::new()
        .execute(
            json!({"kind": "decision", "content": "use serde"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::NotFound(_)));
}

#[tokio::test]
async fn test_tool_log_decision_appends_to_plan() {
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

    LogEntryTool::new()
        .execute(
            json!({
                "kind": "decision",
                "content": "Use sqlite",
                "rationale": "simpler than postgres for now",
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

    let plan = read_plan(&plan_path(dir.path())).unwrap();
    assert_eq!(plan.decisions.len(), 1);
    assert_eq!(plan.decisions[0].decision, "Use sqlite");
}

#[tokio::test]
async fn test_tool_log_invalid_kind() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    let err = LogEntryTool::new()
        .execute(
            json!({"kind": "bogus", "content": "x"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

// ---- Schema validation ----

