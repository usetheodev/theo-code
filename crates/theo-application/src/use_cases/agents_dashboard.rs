//! Per-agent dashboard use case — Phase 15 (sota-gaps-plan).
//!
//! Aggregates persisted `SubagentRun` records into per-agent statistics so
//! the dashboard frontend can render an "Agents" page with cost / success /
//! latency breakdowns. Closes SOTA gap A4 (per-agent observability) by
//! exposing data the Phase 12 metrics collector already records.
//!
//! Reads from `<project_dir>/.theo/subagent/runs/*.json` via
//! `FileSubagentRunStore::list` + `load`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use theo_agent_runtime::subagent_runs::{FileSubagentRunStore, RunStatus, SubagentRun};

const RUNS_SUBDIR: &str = ".theo/subagent";

fn store(project_dir: &Path) -> FileSubagentRunStore {
    FileSubagentRunStore::new(project_dir.join(RUNS_SUBDIR))
}

/// Aggregated stats for a single agent name (across all runs).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AgentStats {
    /// Agent name (matches `AgentSpec::name`).
    pub agent_name: String,
    /// Source: builtin | project | global | on_demand.
    pub agent_source: String,
    /// Total runs persisted for this agent.
    pub run_count: usize,
    /// How many ended in `Completed` state.
    pub success_count: usize,
    /// How many ended in `Failed` state.
    pub failure_count: usize,
    /// How many ended in `Cancelled` state.
    pub cancelled_count: usize,
    /// How many ended in `Abandoned` state (user-marked).
    pub abandoned_count: usize,
    /// How many are still `Running`.
    pub running_count: usize,
    /// Sum of tokens used across all runs.
    pub total_tokens_used: u64,
    /// Sum of iterations used across all runs.
    pub total_iterations_used: u64,
    /// Average tokens per run (zero if no runs).
    pub avg_tokens_per_run: f64,
    /// Average iterations per run.
    pub avg_iterations_per_run: f64,
    /// Success rate (success / (success + failure)) — ignores cancelled/abandoned/running.
    pub success_rate: f64,
    /// Most recent `started_at` epoch seconds.
    pub last_started_at: i64,
}

impl AgentStats {
    fn record(&mut self, run: &SubagentRun) {
        self.run_count += 1;
        self.total_tokens_used += run.tokens_used;
        self.total_iterations_used += run.iterations_used as u64;
        if run.started_at > self.last_started_at {
            self.last_started_at = run.started_at;
        }
        match run.status {
            RunStatus::Completed => self.success_count += 1,
            RunStatus::Failed => self.failure_count += 1,
            RunStatus::Cancelled => self.cancelled_count += 1,
            RunStatus::Abandoned => self.abandoned_count += 1,
            RunStatus::Running => self.running_count += 1,
        }
    }

    fn finalize(&mut self) {
        if self.run_count > 0 {
            self.avg_tokens_per_run = self.total_tokens_used as f64 / self.run_count as f64;
            self.avg_iterations_per_run = self.total_iterations_used as f64 / self.run_count as f64;
        }
        let denom = self.success_count + self.failure_count;
        if denom > 0 {
            self.success_rate = self.success_count as f64 / denom as f64;
        }
    }
}

/// Aggregate every persisted SubagentRun into per-agent stats.
/// Returns sorted by `agent_name` (stable).
pub fn list_agents(project_dir: &Path) -> Vec<AgentStats> {
    let store = store(project_dir);
    let ids = match store.list() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut by_name: BTreeMap<String, AgentStats> = BTreeMap::new();
    for id in ids {
        let run = match store.load(&id) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let entry = by_name
            .entry(run.agent_name.clone())
            .or_insert_with(|| AgentStats {
                agent_name: run.agent_name.clone(),
                agent_source: run.agent_source.clone(),
                ..Default::default()
            });
        entry.record(&run);
    }
    let mut out: Vec<AgentStats> = by_name.into_values().collect();
    for stats in &mut out {
        stats.finalize();
    }
    out
}

/// Single-agent detail: stats + the most recent N runs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentDetail {
    pub stats: AgentStats,
    pub recent_runs: Vec<RecentRun>,
}

/// Lightweight projection of a run for the dashboard list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentRun {
    pub run_id: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub iterations_used: usize,
    pub tokens_used: u64,
    pub objective: String,
    pub summary: Option<String>,
}

/// List ALL runs for a given agent name (no aggregation), sorted DESC by
/// `started_at`. Used by `/api/agents/:name/runs` for the dashboard's
/// per-agent run-history table.
pub fn list_agent_runs(project_dir: &Path, agent_name: &str) -> Vec<RecentRun> {
    let store = store(project_dir);
    let ids = match store.list() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut runs: Vec<SubagentRun> = ids
        .into_iter()
        .filter_map(|id| store.load(&id).ok())
        .filter(|r| r.agent_name == agent_name)
        .collect();
    runs.sort_by_key(|r| std::cmp::Reverse(r.started_at));
    runs.into_iter()
        .map(|r| RecentRun {
            run_id: r.run_id,
            status: format!("{:?}", r.status).to_lowercase(),
            started_at: r.started_at,
            finished_at: r.finished_at,
            iterations_used: r.iterations_used,
            tokens_used: r.tokens_used,
            objective: r.objective,
            summary: r.summary,
        })
        .collect()
}

