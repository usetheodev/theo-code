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
// ReplanTool — `plan_replan` (T6.1)
// ---------------------------------------------------------------------------

pub struct ReplanTool;
impl Default for ReplanTool {
    fn default() -> Self {
        Self
    }
}
impl ReplanTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReplanTool {
    fn id(&self) -> &str {
        "plan_replan"
    }

    fn description(&self) -> &str {
        "T6.1 — Apply a typed PlanPatch to the persisted plan to recover \
         from a stuck task. The patch must be one of: \
         {kind:'add_task', phase:u32, task:{...}, position?:'end'|'begin'|{after_task:u32}}, \
         {kind:'remove_task', id:u32}, \
         {kind:'edit_task', id:u32, edits:{...}}, \
         {kind:'reorder_deps', id:u32, new_deps:[u32]}, \
         {kind:'skip_task', id:u32, rationale:string}. \
         The plan is re-validated after the patch — invalid mutations \
         (cycle, orphan dep, etc.) are rejected and the plan is left \
         untouched. Use SkipTask when you cannot make progress on a \
         failed task. Example: \
         plan_replan({patch: {kind:'skip_task', id: 3, rationale: 'API deprecated'}})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "patch".into(),
                param_type: "object".into(),
                description:
                    "PlanPatch JSON object with `kind` discriminator (add_task|remove_task|edit_task|reorder_deps|skip_task)."
                        .into(),
                required: true,
            }],
            input_examples: vec![
                json!({"patch": {"kind": "skip_task", "id": 3, "rationale": "Out of scope"}}),
                json!({
                    "patch": {
                        "kind": "edit_task",
                        "id": 1,
                        "edits": {"status": "blocked", "rationale": "External dep failed"}
                    }
                }),
            ],
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
        let patch_value = args
            .get("patch")
            .ok_or_else(|| ToolError::InvalidArgs("missing object `patch`".into()))?;
        let patch: theo_domain::plan_patch::PlanPatch = serde_json::from_value(patch_value.clone())
            .map_err(|e| ToolError::InvalidArgs(format!("invalid `patch`: {e}")))?;

        let path = plan_path(&ctx.project_dir);
        let mut plan = read_plan(&path).map_err(|e| match e {
            ToolError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                ToolError::NotFound(
                    "plan.json missing — call plan_create before plan_replan".into(),
                )
            }
            other => other,
        })?;

        plan.apply_patch(&patch).map_err(|e| match e {
            theo_domain::plan_patch::PatchError::TaskNotFound(id) => {
                ToolError::NotFound(format!("task {id} not found in plan"))
            }
            theo_domain::plan_patch::PatchError::PhaseNotFound(id) => {
                ToolError::NotFound(format!("phase {id} not found in plan"))
            }
            other => ToolError::Execution(format!("plan patch failed: {other}")),
        })?;
        plan.updated_at = now_millis();
        write_plan(&path, &plan)?;

        // Surface a short summary for both the user UI and the model.
        let kind_label = match &patch {
            theo_domain::plan_patch::PlanPatch::AddTask { .. } => "add_task",
            theo_domain::plan_patch::PlanPatch::RemoveTask { .. } => "remove_task",
            theo_domain::plan_patch::PlanPatch::EditTask { .. } => "edit_task",
            theo_domain::plan_patch::PlanPatch::ReorderDeps { .. } => "reorder_deps",
            theo_domain::plan_patch::PlanPatch::SkipTask { .. } => "skip_task",
            _ => "unknown",
        };

        Ok(ToolOutput {
            title: format!("Plan patched: {kind_label}"),
            output: format!(
                "Patch `{kind_label}` applied. Plan now has {tasks} task(s) across {phases} phase(s).",
                tasks = plan.all_tasks().len(),
                phases = plan.phases.len()
            ),
            metadata: json!({
                "type": "plan_replan",
                "kind": kind_label,
                "version_counter": plan.version_counter,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Suppress unused warning for backward-compat helper.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub(super) fn _silence_map_plan_err(err: PlanError) -> ToolError {
    map_plan_err("plan", err)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------