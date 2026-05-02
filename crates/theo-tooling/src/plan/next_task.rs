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
// GetNextTaskTool — `plan_next_task`
// ---------------------------------------------------------------------------

pub struct GetNextTaskTool;
impl Default for GetNextTaskTool {
    fn default() -> Self {
        Self
    }
}
impl GetNextTaskTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GetNextTaskTool {
    fn id(&self) -> &str {
        "plan_next_task"
    }

    fn description(&self) -> &str {
        "Return the next actionable task from the persisted plan, respecting the \
         dependency DAG. Returns metadata.found=false when every task is in a \
         terminal state. Use this each iteration to know what to work on. \
         Example: plan_next_task({})."
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
                metadata: json!({"type": "plan_next_task", "found": false}),
                attachments: None,
                llm_suffix: None,
            });
        }
        let plan = read_plan(&path)?;
        match plan.next_actionable_task() {
            Some(task) => {
                let prompt = plan.task_to_agent_prompt(task);
                Ok(ToolOutput {
                    title: format!("Next task: {} — {}", task.id, task.title),
                    output: prompt,
                    metadata: json!({
                        "type": "plan_next_task",
                        "found": true,
                        "task_id": task.id.as_u32(),
                        "title": task.title,
                        "depends_on": task
                            .depends_on
                            .iter()
                            .map(|d| d.as_u32())
                            .collect::<Vec<_>>(),
                    }),
                    attachments: None,
                    llm_suffix: None,
                })
            }
            None => Ok(ToolOutput {
                title: "No actionable task".into(),
                output: "All tasks are in terminal states (Completed/Skipped/Failed/Blocked) \
                         or blocked by failed dependencies. The plan is fully resolved or stuck."
                    .into(),
                metadata: json!({"type": "plan_next_task", "found": false}),
                attachments: None,
                llm_suffix: None,
            }),
        }
    }
}

