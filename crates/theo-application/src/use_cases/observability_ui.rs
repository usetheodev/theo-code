//! Observability dashboard use cases.
//!
//! Reads trajectory JSONL files from `.theo/trajectories/` and exposes
//! structured data for the desktop dashboard. Wraps the low-level reader
//! and projection primitives from `theo_agent_runtime::observability`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use theo_agent_runtime::observability::RunReport;
use theo_agent_runtime::observability::{
    project, read_trajectory, DerivedMetrics, TrajectoryProjection,
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

/// Full RunReport for a single run (all metric sections).
pub fn get_run_report(project_dir: &Path, run_id: &str) -> Result<RunReport, String> {
    let path = trajectories_base(project_dir).join(format!("{}.jsonl", run_id));
    let (envelopes, _) = read_trajectory(&path).map_err(|e| e.to_string())?;
    for env in envelopes.iter().rev() {
        if matches!(
            env.kind,
            theo_agent_runtime::observability::envelope::EnvelopeKind::Summary
        ) && let Ok(r) = serde_json::from_value::<RunReport>(env.payload.clone())
        {
            return Ok(r);
        }
    }
    Err(format!("no summary line in trajectory for run {}", run_id))
}

/// Aggregated system-wide stats across all runs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SystemStats {
    pub total_runs: usize,
    pub successful_runs: usize,
    pub failed_runs: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_tool_calls: u64,
    pub total_tool_failures: u64,
    pub total_duration_ms: u64,
    pub total_subagent_spawned: u64,
    pub total_subagent_succeeded: u64,
    pub total_errors: u64,
    pub errors_by_category: std::collections::HashMap<String, u64>,
    pub tools_by_usage: Vec<(String, u64)>,
    pub tools_by_failure_rate: Vec<(String, f64)>,
    pub avg_llm_efficiency: f64,
    pub avg_cache_hit_rate: f64,
    pub avg_iterations_per_run: f64,
    pub total_episodes_injected: u64,
    pub total_hypotheses_formed: u64,
    pub total_hypotheses_invalidated: u64,
    pub total_constraints_learned: u64,
    pub total_fingerprints_new: u64,
    pub total_fingerprints_recurrent: u64,
}

/// Aggregate all runs for a system-wide view.
pub fn get_system_stats(project_dir: &Path) -> SystemStats {
    let base = trajectories_base(project_dir);
    let Ok(entries) = std::fs::read_dir(&base) else {
        return SystemStats::default();
    };
    let mut stats = SystemStats::default();
    let mut tool_calls_map: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    let mut tool_failures_map: std::collections::HashMap<String, (u64, u64)> =
        std::collections::HashMap::new();
    let mut llm_eff_sum = 0.0;
    let mut llm_eff_count = 0;
    let mut cache_sum = 0.0;
    let mut cache_count = 0;
    let mut iter_sum = 0u64;

    for entry in entries.flatten() {
        let Some(run_id) = run_id_from_jsonl(&entry.path()) else {
            continue;
        };
        let Ok(report) = get_run_report(project_dir, &run_id) else {
            continue;
        };
        accumulate_run_report(
            &report,
            &mut stats,
            &mut tool_calls_map,
            &mut tool_failures_map,
            &mut llm_eff_sum,
            &mut llm_eff_count,
            &mut cache_sum,
            &mut cache_count,
            &mut iter_sum,
        );
    }
    finalise_stats(
        &mut stats,
        tool_calls_map,
        tool_failures_map,
        llm_eff_sum,
        llm_eff_count,
        cache_sum,
        cache_count,
        iter_sum,
    );
    stats
}

fn run_id_from_jsonl(path: &std::path::Path) -> Option<String> {
    if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
        return None;
    }
    path.file_stem().and_then(|s| s.to_str()).map(String::from)
}

