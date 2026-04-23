//! Verifies the dashboard use cases parse a real trajectory produced by
//! a live run against OpenAI (copied into tests/fixtures/ as a golden file).

use std::path::PathBuf;

use theo_application::use_cases::observability_ui;

fn fixture_dir() -> PathBuf {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join(".theo").join("trajectories");
    std::fs::create_dir_all(&base).unwrap();

    // Inline the golden trajectory captured from a real Codex run.
    let jsonl = r#"{"v":1,"seq":0,"ts":1776902226807,"run_id":"real-probe","kind":"event","event_type":"RunInitialized","event_kind":"Lifecycle","entity_id":"real-probe","payload":{"max_iterations":2,"task_id":"task"}}
{"v":1,"seq":1,"ts":1776902226808,"run_id":"real-probe","kind":"event","event_type":"RunStateChanged","event_kind":"Lifecycle","entity_id":"real-probe","payload":{"from":"Initialized","to":"Planning"}}
{"v":1,"seq":2,"ts":1776902226833,"run_id":"real-probe","kind":"event","event_type":"LlmCallStart","event_kind":"Context","entity_id":"real-probe","payload":{"iteration":1,"model":"gpt-5.3-codex"}}
{"v":1,"seq":3,"ts":1776902230108,"run_id":"real-probe","kind":"event","event_type":"RunStateChanged","event_kind":"Lifecycle","entity_id":"real-probe","payload":{"from":"Planning","to":"Converged"}}
{"v":1,"seq":4,"ts":1776902230120,"run_id":"real-probe","kind":"summary","payload":{"integrity":{"complete":true,"confidence":1.0,"drop_sentinels_found":0,"missing_sequences":[],"schema_version":1,"total_events_expected":4,"total_events_received":4,"writer_recoveries_found":0},"loop_metrics":{"convergence_rate":1.0,"done_blocked_count":0,"evolution_attempts":0,"evolution_success":false,"phase_distribution":{},"total_iterations":1,"budget_utilization":{"iterations_pct":0.5,"time_pct":0.0,"tokens_pct":0.004}},"memory_metrics":{"constraints_learned":0,"episodes_created":1,"episodes_injected":3,"failure_fingerprints_new":0,"failure_fingerprints_recurrent":0,"hypotheses_active":0,"hypotheses_formed":0,"hypotheses_invalidated":0},"surrogate_metrics":{"context_waste_ratio":{"caveat":"","confidence":1.0,"denominator":1.0,"is_surrogate":true,"numerator":0.0,"value":0.0},"doom_loop_frequency":{"caveat":"","confidence":0.0,"denominator":0.0,"is_surrogate":true,"numerator":0.0,"value":0.0},"hypothesis_churn_rate":{"caveat":"","confidence":0.0,"denominator":0.0,"is_surrogate":true,"numerator":0.0,"value":0.0},"llm_efficiency":{"caveat":"","confidence":1.0,"denominator":1.0,"is_surrogate":true,"numerator":0.0,"value":0.0},"time_to_first_tool_ms":{"caveat":"","confidence":1.0,"denominator":1.0,"is_surrogate":true,"numerator":3301.0,"value":3301.0}},"token_metrics":{"cache_hit_rate":0.0,"cache_read_tokens":0,"cache_write_tokens":0,"input_tokens":4632,"output_tokens":124,"reasoning_tokens":0,"tokens_per_successful_edit":0.0,"total_cost_usd":0.0},"tool_breakdown":[],"context_health":{"avg_context_size_tokens":0.0,"max_context_size_tokens":0,"context_growth_rate":0.0,"compaction_count":0,"compaction_savings_ratio":0.0,"refetch_rate":0.0,"action_repetition_rate":0.0,"usefulness_avg":0.0}}}
"#;
    std::fs::write(base.join("real-probe.jsonl"), jsonl).unwrap();
    // Leak the TempDir so the path stays valid for the test duration.
    std::mem::forget(tmp);
    base.parent().unwrap().parent().unwrap().to_path_buf()
}

#[test]
fn dashboard_reads_real_codex_trajectory() {
    let project_dir = fixture_dir();
    let runs = observability_ui::list_runs(&project_dir);
    assert_eq!(runs.len(), 1, "should find one run");
    let run = &runs[0];
    assert_eq!(run.run_id, "real-probe");
    assert!(run.success, "run converged");
    // Surrogate metrics reflect the summary line.
    assert_eq!(run.metrics.time_to_first_tool_ms.value, 3301.0);
    assert_eq!(run.metrics.llm_efficiency.confidence, 1.0);
}

#[test]
fn dashboard_trajectory_projection_has_steps() {
    let project_dir = fixture_dir();
    let proj = observability_ui::get_run_trajectory(&project_dir, "real-probe").unwrap();
    assert_eq!(proj.steps.len(), 4); // 4 non-summary events
    assert!(proj.steps.iter().any(|s| s.event_type == "LlmCallStart"));
    assert!(proj.integrity.complete);
}

#[test]
fn dashboard_metrics_extract_from_summary() {
    let project_dir = fixture_dir();
    let metrics = observability_ui::get_run_metrics(&project_dir, "real-probe").unwrap();
    assert_eq!(metrics.time_to_first_tool_ms.value, 3301.0);
    assert_eq!(metrics.time_to_first_tool_ms.numerator, 3301.0);
}
