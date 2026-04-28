//! Shared helpers for the plan tool family.
//!
//! Disk helpers (atomic plan.json read/write) + JSON DTOs that mirror
//! the LLM's argument shapes. Extracted from plan/mod.rs during the
//! T1.2 split (god-files-2026-07-23-plan.md, ADR D2).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use theo_domain::clock::now_millis;
use theo_domain::error::ToolError;
use theo_domain::identifiers::{PhaseId, PlanTaskId};
use theo_domain::plan::{
    PLAN_FORMAT_VERSION, Phase, PhaseStatus, Plan, PlanDecision, PlanError, PlanTask,
    PlanTaskStatus,
};

// ---------------------------------------------------------------------------
// Disk helpers (private to this module — mirror of plan_store)
// ---------------------------------------------------------------------------

/// Returns `<project>/.theo/plans/plan.json`.
pub fn plan_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".theo").join("plans").join("plan.json")
}

/// Returns `<project>/.theo/plans/findings.json`.
pub fn findings_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".theo")
        .join("plans")
        .join("findings.json")
}

/// Returns `<project>/.theo/plans/progress.json`.
pub fn progress_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".theo")
        .join("plans")
        .join("progress.json")
}

pub fn map_plan_err(prefix: &str, err: PlanError) -> ToolError {
    match err {
        PlanError::Io(e) => ToolError::Io(e),
        other => ToolError::Execution(format!("{prefix}: {other}")),
    }
}

pub fn read_plan(path: &Path) -> Result<Plan, ToolError> {
    let content = std::fs::read_to_string(path).map_err(ToolError::Io)?;
    let plan: Plan = serde_json::from_str(&content)
        .map_err(|e| ToolError::Execution(format!("invalid plan JSON: {e}")))?;
    if plan.version > PLAN_FORMAT_VERSION {
        return Err(ToolError::Execution(format!(
            "unsupported plan version: found {found}, max supported {max}",
            found = plan.version,
            max = PLAN_FORMAT_VERSION
        )));
    }
    plan.validate()
        .map_err(|e| ToolError::Execution(format!("plan validation failed: {e}")))?;
    Ok(plan)
}

pub fn write_plan(path: &Path, plan: &Plan) -> Result<(), ToolError> {
    plan.validate()
        .map_err(|e| ToolError::Execution(format!("plan validation failed: {e}")))?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
    }
    let json = serde_json::to_string_pretty(plan)
        .map_err(|e| ToolError::Execution(format!("serialize plan: {e}")))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes()).map_err(ToolError::Io)?;
    std::fs::rename(&temp, path).map_err(ToolError::Io)?;
    Ok(())
}

pub fn require_string(args: &Value, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ToolError::InvalidArgs(format!("missing string `{key}`")))
}

pub fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub fn require_u32(args: &Value, key: &str) -> Result<u32, ToolError> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "missing or non-u32 integer `{key}`"
            ))
        })
}

// ---------------------------------------------------------------------------
// Argument parsing helpers — JSON schema-shaped DTOs
// ---------------------------------------------------------------------------

/// Inline DTO shaped to match how the LLM is expected to author phases.
/// Decoupled from `theo_domain::plan::Phase` so tool args stay stable
/// across schema evolutions.
#[derive(Debug, Deserialize, Serialize)]
pub struct PhaseArg {
    pub id: u32,
    pub title: String,
    #[serde(default)]
    pub tasks: Vec<TaskArg>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TaskArg {
    pub id: u32,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub dod: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<u32>,
    #[serde(default)]
    pub rationale: String,
}

pub fn build_plan_from_args(
    title: String,
    goal: String,
    phases: Vec<PhaseArg>,
) -> Result<Plan, ToolError> {
    if phases.is_empty() {
        return Err(ToolError::InvalidArgs(
            "plan must have at least one phase".into(),
        ));
    }
    let current_phase = PhaseId(phases[0].id);
    let domain_phases: Vec<Phase> = phases
        .into_iter()
        .map(|p| Phase {
            id: PhaseId(p.id),
            title: p.title,
            status: PhaseStatus::Pending,
            tasks: p
                .tasks
                .into_iter()
                .map(|t| PlanTask {
                    id: PlanTaskId(t.id),
                    title: t.title,
                    status: PlanTaskStatus::Pending,
                    files: t.files,
                    description: t.description,
                    dod: t.dod,
                    depends_on: t.depends_on.into_iter().map(PlanTaskId).collect(),
                    rationale: t.rationale,
                    outcome: None,
                    assignee: None,
                    failure_count: 0,
                })
                .collect(),
        })
        .collect();

    let now = now_millis();
    Ok(Plan {
        version: PLAN_FORMAT_VERSION,
        title,
        goal,
        current_phase,
        phases: domain_phases,
        decisions: vec![],
        created_at: now,
        updated_at: now,
        version_counter: 0,
    })
}

pub fn parse_status(s: &str) -> Result<PlanTaskStatus, ToolError> {
    match s {
        "pending" => Ok(PlanTaskStatus::Pending),
        "in_progress" => Ok(PlanTaskStatus::InProgress),
        "completed" => Ok(PlanTaskStatus::Completed),
        "skipped" => Ok(PlanTaskStatus::Skipped),
        "blocked" => Ok(PlanTaskStatus::Blocked),
        "failed" => Ok(PlanTaskStatus::Failed),
        other => Err(ToolError::InvalidArgs(format!(
            "invalid status `{other}`. Use one of: pending, in_progress, completed, skipped, blocked, failed"
        ))),
    }
}

// ---------------------------------------------------------------------------
