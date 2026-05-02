//! Side-file helpers (findings.json + progress.json) for the plan
//! `plan_log` tool. Private to the plan family. Extracted from
//! plan/mod.rs during the T1.2 split.

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use theo_domain::clock::now_millis;
use theo_domain::error::ToolError;
use theo_domain::plan::{Plan, PlanDecision};

use super::shared::*;

// ---- Side-file helpers (private) ---------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FindingsFile {
    #[serde(default = "default_findings_version")]
    pub version: u32,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default)]
    pub research: Vec<FindingEntry>,
    #[serde(default)]
    pub resources: Vec<ResourceEntry>,
}

pub fn default_findings_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingEntry {
    pub summary: String,
    pub source: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEntry {
    pub title: String,
    pub url: String,
}

pub fn read_findings(path: &Path) -> Result<FindingsFile, ToolError> {
    if !path.exists() {
        return Ok(FindingsFile {
            version: default_findings_version(),
            ..Default::default()
        });
    }
    let content = std::fs::read_to_string(path).map_err(ToolError::Io)?;
    serde_json::from_str(&content)
        .map_err(|e| ToolError::Execution(format!("invalid findings.json: {e}")))
}

pub fn write_findings(path: &Path, findings: &FindingsFile) -> Result<(), ToolError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
    }
    let json = serde_json::to_string_pretty(findings)
        .map_err(|e| ToolError::Execution(format!("serialize findings: {e}")))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes()).map_err(ToolError::Io)?;
    std::fs::rename(&temp, path).map_err(ToolError::Io)?;
    Ok(())
}

pub fn append_finding(
    path: &Path,
    summary: &str,
    source: Option<&str>,
    timestamp: u64,
) -> Result<(), ToolError> {
    let mut findings = read_findings(path)?;
    findings.research.push(FindingEntry {
        summary: summary.to_owned(),
        source: source.unwrap_or("").to_owned(),
        timestamp,
    });
    write_findings(path, &findings)
}

pub fn append_resource(path: &Path, title: &str, url: &str) -> Result<(), ToolError> {
    let mut findings = read_findings(path)?;
    findings.resources.push(ResourceEntry {
        title: title.to_owned(),
        url: url.to_owned(),
    });
    write_findings(path, &findings)
}

pub fn append_requirement(path: &Path, requirement: &str) -> Result<(), ToolError> {
    let mut findings = read_findings(path)?;
    findings.requirements.push(requirement.to_owned());
    write_findings(path, &findings)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressFile {
    #[serde(default = "default_progress_version")]
    pub version: u32,
    #[serde(default)]
    pub sessions: Vec<Value>,
    #[serde(default)]
    pub errors: Vec<ErrorEntry>,
    #[serde(default)]
    pub reboot_check: Value,
}

pub fn default_progress_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEntry {
    pub error: String,
    pub attempt: u32,
    pub resolution: String,
    pub timestamp: u64,
}

pub fn read_progress(path: &Path) -> Result<ProgressFile, ToolError> {
    if !path.exists() {
        return Ok(ProgressFile {
            version: default_progress_version(),
            ..Default::default()
        });
    }
    let content = std::fs::read_to_string(path).map_err(ToolError::Io)?;
    serde_json::from_str(&content)
        .map_err(|e| ToolError::Execution(format!("invalid progress.json: {e}")))
}

pub fn write_progress(path: &Path, progress: &ProgressFile) -> Result<(), ToolError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
    }
    let json = serde_json::to_string_pretty(progress)
        .map_err(|e| ToolError::Execution(format!("serialize progress: {e}")))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes()).map_err(ToolError::Io)?;
    std::fs::rename(&temp, path).map_err(ToolError::Io)?;
    Ok(())
}

pub fn append_error_entry(
    path: &Path,
    error: &str,
    resolution: &str,
    timestamp: u64,
) -> Result<(), ToolError> {
    let mut progress = read_progress(path)?;
    let attempt = progress
        .errors
        .iter()
        .filter(|e| e.error == error)
        .count()
        .saturating_add(1) as u32;
    progress.errors.push(ErrorEntry {
        error: error.to_owned(),
        attempt,
        resolution: resolution.to_owned(),
        timestamp,
    });
    write_progress(path, &progress)
}

pub fn append_decision(
    plan_p: &Path,
    decision: &str,
    rationale: &str,
    timestamp: u64,
) -> Result<(), ToolError> {
    let mut plan = read_plan(plan_p).map_err(|e| match e {
        ToolError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => ToolError::NotFound(
            "plan.json missing — call plan_create before plan_log(kind=decision)".into(),
        ),
        other => other,
    })?;
    plan.decisions.push(PlanDecision {
        decision: decision.to_owned(),
        rationale: rationale.to_owned(),
        timestamp,
    });
    plan.updated_at = now_millis();
    write_plan(plan_p, &plan).map_err(|e| match e {
        ToolError::Execution(msg) if msg.starts_with("plan validation failed") => {
            ToolError::Execution(msg)
        }
        other => other,
    })
}

