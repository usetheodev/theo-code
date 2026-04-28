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

pub struct PlanFailureStatusTool;

impl Default for PlanFailureStatusTool {
    fn default() -> Self {
        Self
    }
}

impl PlanFailureStatusTool {
    pub fn new() -> Self {
        Self
    }
}

const DEFAULT_FAILURE_THRESHOLD: u32 = 3;

#[async_trait]
impl Tool for PlanFailureStatusTool {
    fn id(&self) -> &str {
        "plan_failure_status"
    }

    fn description(&self) -> &str {
        "T6.1 — List tasks in the persisted plan whose failure_count meets or \
         exceeds `threshold` (default 3). Each entry carries id, title, \
         failure_count, status, and recent outcome — exactly the information \
         the agent needs to decide whether to call `plan_replan` with a \
         SkipTask, EditTask, or RemoveTask patch. The pilot's auto-replan \
         trigger logs threshold breaches; this tool lets the agent itself \
         observe the same state and act. \
         Example: plan_failure_status({}) or plan_failure_status({threshold: 5})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "threshold".into(),
                param_type: "integer".into(),
                description:
                    "Minimum failure_count to include. Default 3 (matches the \
                     pilot's `replan_failure_threshold`). Pass 1 to see EVERY \
                     task that has failed at least once."
                        .into(),
                required: false,
            }],
            input_examples: vec![
                json!({}),
                json!({"threshold": 1}),
                json!({"threshold": 5}),
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
        let threshold = args
            .get("threshold")
            .and_then(Value::as_u64)
            .map(|n| n as u32)
            .unwrap_or(DEFAULT_FAILURE_THRESHOLD);

        let path = plan_path(&ctx.project_dir);
        if !path.exists() {
            return Ok(ToolOutput {
                title: "plan_failure_status: no plan".into(),
                output: "No plan.json found at .theo/plans/plan.json. Call \
                         plan_create to author one."
                    .into(),
                metadata: json!({
                    "type": "plan_failure_status",
                    "threshold": threshold,
                    "stuck_count": 0,
                    "stuck_tasks": [],
                }),
                attachments: None,
                llm_suffix: None,
            });
        }

        let plan = read_plan(&path)?;
        let offender_ids = plan.tasks_exceeding_failure_threshold(threshold);
        let by_id: std::collections::HashMap<_, _> = plan
            .all_tasks()
            .into_iter()
            .map(|t| (t.id, t.clone()))
            .collect();

        let mut entries: Vec<Value> = Vec::with_capacity(offender_ids.len());
        for id in &offender_ids {
            if let Some(task) = by_id.get(id) {
                entries.push(json!({
                    "task_id": id.as_u32(),
                    "title": task.title,
                    "failure_count": task.failure_count,
                    "status": format!("{:?}", task.status),
                    "outcome": task.outcome.as_deref().unwrap_or(""),
                    "depends_on": task.depends_on.iter().map(|d| d.as_u32()).collect::<Vec<_>>(),
                }));
            }
        }

        if entries.is_empty() {
            return Ok(ToolOutput {
                title: format!(
                    "plan_failure_status: 0 task(s) ≥ {threshold} failures"
                ),
                output: format!(
                    "No tasks have reached the failure threshold ({threshold}). \
                     The plan is healthy. Check `plan_next_task` for the \
                     next actionable item."
                ),
                metadata: json!({
                    "type": "plan_failure_status",
                    "threshold": threshold,
                    "stuck_count": 0,
                    "stuck_tasks": [],
                }),
                attachments: None,
                llm_suffix: None,
            });
        }

        let mut output = format!(
            "plan_failure_status: {n} task(s) at or above threshold {threshold}\n\n",
            n = entries.len(),
        );
        for e in &entries {
            output.push_str(&format!(
                "  - id={id}  failures={fc}  status={st}\n    title: {title}\n",
                id = e["task_id"],
                fc = e["failure_count"],
                st = e["status"].as_str().unwrap_or("?"),
                title = e["title"].as_str().unwrap_or(""),
            ));
            let outcome = e["outcome"].as_str().unwrap_or("");
            if !outcome.is_empty() {
                let preview: String = outcome.chars().take(120).collect();
                let suffix = if outcome.chars().count() > 120 {
                    "…"
                } else {
                    ""
                };
                output.push_str(&format!("    last outcome: {preview}{suffix}\n"));
            }
        }
        output.push_str(
            "\nNext: call `plan_replan` with a SkipTask (unrecoverable), \
             EditTask (clarify dod/files), or RemoveTask patch for one of \
             the offenders.",
        );

        Ok(ToolOutput {
            title: format!(
                "plan_failure_status: {} stuck task(s)",
                entries.len()
            ),
            output,
            metadata: json!({
                "type": "plan_failure_status",
                "threshold": threshold,
                "stuck_count": entries.len(),
                "stuck_tasks": entries,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// ReplanTool — `plan_replan` (T6.1)
// ---------------------------------------------------------------------------