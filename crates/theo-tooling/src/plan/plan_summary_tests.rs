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
async fn test_tool_plan_summary_returns_markdown() {
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

    let result = GetPlanSummaryTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    assert_eq!(result.metadata["exists"], true);
    assert!(result.output.contains("# Demo"));
    assert!(result.output.contains("Phase 1"));
}

#[tokio::test]
async fn test_tool_plan_summary_when_no_plan() {
    let dir = tempdir().unwrap();
    let ctx = make_ctx(dir.path().to_path_buf());
    let mut perms = PermissionCollector::new();

    let result = GetPlanSummaryTool::new()
        .execute(json!({}), &ctx, &mut perms)
        .await
        .unwrap();
    assert_eq!(result.metadata["exists"], false);
}

// ---- AdvancePhase ----

