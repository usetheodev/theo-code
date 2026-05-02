//! Shared test fixtures for plan_*_tests.rs sibling files (T3.6 split).
#![cfg(test)]
#![allow(unused_imports)]

use super::*;
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

pub(super) fn make_ctx(project_dir: PathBuf) -> ToolContext {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolContext {
        session_id: SessionId::new("ses_test"),
        message_id: MessageId::new(""),
        call_id: "call_test".into(),
        agent: "build".into(),
        abort: rx,
        project_dir,
        graph_context: None,
        stdout_tx: None,
    }
}

pub(super) fn sample_phase_args() -> Value {
    json!([
        {
            "id": 1,
            "title": "Setup",
            "tasks": [
                {"id": 1, "title": "Create struct", "dod": "compiles"}
            ]
        },
        {
            "id": 2,
            "title": "Tests",
            "tasks": [
                {"id": 2, "title": "Add unit test", "depends_on": [1]}
            ]
        }
    ])
}

// ---- RED 18 ----

pub(super) async fn create_plan_with_failures(
    dir: &std::path::Path,
    ctx: &ToolContext,
) -> Plan {
    let mut perms = PermissionCollector::new();
    // Build a 2-phase plan with 2 tasks.
    let _ = CreatePlanTool::new()
        .execute(
            json!({
                "title": "Demo",
                "goal": "Demo",
                "phases": sample_phase_args(),
            }),
            ctx,
            &mut perms,
        )
        .await
        .unwrap();
    // Read it back, bump failure_counts directly, write it back.
    let path = plan_path(dir);
    let mut plan = read_plan(&path).unwrap();
    // Task 1: 4 failures (above default threshold 3).
    for _ in 0..4 {
        plan.record_failure(theo_domain::identifiers::PlanTaskId(1));
    }
    // Task 2: 1 failure (below threshold).
    plan.record_failure(theo_domain::identifiers::PlanTaskId(2));
    write_plan(&path, &plan).unwrap();
    plan
}

