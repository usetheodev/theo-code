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
// UpdateTaskTool — `plan_update_task`
// ---------------------------------------------------------------------------

pub struct UpdateTaskTool;
impl Default for UpdateTaskTool {
    fn default() -> Self {
        Self
    }
}
impl UpdateTaskTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for UpdateTaskTool {
    fn id(&self) -> &str {
        "plan_update_task"
    }

    fn description(&self) -> &str {
        "Update the status (and optionally the outcome summary) of a single task in \
         the persisted plan. Pass `task_id` (u32) and `status` (one of: pending, \
         in_progress, completed, skipped, blocked, failed). Optional `outcome` is a \
         free-form summary of what happened. Persists atomically to plan.json. \
         Example: plan_update_task({task_id: 3, status: 'completed', outcome: 'Tests passed'})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "task_id".into(),
                    param_type: "integer".into(),
                    description: "Task ID (u32) within the current plan".into(),
                    required: true,
                },
                ToolParam {
                    name: "status".into(),
                    param_type: "string".into(),
                    description:
                        "New status: pending, in_progress, completed, skipped, blocked, failed"
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "outcome".into(),
                    param_type: "string".into(),
                    description: "Optional summary of what happened during the task".into(),
                    required: false,
                },
            ],
            input_examples: vec![json!({
                "task_id": 3,
                "status": "completed",
                "outcome": "All 12 unit tests green; coverage held at 87%."
            })],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let task_id = require_u32(&args, "task_id")?;
        let status_str = require_string(&args, "status")?;
        let status = parse_status(&status_str)?;
        let outcome = optional_string(&args, "outcome");

        let path = plan_path(&ctx.project_dir);
        let mut plan = read_plan(&path)?;

        let id = PlanTaskId(task_id);
        let task = plan
            .find_task_mut(id)
            .ok_or_else(|| ToolError::NotFound(format!("task {id} not found in plan")))?;
        task.status = status;
        if let Some(o) = outcome.clone() {
            task.outcome = Some(o);
        }
        plan.updated_at = now_millis();

        write_plan(&path, &plan)?;

        Ok(ToolOutput {
            title: format!("Task {} → {}", id, status_str),
            output: format!("Task {id} status set to {status_str}"),
            metadata: json!({
                "type": "plan_update_task",
                "task_id": task_id,
                "status": status_str,
                "outcome": outcome,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// AdvancePhaseTool — `plan_advance_phase`
// ---------------------------------------------------------------------------