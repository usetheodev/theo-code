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

#[test]
fn all_plan_tools_have_valid_schemas() {
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(CreatePlanTool::new()),
        Box::new(UpdateTaskTool::new()),
        Box::new(AdvancePhaseTool::new()),
        Box::new(LogEntryTool::new()),
        Box::new(GetPlanSummaryTool::new()),
        Box::new(GetNextTaskTool::new()),
        Box::new(ReplanTool::new()),
    ];
    for t in &tools {
        t.schema().validate().unwrap_or_else(|e| {
            panic!("tool `{}` has invalid schema: {}", t.id(), e)
        });
        assert_eq!(t.category(), ToolCategory::Orchestration);
    }
}

#[test]
fn plan_tool_ids_are_correct() {
    assert_eq!(CreatePlanTool::new().id(), "plan_create");
    assert_eq!(UpdateTaskTool::new().id(), "plan_update_task");
    assert_eq!(AdvancePhaseTool::new().id(), "plan_advance_phase");
    assert_eq!(LogEntryTool::new().id(), "plan_log");
    assert_eq!(GetPlanSummaryTool::new().id(), "plan_summary");
    assert_eq!(GetNextTaskTool::new().id(), "plan_next_task");
    assert_eq!(ReplanTool::new().id(), "plan_replan");
}

// ----- T6.1 ReplanTool -----