/// Get detail for a specific agent name. Returns `None` if no runs exist.
pub fn get_agent(project_dir: &Path, agent_name: &str, limit: usize) -> Option<AgentDetail> {
    let store = store(project_dir);
    let ids = store.list().ok()?;
    let mut runs: Vec<SubagentRun> = ids
        .into_iter()
        .filter_map(|id| store.load(&id).ok())
        .filter(|r| r.agent_name == agent_name)
        .collect();
    if runs.is_empty() {
        return None;
    }
    runs.sort_by_key(|r| std::cmp::Reverse(r.started_at));

    // Build stats from full set
    let mut stats = AgentStats {
        agent_name: agent_name.to_string(),
        agent_source: runs[0].agent_source.clone(),
        ..Default::default()
    };
    for r in &runs {
        stats.record(r);
    }
    stats.finalize();

    // Project the most recent N runs
    let recent_runs: Vec<RecentRun> = runs
        .into_iter()
        .take(limit)
        .map(|r| RecentRun {
            run_id: r.run_id,
            status: format!("{:?}", r.status).to_lowercase(),
            started_at: r.started_at,
            finished_at: r.finished_at,
            iterations_used: r.iterations_used,
            tokens_used: r.tokens_used,
            objective: r.objective,
            summary: r.summary,
        })
        .collect();

    Some(AgentDetail {
        stats,
        recent_runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use theo_domain::agent_spec::AgentSpec;

    fn save_run(
        store: &FileSubagentRunStore,
        agent_name: &str,
        status: RunStatus,
        tokens: u64,
        iter: usize,
        started_at: i64,
    ) -> String {
        let spec = AgentSpec::on_demand(agent_name, "obj");
        let id = format!("run-{}-{}", agent_name, started_at);
        let mut run = SubagentRun::new_running(&id, None, &spec, "obj", "/tmp", None);
        run.status = status;
        run.tokens_used = tokens;
        run.iterations_used = iter;
        run.started_at = started_at;
        run.finished_at = Some(started_at + 10);
        store.save(&run).unwrap();
        id
    }

    fn fixture_project() -> (TempDir, FileSubagentRunStore) {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path().join(".theo").join("subagent"));
        (dir, store)
    }

    // ── list_agents ──

    #[test]
    fn list_agents_empty_project_returns_empty_vec() {
        let dir = TempDir::new().unwrap();
        assert!(list_agents(dir.path()).is_empty());
    }

    #[test]
    fn list_agents_aggregates_runs_by_agent_name() {
        let (dir, store) = fixture_project();
        save_run(&store, "explorer", RunStatus::Completed, 100, 3, 1);
        save_run(&store, "explorer", RunStatus::Completed, 200, 5, 2);
        save_run(&store, "implementer", RunStatus::Failed, 500, 8, 3);
        let agents = list_agents(dir.path());
        assert_eq!(agents.len(), 2);
        let exp = agents.iter().find(|a| a.agent_name == "explorer").unwrap();
        assert_eq!(exp.run_count, 2);
        assert_eq!(exp.total_tokens_used, 300);
        assert_eq!(exp.success_count, 2);
        let imp = agents.iter().find(|a| a.agent_name == "implementer").unwrap();
        assert_eq!(imp.run_count, 1);
        assert_eq!(imp.failure_count, 1);
    }

    #[test]
    fn list_agents_returns_sorted_by_name() {
        let (dir, store) = fixture_project();
        save_run(&store, "zeta", RunStatus::Completed, 0, 0, 1);
        save_run(&store, "alpha", RunStatus::Completed, 0, 0, 2);
        save_run(&store, "mu", RunStatus::Completed, 0, 0, 3);
        let agents = list_agents(dir.path());
        assert_eq!(agents.len(), 3);
        let names: Vec<&str> = agents.iter().map(|a| a.agent_name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn list_agents_computes_average_metrics() {
        let (dir, store) = fixture_project();
        save_run(&store, "x", RunStatus::Completed, 100, 4, 1);
        save_run(&store, "x", RunStatus::Completed, 300, 6, 2);
        let agents = list_agents(dir.path());
        let x = agents.iter().find(|a| a.agent_name == "x").unwrap();
        assert_eq!(x.total_tokens_used, 400);
        assert_eq!(x.avg_tokens_per_run, 200.0);
        assert_eq!(x.avg_iterations_per_run, 5.0);
    }

    #[test]
    fn list_agents_computes_success_rate_excluding_cancelled() {
        let (dir, store) = fixture_project();
        save_run(&store, "x", RunStatus::Completed, 0, 0, 1);
        save_run(&store, "x", RunStatus::Completed, 0, 0, 2);
        save_run(&store, "x", RunStatus::Failed, 0, 0, 3);
        save_run(&store, "x", RunStatus::Cancelled, 0, 0, 4);
        let agents = list_agents(dir.path());
        let x = agents.iter().find(|a| a.agent_name == "x").unwrap();
        // success_rate = 2 / (2 + 1) = 0.6666…
        assert!((x.success_rate - (2.0 / 3.0)).abs() < 1e-9);
        assert_eq!(x.cancelled_count, 1);
    }

    #[test]
    fn list_agents_tracks_running_count_separately() {
        let (dir, store) = fixture_project();
        save_run(&store, "x", RunStatus::Running, 0, 0, 1);
        save_run(&store, "x", RunStatus::Running, 0, 0, 2);
        save_run(&store, "x", RunStatus::Completed, 0, 0, 3);
        let agents = list_agents(dir.path());
        let x = agents.iter().find(|a| a.agent_name == "x").unwrap();
        assert_eq!(x.running_count, 2);
        assert_eq!(x.success_count, 1);
    }

    #[test]
    fn list_agents_tracks_last_started_at() {
        let (dir, store) = fixture_project();
        save_run(&store, "x", RunStatus::Completed, 0, 0, 100);
        save_run(&store, "x", RunStatus::Completed, 0, 0, 999);
        save_run(&store, "x", RunStatus::Completed, 0, 0, 50);
        let agents = list_agents(dir.path());
        let x = agents.iter().find(|a| a.agent_name == "x").unwrap();
        assert_eq!(x.last_started_at, 999);
    }

    // ── get_agent ──

    #[test]
    fn get_agent_unknown_returns_none() {
        let dir = TempDir::new().unwrap();
        assert!(get_agent(dir.path(), "missing", 5).is_none());
    }

    #[test]
    fn get_agent_returns_detail_with_recent_runs_sorted_newest_first() {
        let (dir, store) = fixture_project();
        save_run(&store, "x", RunStatus::Completed, 100, 3, 1);
        save_run(&store, "x", RunStatus::Completed, 200, 5, 100);
        save_run(&store, "x", RunStatus::Completed, 300, 7, 50);
        let detail = get_agent(dir.path(), "x", 5).unwrap();
        assert_eq!(detail.stats.run_count, 3);
        assert_eq!(detail.recent_runs.len(), 3);
        assert_eq!(detail.recent_runs[0].started_at, 100);
        assert_eq!(detail.recent_runs[1].started_at, 50);
        assert_eq!(detail.recent_runs[2].started_at, 1);
    }

    #[test]
    fn get_agent_respects_limit() {
        let (dir, store) = fixture_project();
        for i in 0..10 {
            save_run(&store, "x", RunStatus::Completed, i * 10, i as usize, i as i64);
        }
        let detail = get_agent(dir.path(), "x", 3).unwrap();
        assert_eq!(detail.stats.run_count, 10);
        assert_eq!(detail.recent_runs.len(), 3, "limit honored on recent_runs");
    }

    #[test]
    fn get_agent_filters_other_agent_names_out() {
        let (dir, store) = fixture_project();
        save_run(&store, "x", RunStatus::Completed, 100, 3, 1);
        save_run(&store, "y", RunStatus::Completed, 999, 99, 2);
        let detail = get_agent(dir.path(), "x", 5).unwrap();
        assert_eq!(detail.stats.run_count, 1);
        assert_eq!(detail.stats.total_tokens_used, 100);
    }

    // ── list_agent_runs ──

    #[test]
    fn list_agent_runs_empty_for_unknown_name() {
        let dir = TempDir::new().unwrap();
        assert!(list_agent_runs(dir.path(), "missing").is_empty());
    }

    #[test]
    fn list_agent_runs_returns_only_matching_agent() {
        let (dir, store) = fixture_project();
        save_run(&store, "x", RunStatus::Completed, 100, 3, 1);
        save_run(&store, "y", RunStatus::Completed, 200, 5, 2);
        save_run(&store, "x", RunStatus::Failed, 50, 2, 3);
        let runs = list_agent_runs(dir.path(), "x");
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|r| !r.run_id.contains("-y-")));
    }

    #[test]
    fn list_agent_runs_sorts_descending_by_started_at() {
        let (dir, store) = fixture_project();
        save_run(&store, "z", RunStatus::Completed, 0, 0, 5);
        save_run(&store, "z", RunStatus::Completed, 0, 0, 1);
        save_run(&store, "z", RunStatus::Completed, 0, 0, 100);
        let runs = list_agent_runs(dir.path(), "z");
        let ts: Vec<i64> = runs.iter().map(|r| r.started_at).collect();
        assert_eq!(ts, vec![100, 5, 1]);
    }

    #[test]
    fn list_agent_runs_includes_status_in_lowercase() {
        let (dir, store) = fixture_project();
        save_run(&store, "z", RunStatus::Failed, 0, 0, 1);
        let runs = list_agent_runs(dir.path(), "z");
        assert_eq!(runs[0].status, "failed");
    }

    // ── general ──

    #[test]
    fn agent_stats_default_has_zero_metrics() {
        let s = AgentStats::default();
        assert_eq!(s.run_count, 0);
        assert_eq!(s.success_rate, 0.0);
        assert_eq!(s.avg_tokens_per_run, 0.0);
    }
}
