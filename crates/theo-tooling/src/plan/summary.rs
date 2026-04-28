//! Single-tool slice extracted from `plan/mod.rs` (T1.2 of
//! `docs/plans/god-files-2026-07-23-plan.md`, ADR D2 split-per-tool).

#![allow(unused_imports)]

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use theo_domain::clock::now_millis;
use theo_domain::error::ToolError;
use theo_domain::identifiers::{PhaseId, PlanTaskId};
use theo_domain::plan::{
    PLAN_FORMAT_VERSION, Phase, PhaseStatus, Plan, PlanDecision, PlanError, PlanTask,
    PlanTaskStatus,
};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

use super::shared::*;
use super::side_files::*;

// ---------------------------------------------------------------------------
// GetPlanSummaryTool — `plan_summary`
// ---------------------------------------------------------------------------

pub struct GetPlanSummaryTool;
impl Default for GetPlanSummaryTool {
    fn default() -> Self {
        Self
    }
}
impl GetPlanSummaryTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GetPlanSummaryTool {
    fn id(&self) -> &str {
        "plan_summary"
    }

    fn description(&self) -> &str {
        "Return the persisted plan as a human-readable Markdown view (read-only). \
         Use this to inject the plan into a prompt when continuing work. \
         Returns an empty string + metadata.exists=false when no plan.json is \
         present. Example: plan_summary({})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        _args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let path = plan_path(&ctx.project_dir);
        if !path.exists() {
            return Ok(ToolOutput {
                title: "No plan".into(),
                output: "No plan.json found. Call plan_create to author one.".into(),
                metadata: json!({"type": "plan_summary", "exists": false}),
                attachments: None,
                llm_suffix: None,
            });
        }
        let plan = read_plan(&path)?;
        let md = plan.to_markdown();
        let task_count = plan.all_tasks().len();
        Ok(ToolOutput {
            title: format!("Plan: {} ({} tasks)", plan.title, task_count),
            output: md,
            metadata: json!({
                "type": "plan_summary",
                "exists": true,
                "title": plan.title,
                "tasks": task_count,
                "current_phase": plan.current_phase.as_u32(),
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// GetNextTaskTool — `plan_next_task`
// ---------------------------------------------------------------------------