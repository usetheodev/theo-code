//! Phase 15 dashboard integration test — validates that the
//! agents_dashboard use case correctly aggregates SubagentRun records into
//! per-agent statistics and serves them to the CLI dashboard endpoints.

use tempfile::TempDir;

use theo_agent_runtime::subagent::builtins;
use theo_agent_runtime::subagent_runs::{FileSubagentRunStore, RunStatus, SubagentRun};
use theo_application::use_cases::agents_dashboard::{get_agent, list_agents};

fn save(
    store: &FileSubagentRunStore,
    spec_name: &str,
    status: RunStatus,
    tokens: u64,
    iter: usize,
    started_at: i64,
) {
    use theo_domain::agent_spec::AgentSpec;
    let spec = AgentSpec::on_demand(spec_name, "obj");
    let id = format!("{}-{}", spec_name, started_at);
    let mut run = SubagentRun::new_running(&id, None, &spec, "obj", "/tmp", None);
    run.status = status;
    run.tokens_used = tokens;
    run.iterations_used = iter;
    run.started_at = started_at;
    run.finished_at = Some(started_at + 5);
    store.save(&run).unwrap();
}

#[test]
fn list_agents_aggregates_three_distinct_agents() {
    let dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(dir.path().join(".theo").join("subagent"));

    save(&store, "explorer", RunStatus::Completed, 100, 3, 1);
    save(&store, "explorer", RunStatus::Completed, 200, 5, 2);
    save(&store, "implementer", RunStatus::Completed, 800, 12, 3);
    save(&store, "implementer", RunStatus::Failed, 400, 8, 4);
    save(&store, "verifier", RunStatus::Cancelled, 50, 2, 5);

    let agents = list_agents(dir.path());
    assert_eq!(agents.len(), 3);

    let exp = agents.iter().find(|a| a.agent_name == "explorer").unwrap();
    assert_eq!(exp.run_count, 2);
    assert!((exp.success_rate - 1.0).abs() < 1e-9);

    let imp = agents.iter().find(|a| a.agent_name == "implementer").unwrap();
    assert_eq!(imp.run_count, 2);
    assert_eq!(imp.success_count, 1);
    assert_eq!(imp.failure_count, 1);
    assert!((imp.success_rate - 0.5).abs() < 1e-9);

    let ver = agents.iter().find(|a| a.agent_name == "verifier").unwrap();
    assert_eq!(ver.run_count, 1);
    assert_eq!(ver.cancelled_count, 1);
    assert_eq!(ver.success_rate, 0.0); // no completed/failed → 0
}

#[test]
fn get_agent_returns_recent_runs_in_descending_order() {
    let dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(dir.path().join(".theo").join("subagent"));
    for i in 1..=5 {
        save(&store, "x", RunStatus::Completed, 100, 2, i);
    }
    let detail = get_agent(dir.path(), "x", 3).unwrap();
    assert_eq!(detail.recent_runs.len(), 3);
    let ts: Vec<i64> = detail.recent_runs.iter().map(|r| r.started_at).collect();
    assert_eq!(ts, vec![5, 4, 3], "must be descending by started_at");
}

#[test]
fn dashboard_aggregation_uses_real_builtin_specs() {
    // Validates the aggregation works with real `AgentSpec` shape, not just
    // on-demand spec.
    let dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(dir.path().join(".theo").join("subagent"));

    let real_spec = builtins::implementer();
    let mut run = SubagentRun::new_running("r-real", None, &real_spec, "obj", "/tmp", None);
    run.status = RunStatus::Completed;
    run.tokens_used = 999;
    store.save(&run).unwrap();

    let agents = list_agents(dir.path());
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].agent_name, "implementer");
    assert_eq!(agents[0].agent_source, "builtin");
}
