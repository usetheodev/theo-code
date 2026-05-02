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
// AdvancePhaseTool — `plan_advance_phase`
// ---------------------------------------------------------------------------

pub struct AdvancePhaseTool;
impl Default for AdvancePhaseTool {
    fn default() -> Self {
        Self
    }
}
impl AdvancePhaseTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for AdvancePhaseTool {
    fn id(&self) -> &str {
        "plan_advance_phase"
    }

    fn description(&self) -> &str {
        "Mark the current phase as Completed and move `current_phase` to the \
         next phase (by ascending phase ID). No-op when already at the last phase \
         (returns success with `last_phase: true`). Persists atomically to \
         plan.json. Use this AFTER all tasks in the current phase reach a \
         terminal status. Example: plan_advance_phase({})."
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
        let mut plan = read_plan(&path)?;

        let current_id = plan.current_phase;
        // Sort phase IDs ascending; pick the next id strictly greater than current.
        let mut ids: Vec<PhaseId> = plan.phases.iter().map(|p| p.id).collect();
        ids.sort();
        let next_phase = ids.into_iter().find(|id| *id > current_id);

        if let Some(curr) = plan.phases.iter_mut().find(|p| p.id == current_id) {
            curr.status = PhaseStatus::Completed;
        }
        let last_phase = next_phase.is_none();
        let new_current = next_phase.unwrap_or(current_id);
        if let Some(next) = plan.phases.iter_mut().find(|p| p.id == new_current)
            && next.status == PhaseStatus::Pending
        {
            next.status = PhaseStatus::InProgress;
        }
        plan.current_phase = new_current;
        plan.updated_at = now_millis();
        write_plan(&path, &plan)?;

        Ok(ToolOutput {
            title: if last_phase {
                format!("Phase {current_id} completed (last phase)")
            } else {
                format!("Advanced from {current_id} to {new_current}")
            },
            output: format!(
                "Marked {current_id} as completed. Current phase: {new_current}{}",
                if last_phase { " (last)" } else { "" }
            ),
            metadata: json!({
                "type": "plan_advance_phase",
                "from": current_id.as_u32(),
                "to": new_current.as_u32(),
                "last_phase": last_phase,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// LogEntryTool — `plan_log` (unified findings/error/decision log)
// ---------------------------------------------------------------------------