#[allow(clippy::too_many_arguments)]
fn accumulate_run_report(
    report: &RunReport,
    stats: &mut SystemStats,
    tool_calls_map: &mut std::collections::HashMap<String, u64>,
    tool_failures_map: &mut std::collections::HashMap<String, (u64, u64)>,
    llm_eff_sum: &mut f64,
    llm_eff_count: &mut usize,
    cache_sum: &mut f64,
    cache_count: &mut usize,
    iter_sum: &mut u64,
) {
    stats.total_runs += 1;
    if report.loop_metrics.convergence_rate > 0.0 {
        stats.successful_runs += 1;
    } else {
        stats.failed_runs += 1;
    }
    stats.total_input_tokens += report.token_metrics.input_tokens;
    stats.total_output_tokens += report.token_metrics.output_tokens;
    stats.total_cache_read_tokens += report.token_metrics.cache_read_tokens;
    stats.total_subagent_spawned += report.subagent_metrics.spawned as u64;
    stats.total_subagent_succeeded += report.subagent_metrics.succeeded as u64;
    stats.total_errors += report.error_taxonomy.total_errors as u64;
    *iter_sum += report.loop_metrics.total_iterations as u64;

    accumulate_error_taxonomy(report, stats);
    accumulate_tool_breakdown(report, stats, tool_calls_map, tool_failures_map);
    if report.surrogate_metrics.llm_efficiency.confidence > 0.0 {
        *llm_eff_sum += report.surrogate_metrics.llm_efficiency.value;
        *llm_eff_count += 1;
    }
    if report.token_metrics.cache_hit_rate > 0.0 {
        *cache_sum += report.token_metrics.cache_hit_rate;
        *cache_count += 1;
    }
    stats.total_episodes_injected += report.memory_metrics.episodes_injected as u64;
    stats.total_hypotheses_formed += report.memory_metrics.hypotheses_formed as u64;
    stats.total_hypotheses_invalidated += report.memory_metrics.hypotheses_invalidated as u64;
    stats.total_constraints_learned += report.memory_metrics.constraints_learned as u64;
    stats.total_fingerprints_new += report.memory_metrics.failure_fingerprints_new as u64;
    stats.total_fingerprints_recurrent +=
        report.memory_metrics.failure_fingerprints_recurrent as u64;
}

fn accumulate_error_taxonomy(report: &RunReport, stats: &mut SystemStats) {
    for (k, v) in [
        ("network", report.error_taxonomy.network_errors),
        ("llm", report.error_taxonomy.llm_errors),
        ("tool", report.error_taxonomy.tool_errors),
        ("sandbox", report.error_taxonomy.sandbox_errors),
        ("budget", report.error_taxonomy.budget_errors),
        ("validation", report.error_taxonomy.validation_errors),
        ("failure_mode", report.error_taxonomy.failure_mode_errors),
        ("other", report.error_taxonomy.other_errors),
    ] {
        if v > 0 {
            *stats.errors_by_category.entry(k.into()).or_insert(0) += v as u64;
        }
    }
}

fn accumulate_tool_breakdown(
    report: &RunReport,
    stats: &mut SystemStats,
    tool_calls_map: &mut std::collections::HashMap<String, u64>,
    tool_failures_map: &mut std::collections::HashMap<String, (u64, u64)>,
) {
    for t in &report.tool_breakdown {
        stats.total_tool_calls += t.call_count as u64;
        stats.total_tool_failures += t.failure_count as u64;
        *tool_calls_map.entry(t.tool_name.clone()).or_insert(0) += t.call_count as u64;
        let entry = tool_failures_map
            .entry(t.tool_name.clone())
            .or_insert((0, 0));
        entry.0 += t.call_count as u64;
        entry.1 += t.failure_count as u64;
    }
}

#[allow(clippy::too_many_arguments)]
fn finalise_stats(
    stats: &mut SystemStats,
    tool_calls_map: std::collections::HashMap<String, u64>,
    tool_failures_map: std::collections::HashMap<String, (u64, u64)>,
    llm_eff_sum: f64,
    llm_eff_count: usize,
    cache_sum: f64,
    cache_count: usize,
    iter_sum: u64,
) {
    let mut tools_by_usage: Vec<(String, u64)> = tool_calls_map.into_iter().collect();
    tools_by_usage.sort_by_key(|x| std::cmp::Reverse(x.1));
    stats.tools_by_usage = tools_by_usage;

    let mut tools_by_failure_rate: Vec<(String, f64)> = tool_failures_map
        .into_iter()
        .filter(|(_, (c, _))| *c > 0)
        .map(|(k, (c, f))| (k, f as f64 / c as f64))
        .collect();
    tools_by_failure_rate.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    stats.tools_by_failure_rate = tools_by_failure_rate;

    stats.avg_llm_efficiency = if llm_eff_count > 0 {
        llm_eff_sum / llm_eff_count as f64
    } else {
        0.0
    };
    stats.avg_cache_hit_rate = if cache_count > 0 {
        cache_sum / cache_count as f64
    } else {
        0.0
    };
    stats.avg_iterations_per_run = if stats.total_runs > 0 {
        iter_sum as f64 / stats.total_runs as f64
    } else {
        0.0
    };
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
