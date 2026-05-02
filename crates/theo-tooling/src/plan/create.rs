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
// CreatePlanTool — `plan_create`
// ---------------------------------------------------------------------------

pub struct CreatePlanTool;
impl Default for CreatePlanTool {
    fn default() -> Self {
        Self
    }
}
impl CreatePlanTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CreatePlanTool {
    fn id(&self) -> &str {
        "plan_create"
    }

    fn description(&self) -> &str {
        "Create a schema-validated plan and persist it to .theo/plans/plan.json. \
         Pass a title, goal, and an array of phases each containing tasks. \
         Tasks may declare `depends_on` (array of task IDs) so the runtime can \
         execute them in dependency order. Use this BEFORE starting work — the \
         pilot loop reads this file to drive iteration. Overwrites any existing \
         plan.json. Example: plan_create({title: 'Add auth', goal: 'JWT login', \
         phases: [{id: 1, title: 'Schema', tasks: [{id: 1, title: 'User model'}]}]})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "title".into(),
                    param_type: "string".into(),
                    description: "Plan title (required, non-empty)".into(),
                    required: true,
                },
                ToolParam {
                    name: "goal".into(),
                    param_type: "string".into(),
                    description: "What the plan achieves".into(),
                    required: true,
                },
                ToolParam {
                    name: "phases".into(),
                    param_type: "array".into(),
                    description:
                        "Array of phase objects: {id: u32, title: string, tasks: [...]}. \
                         Each task: {id: u32, title: string, description?, dod?, files?, depends_on?, rationale?}."
                            .into(),
                    required: true,
                },
            ],
            input_examples: vec![json!({
                "title": "Add JWT auth",
                "goal": "Users can log in via JWT bearer token",
                "phases": [{
                    "id": 1,
                    "title": "Schema",
                    "tasks": [{
                        "id": 1,
                        "title": "Add user model",
                        "files": ["src/models/user.rs"],
                        "dod": "User struct compiles + tests pass"
                    }]
                }, {
                    "id": 2,
                    "title": "Routes",
                    "tasks": [{
                        "id": 2,
                        "title": "POST /login",
                        "depends_on": [1],
                        "dod": "200 OK with token; 401 on bad creds"
                    }]
                }]
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
        let title = require_string(&args, "title")?;
        let goal = require_string(&args, "goal")?;
        let phases_value = args
            .get("phases")
            .ok_or_else(|| ToolError::InvalidArgs("missing array `phases`".into()))?;
        let phases: Vec<PhaseArg> = serde_json::from_value(phases_value.clone())
            .map_err(|e| ToolError::InvalidArgs(format!("invalid `phases`: {e}")))?;

        let plan = build_plan_from_args(title.clone(), goal.clone(), phases)?;
        let path = plan_path(&ctx.project_dir);
        write_plan(&path, &plan)?;

        let task_count = plan.all_tasks().len();
        Ok(ToolOutput {
            title: format!("Plan created: {} ({} tasks)", plan.title, task_count),
            output: format!(
                "Plan saved to {}\nPhases: {}\nTasks: {}",
                path.display(),
                plan.phases.len(),
                task_count
            ),
            metadata: json!({
                "type": "plan_create",
                "path": path.display().to_string(),
                "phases": plan.phases.len(),
                "tasks": task_count,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// UpdateTaskTool — `plan_update_task`
// ---------------------------------------------------------------------------