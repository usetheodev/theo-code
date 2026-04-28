//! Plan tools — schema-validated planning surface for the LLM.
//!
//! Eight schema-validated tools backed by `theo_domain::plan::Plan` and a
//! canonical JSON file at `<project>/.theo/plans/plan.json`:
//!
//! | Tool | ID | Purpose |
//! |------|----|---------|
//! | `CreatePlanTool`     | `plan_create`         | Author a plan from JSON args. |
//! | `UpdateTaskTool`     | `plan_update_task`    | Change a task's status/outcome. |
//! | `AdvancePhaseTool`   | `plan_advance_phase`  | Mark current phase complete, move to next. |
//! | `LogEntryTool`       | `plan_log`            | Append finding/error/decision to side files. |
//! | `GetPlanSummaryTool` | `plan_summary`        | Return `Plan::to_markdown()` view. |
//! | `GetNextTaskTool`    | `plan_next_task`      | Return next actionable task via toposort. |
//! | `PlanFailureStatusTool` | `plan_failure_status` | Inspect failure/replan state. |
//! | `ReplanTool`         | `plan_replan`         | Replan when stuck. |
//!
//! Plus `PlanExitTool` (`plan_exit`) for backward compat. All filesystem
//! writes go through atomic temp+rename. Every tool validates the plan
//! after mutating it — invalid plans never reach disk.
//!
//! `theo-tooling` cannot depend on `theo-agent-runtime`, so the IO helpers
//! here mirror (without sharing) the implementation in `plan_store.rs`.
//! Pre-2026-04-28 the family lived in a single 2356-LOC mod.rs; the
//! per-tool split is T1.2 of `docs/plans/god-files-2026-07-23-plan.md`.

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolSchema,
};

mod shared;
mod side_files;

mod advance_phase;
mod create;
mod failure_status;
mod log_entry;
mod next_task;
mod replan;
mod summary;
mod update_task;

pub use advance_phase::AdvancePhaseTool;
pub use create::CreatePlanTool;
pub use failure_status::PlanFailureStatusTool;
pub use log_entry::LogEntryTool;
pub use next_task::GetNextTaskTool;
pub use replan::ReplanTool;
pub use summary::GetPlanSummaryTool;
pub use update_task::UpdateTaskTool;

// Existing PlanExitTool (kept for backward compat — registered separately)
// ---------------------------------------------------------------------------

pub struct PlanExitTool;

impl Default for PlanExitTool {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanExitTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PlanExitTool {
    fn id(&self) -> &str {
        "plan_exit"
    }

    fn description(&self) -> &str {
        "Exit plan mode (experimental)"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput {
            title: "Plan mode exit".to_string(),
            output: "Switching to build agent...".to_string(),
            metadata: json!({}),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// CreatePlanTool — `plan_create`
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
