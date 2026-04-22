//! Observability dashboard use cases.
//!
//! Reads trajectory JSONL files from `.theo/trajectories/` and exposes
//! structured data for the desktop dashboard. Wraps the low-level reader
//! and projection primitives from `theo_agent_runtime::observability`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use theo_agent_runtime::observability::{
    project, read_trajectory, DerivedMetrics, RunReport, TrajectoryProjection,
};

const TRAJECTORY_SUBDIR: &str = ".theo/trajectories";

fn trajectories_base(project_dir: &Path) -> PathBuf {
    project_dir.join(TRAJECTORY_SUBDIR)
}

/// Summary of a run — used in the dashboard list view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub timestamp: u64,
    pub success: bool,
    pub total_steps: usize,
    pub total_tool_calls: usize,
    pub duration_ms: u64,
    pub metrics: DerivedMetrics,
}

/// List all runs with a valid trajectory JSONL in `<project_dir>/.theo/trajectories/`.
pub fn list_runs(project_dir: &Path) -> Vec<RunSummary> {
    let base = trajectories_base(project_dir);
    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let run_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let (envelopes, integrity) = match read_trajectory(&path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let projection = project(&run_id, envelopes.clone(), integrity);

        // Find optional summary envelope — last line, kind=Summary.
        let report: Option<RunReport> = envelopes
            .iter()
            .rev()
            .find(|env| {
                matches!(
                    env.kind,
                    theo_agent_runtime::observability::envelope::EnvelopeKind::Summary
                )
            })
            .and_then(|env| serde_json::from_value::<RunReport>(env.payload.clone()).ok());

        let (timestamp, duration_ms) = time_bounds(&projection);
        let total_tool_calls = projection
            .steps
            .iter()
            .filter(|s| s.event_type == "ToolCallCompleted")
            .count();
        let success = success_from_steps(&projection);

        let metrics = report
            .as_ref()
            .map(|r| r.surrogate_metrics.clone())
            .unwrap_or_default();

        out.push(RunSummary {
            run_id,
            timestamp,
            success,
            total_steps: projection.steps.len(),
            total_tool_calls,
            duration_ms,
            metrics,
        });
    }
    // Newest first.
    out.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
    out
}

/// Get the full trajectory (projection + integrity) for a single run.
pub fn get_run_trajectory(project_dir: &Path, run_id: &str) -> Result<TrajectoryProjection, String> {
    let path = trajectories_base(project_dir).join(format!("{}.jsonl", run_id));
    let (envelopes, integrity) = read_trajectory(&path).map_err(|e| e.to_string())?;
    Ok(project(run_id, envelopes, integrity))
}

/// Get only the derived metrics for a run (faster than full projection).
pub fn get_run_metrics(project_dir: &Path, run_id: &str) -> Result<DerivedMetrics, String> {
    let path = trajectories_base(project_dir).join(format!("{}.jsonl", run_id));
    let (envelopes, _) = read_trajectory(&path).map_err(|e| e.to_string())?;
    for env in envelopes.iter().rev() {
        if matches!(
            env.kind,
            theo_agent_runtime::observability::envelope::EnvelopeKind::Summary
        ) && let Ok(r) = serde_json::from_value::<RunReport>(env.payload.clone())
        {
            return Ok(r.surrogate_metrics);
        }
    }
    Ok(DerivedMetrics::default())
}

/// Get derived metrics for a batch of runs for side-by-side comparison.
pub fn compare_runs(project_dir: &Path, run_ids: &[String]) -> Vec<DerivedMetrics> {
    run_ids
        .iter()
        .map(|id| get_run_metrics(project_dir, id).unwrap_or_default())
        .collect()
}

fn time_bounds(proj: &TrajectoryProjection) -> (u64, u64) {
    let timestamp = proj.steps.first().map(|s| s.timestamp).unwrap_or(0);
    let duration_ms = proj
        .steps
        .last()
        .map(|s| s.timestamp.saturating_sub(timestamp))
        .unwrap_or(0);
    (timestamp, duration_ms)
}

fn success_from_steps(proj: &TrajectoryProjection) -> bool {
    proj.steps.iter().rev().any(|s| {
        s.event_type == "RunStateChanged"
            && s.payload_summary.to_lowercase().contains("converged")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_project() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join(TRAJECTORY_SUBDIR);
        std::fs::create_dir_all(&base).unwrap();
        (tmp, base)
    }

    #[test]
    fn test_list_runs_returns_empty_when_no_trajectories() {
        let tmp = tempfile::tempdir().unwrap();
        let runs = list_runs(tmp.path());
        assert!(runs.is_empty());
    }

    #[test]
    fn test_list_runs_parses_summary_from_jsonl() {
        use theo_agent_runtime::observability::envelope::{EnvelopeKind, TrajectoryEnvelope};
        let (tmp, base) = tmp_project();
        let run_id = "run-x";
        let path = base.join(format!("{}.jsonl", run_id));
        let mut lines = Vec::new();
        let env = TrajectoryEnvelope {
            v: 1,
            seq: 0,
            ts: 100,
            run_id: run_id.into(),
            kind: EnvelopeKind::Event,
            event_type: Some("ToolCallCompleted".into()),
            event_kind: None,
            entity_id: Some("e".into()),
            payload: serde_json::json!({}),
            dropped_since_last: 0,
        };
        lines.push(serde_json::to_string(&env).unwrap());
        std::fs::write(&path, lines.join("\n")).unwrap();
        let runs = list_runs(tmp.path());
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, run_id);
    }

    #[test]
    fn test_get_run_trajectory_not_found_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let r = get_run_trajectory(tmp.path(), "nope");
        assert!(r.is_err());
    }

    #[test]
    fn test_get_run_trajectory_returns_projection() {
        use theo_agent_runtime::observability::envelope::{EnvelopeKind, TrajectoryEnvelope};
        let (tmp, base) = tmp_project();
        let run_id = "run-y";
        let path = base.join(format!("{}.jsonl", run_id));
        let env = TrajectoryEnvelope {
            v: 1,
            seq: 0,
            ts: 100,
            run_id: run_id.into(),
            kind: EnvelopeKind::Event,
            event_type: Some("RunInitialized".into()),
            event_kind: None,
            entity_id: Some("r".into()),
            payload: serde_json::json!({}),
            dropped_since_last: 0,
        };
        std::fs::write(&path, serde_json::to_string(&env).unwrap()).unwrap();
        let proj = get_run_trajectory(tmp.path(), run_id).unwrap();
        assert_eq!(proj.steps.len(), 1);
    }
}
