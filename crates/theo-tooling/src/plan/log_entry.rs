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
// LogEntryTool — `plan_log` (unified findings/error/decision log)
// ---------------------------------------------------------------------------

pub struct LogEntryTool;
impl Default for LogEntryTool {
    fn default() -> Self {
        Self
    }
}
impl LogEntryTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for LogEntryTool {
    fn id(&self) -> &str {
        "plan_log"
    }

    fn description(&self) -> &str {
        "Append a structured log entry to one of the side files in \
         .theo/plans/. `kind` selects the destination: 'finding' → \
         findings.json#research, 'resource' → findings.json#resources, \
         'requirement' → findings.json#requirements, 'error' → \
         progress.json#errors (with attempt counter), 'decision' → \
         plan.json#decisions. `content` carries the body; `rationale` is \
         optional context. Use after web/grep searches (Manus 2-action rule), \
         on failed attempts, and when a binding choice is made. Example: \
         plan_log({kind: 'finding', content: 'serde_json supports streaming', \
         rationale: 'Found in docs.rs/serde_json'})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "kind".into(),
                    param_type: "string".into(),
                    description:
                        "Entry type: finding, resource, requirement, error, decision"
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "content".into(),
                    param_type: "string".into(),
                    description: "Log entry body".into(),
                    required: true,
                },
                ToolParam {
                    name: "rationale".into(),
                    param_type: "string".into(),
                    description: "Optional explanation/context".into(),
                    required: false,
                },
                ToolParam {
                    name: "source".into(),
                    param_type: "string".into(),
                    description:
                        "Source attribution for findings/resources (e.g. URL, doc title)"
                            .into(),
                    required: false,
                },
            ],
            input_examples: vec![
                json!({
                    "kind": "finding",
                    "content": "FastAPI uses pydantic for validation",
                    "source": "https://fastapi.tiangolo.com/"
                }),
                json!({
                    "kind": "error",
                    "content": "cargo test failed: missing feature flag",
                    "rationale": "Need to enable `tokio/full` in Cargo.toml"
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
        let kind = require_string(&args, "kind")?;
        let content = require_string(&args, "content")?;
        let rationale = optional_string(&args, "rationale");
        let source = optional_string(&args, "source");
        let now = now_millis();

        match kind.as_str() {
            "finding" => {
                let path = findings_path(&ctx.project_dir);
                append_finding(&path, &content, source.as_deref(), now)?;
            }
            "resource" => {
                let path = findings_path(&ctx.project_dir);
                let url = source
                    .ok_or_else(|| ToolError::InvalidArgs(
                        "kind=resource requires `source` (URL)".into(),
                    ))?;
                append_resource(&path, &content, &url)?;
            }
            "requirement" => {
                let path = findings_path(&ctx.project_dir);
                append_requirement(&path, &content)?;
            }
            "error" => {
                let path = progress_path(&ctx.project_dir);
                append_error_entry(
                    &path,
                    &content,
                    rationale.as_deref().unwrap_or(""),
                    now,
                )?;
            }
            "decision" => {
                let plan_p = plan_path(&ctx.project_dir);
                append_decision(
                    &plan_p,
                    &content,
                    rationale.as_deref().unwrap_or(""),
                    now,
                )?;
            }
            other => {
                return Err(ToolError::InvalidArgs(format!(
                    "invalid kind `{other}`. Use one of: finding, resource, requirement, error, decision"
                )));
            }
        }

        Ok(ToolOutput {
            title: format!("Logged {kind}"),
            output: format!("Recorded {kind}: {content}"),
            metadata: json!({
                "type": "plan_log",
                "kind": kind,
                "content": content,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}