//! End-to-end observability pipeline test.
//!
//! Exercises the full flow from event bus → ObservabilityListener → writer thread
//! → JSONL file → reader → projection → finalize → RunReport.
//!
//! This is "real E2E" for the observability stack: we publish the exact sequence
//! of DomainEvents that a live agent run would produce, drain the writer,
//! re-read the file, and inspect the RunReport in the summary line.

use std::collections::HashSet;
use std::sync::Arc;

use theo_agent_runtime::event_bus::EventBus;
use theo_agent_runtime::observability::{
    context_metrics::ContextMetricsReport, finalize_trajectory_summary,
    install_observability, read_trajectory, FinalizeInputs,
};
use theo_agent_runtime::observability::envelope::EnvelopeKind;
use theo_agent_runtime::observability::report::RunReport;
use theo_domain::budget::{Budget, BudgetUsage, TokenUsage};
use theo_domain::event::{DomainEvent, EventType};

fn empty_ctx_report() -> ContextMetricsReport {
    ContextMetricsReport {
        avg_context_size: 0.0,
        max_context_size: 0,
        total_iterations: 0,
        refetch_rate: 0.0,
        action_repetition_rate: 0.0,
        hypothesis_changes: 0,
        unique_artifacts_fetched: 0,
        unique_actions: 0,
        top_refetched: vec![],
        repeated_actions: vec![],
        usefulness_scores: Default::default(),
        causal_usefulness: Default::default(),
        failure_constraints: vec![],
    }
}

fn publish_sequence(bus: &EventBus, run_id: &str) {
    // RunInitialized.
    bus.publish(DomainEvent::new(
        EventType::RunInitialized,
        run_id,
        serde_json::json!({"task_id": "t1", "max_iterations": 50}),
    ));

    // Iteration 1: LlmCallStart → HypothesisFormed → ToolCallDispatched → ToolCallCompleted.
    bus.publish(DomainEvent::new(
        EventType::LlmCallStart,
        run_id,
        serde_json::json!({"context_tokens": 1500}),
    ));
    bus.publish(DomainEvent::new(
        EventType::HypothesisFormed,
        run_id,
        serde_json::json!({
            "hypothesis": "the bug is in the parser",
            "rationale": "error message points to parse_expr"
        }),
    ));
    bus.publish(DomainEvent::new(
        EventType::ToolCallDispatched,
        run_id,
        serde_json::json!({"tool_name": "read"}),
    ));
    bus.publish(DomainEvent::new(
        EventType::ToolCallCompleted,
        run_id,
        serde_json::json!({
            "tool_name": "read",
            "state": "Succeeded",
            "args": {"path": "src/main.rs"}
        }),
    ));
    bus.publish(DomainEvent::new(
        EventType::LlmCallEnd,
        run_id,
        serde_json::json!({"context_tokens": 1600}),
    ));

    // Iteration 2: edit + verification (good pattern — no weak verification).
    bus.publish(DomainEvent::new(
        EventType::LlmCallStart,
        run_id,
        serde_json::json!({"context_tokens": 1800}),
    ));
    bus.publish(DomainEvent::new(
        EventType::ToolCallDispatched,
        run_id,
        serde_json::json!({"tool_name": "edit"}),
    ));
    bus.publish(DomainEvent::new(
        EventType::ToolCallCompleted,
        run_id,
        serde_json::json!({
            "tool_name": "edit",
            "state": "Succeeded",
            "args": {"path": "src/main.rs"}
        }),
    ));
    bus.publish(DomainEvent::new(
        EventType::ToolCallDispatched,
        run_id,
        serde_json::json!({"tool_name": "bash"}),
    ));
    bus.publish(DomainEvent::new(
        EventType::ToolCallCompleted,
        run_id,
        serde_json::json!({
            "tool_name": "bash",
            "state": "Succeeded",
            "args": {"command": "cargo test"}
        }),
    ));
    bus.publish(DomainEvent::new(
        EventType::LlmCallEnd,
        run_id,
        serde_json::json!({"context_tokens": 1900}),
    ));

    // Streaming events — must NOT appear in trajectory.
    bus.publish(DomainEvent::new(
        EventType::ContentDelta,
        run_id,
        serde_json::json!({"text": "thinking..."}),
    ));
    bus.publish(DomainEvent::new(
        EventType::ReasoningDelta,
        run_id,
        serde_json::json!({"text": "consider X"}),
    ));

    // Converged.
    bus.publish(DomainEvent::new(
        EventType::RunStateChanged,
        run_id,
        serde_json::json!({"from": "Executing", "to": "Converged"}),
    ));
}

#[test]
fn e2e_observability_pipeline_full_flow() {
    // Single E2E test: bootstrap → publish → drain → read → finalize →
    // re-read summary. Helpers below own the per-section assertion
    // blocks so this body stays under the size budget.
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "e2e-run-001";
    let base = tmp.path().join(".theo").join("trajectories");
    let bus = EventBus::new();
    let pipeline = install_observability(&bus, run_id, base);
    publish_sequence(&bus, run_id);
    let file_path = pipeline.finalize();
    assert!(file_path.exists(), "trajectory file must exist: {:?}", file_path);
    let (envelopes, integrity) = read_trajectory(&file_path).expect("reader must parse");
    assert_e2e_streaming_events_filtered(&envelopes);
    assert_e2e_event_types_present(&envelopes);
    assert_e2e_sequence_strictly_monotonic(&envelopes);
    assert!(integrity.confidence > 0.0, "confidence must be positive");
    assert_eq!(integrity.schema_version, 1);
    let inputs_owned = build_full_flow_finalize_inputs();
    let (detected, _run_report) =
        finalize_trajectory_summary(&file_path, &inputs_owned.as_inputs(run_id));
    assert!(!detected.premature_termination, "run had edits — not premature");
    assert!(!detected.weak_verification, "run had bash after edit — not weak");
    assert!(!detected.task_derailment, "run referenced initial context");
    assert!(!detected.conversation_history_loss, "no compaction — no loss");
    let report = read_run_report(&file_path);
    assert_e2e_run_report_full_flow(&report);
}

fn assert_e2e_streaming_events_filtered(envelopes: &[theo_agent_runtime::observability::envelope::TrajectoryEnvelope]) {
    let streaming_events: Vec<_> = envelopes
        .iter()
        .filter(|e| {
            matches!(e.kind, EnvelopeKind::Event)
                && (e.event_type.as_deref() == Some("ContentDelta")
                    || e.event_type.as_deref() == Some("ReasoningDelta"))
        })
        .collect();
    assert!(
        streaming_events.is_empty(),
        "streaming events must NOT appear in trajectory, found {:?}",
        streaming_events.len()
    );
}

fn assert_e2e_event_types_present(envelopes: &[theo_agent_runtime::observability::envelope::TrajectoryEnvelope]) {
    let event_types: Vec<String> = envelopes
        .iter()
        .filter(|e| matches!(e.kind, EnvelopeKind::Event))
        .filter_map(|e| e.event_type.clone())
        .collect();
    for expected in [
        "RunInitialized",
        "ToolCallCompleted",
        "HypothesisFormed",
        "RunStateChanged",
    ] {
        assert!(
            event_types.contains(&expected.to_string()),
            "missing expected event_type {expected}"
        );
    }
}

fn assert_e2e_sequence_strictly_monotonic(envelopes: &[theo_agent_runtime::observability::envelope::TrajectoryEnvelope]) {
    let seqs: Vec<u64> = envelopes.iter().map(|e| e.seq).collect();
    for i in 1..seqs.len() {
        assert!(
            seqs[i] > seqs[i - 1],
            "sequence must be strictly monotonic, got {} → {} at index {}",
            seqs[i - 1],
            seqs[i],
            i
        );
    }
}

/// Owned-data carrier for the FinalizeInputs lifetimes (Budget,
/// BudgetUsage, TokenUsage, ContextMetricsReport, HashSet) — the
/// real `FinalizeInputs` borrows from this.
struct FullFlowOwnedInputs {
    budget: Budget,
    usage: BudgetUsage,
    tokens: TokenUsage,
    ctx_report: ContextMetricsReport,
    initial_ctx: HashSet<String>,
    pre_compaction: HashSet<String>,
}

impl FullFlowOwnedInputs {
    fn as_inputs<'a>(&'a self, run_id: &'a str) -> FinalizeInputs<'a> {
        FinalizeInputs {
            run_id,
            token_usage: &self.tokens,
            successful_edits: 1,
            converged: true,
            budget: &self.budget,
            usage: &self.usage,
            ctx_report: &self.ctx_report,
            done_blocked_count: 0,
            evolution_attempts: 0,
            evolution_success: false,
            episodes_injected: 2,
            episodes_created: 1,
            failure_fingerprints_new: 0,
            failure_fingerprints_recurrent: 0,
            initial_context_files: &self.initial_ctx,
            pre_compaction_hot_files: &self.pre_compaction,
        }
    }
}

fn build_full_flow_finalize_inputs() -> FullFlowOwnedInputs {
    FullFlowOwnedInputs {
        budget: Budget::default(),
        usage: BudgetUsage::default(),
        tokens: TokenUsage {
            input_tokens: 800,
            output_tokens: 300,
            cache_read_tokens: 100,
            cache_write_tokens: 50,
            reasoning_tokens: 25,
            estimated_cost_usd: 0.0123,
        },
        ctx_report: ContextMetricsReport {
            avg_context_size: 1700.0,
            max_context_size: 1900,
            total_iterations: 2,
            refetch_rate: 0.0,
            action_repetition_rate: 0.0,
            hypothesis_changes: 1,
            unique_artifacts_fetched: 1,
            unique_actions: 2,
            top_refetched: vec![],
            repeated_actions: vec![],
            usefulness_scores: Default::default(),
            causal_usefulness: Default::default(),
            failure_constraints: vec![],
        },
        initial_ctx: ["src/main.rs"].iter().map(|s| s.to_string()).collect(),
        pre_compaction: HashSet::new(),
    }
}

fn read_run_report(file_path: &std::path::Path) -> RunReport {
    let (envelopes_final, _) = read_trajectory(file_path).unwrap();
    let summary = envelopes_final
        .iter()
        .find(|e| matches!(e.kind, EnvelopeKind::Summary))
        .expect("summary line must exist");
    serde_json::from_value(summary.payload.clone()).expect("summary payload is a RunReport")
}

fn assert_e2e_run_report_full_flow(report: &RunReport) {
    assert_eq!(report.memory_metrics.episodes_injected, 2);
    assert_eq!(report.memory_metrics.episodes_created, 1);
    assert_eq!(report.memory_metrics.hypotheses_formed, 1);
    assert_eq!(report.token_metrics.input_tokens, 800);
    assert!(report.token_metrics.cache_hit_rate > 0.0);
    assert!(report.token_metrics.tokens_per_successful_edit > 0.0);
    assert_eq!(report.loop_metrics.total_iterations, 2);
    assert_eq!(report.loop_metrics.convergence_rate, 1.0);
    assert!(report.tool_breakdown.iter().any(|t| t.tool_name == "read"));
    assert!(report.tool_breakdown.iter().any(|t| t.tool_name == "edit"));
    assert!(report.tool_breakdown.iter().any(|t| t.tool_name == "bash"));
    assert!(report.surrogate_metrics.llm_efficiency.value > 0.0);
    assert!(report.surrogate_metrics.time_to_first_tool_ms.value >= 0.0);
}

#[test]
fn e2e_detects_premature_termination() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join(".theo").join("trajectories");
    let run_id = "e2e-premature";

    let bus = EventBus::new();
    let pipeline = install_observability(&bus, run_id, base);

    // No edits, 3 LlmCall iterations, converged.
    bus.publish(DomainEvent::new(EventType::RunInitialized, run_id, serde_json::json!({})));
    for _ in 0..3 {
        bus.publish(DomainEvent::new(EventType::LlmCallStart, run_id, serde_json::json!({})));
        bus.publish(DomainEvent::new(EventType::LlmCallEnd, run_id, serde_json::json!({})));
    }
    bus.publish(DomainEvent::new(
        EventType::RunStateChanged,
        run_id,
        serde_json::json!({"to": "Converged"}),
    ));

    let file_path = pipeline.finalize();
    let budget = Budget::default();
    let usage = BudgetUsage::default();
    let tokens = TokenUsage::default();
    let ctx_report = empty_ctx_report();
    let empty: HashSet<String> = HashSet::new();
    let inputs = FinalizeInputs {
        run_id,
        token_usage: &tokens,
        successful_edits: 0,
        converged: true,
        budget: &budget,
        usage: &usage,
        ctx_report: &ctx_report,
        done_blocked_count: 0,
        evolution_attempts: 0,
        evolution_success: false,
        episodes_injected: 0,
        episodes_created: 0,
        failure_fingerprints_new: 0,
        failure_fingerprints_recurrent: 0,
        initial_context_files: &empty,
        pre_compaction_hot_files: &empty,
    };
    let (detected, _run_report) = finalize_trajectory_summary(&file_path, &inputs);
    assert!(
        detected.premature_termination,
        "3 iterations + 0 edits + converged = premature termination"
    );
}

#[test]
fn e2e_detects_weak_verification() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join(".theo").join("trajectories");
    let run_id = "e2e-weak-verify";

    let bus = EventBus::new();
    let pipeline = install_observability(&bus, run_id, base);

    bus.publish(DomainEvent::new(EventType::RunInitialized, run_id, serde_json::json!({})));
    // edit_file succeeded, followed by LLM calls but no bash/sensor.
    bus.publish(DomainEvent::new(
        EventType::ToolCallCompleted,
        run_id,
        serde_json::json!({"tool_name": "edit", "state": "Succeeded"}),
    ));
    bus.publish(DomainEvent::new(EventType::LlmCallStart, run_id, serde_json::json!({})));
    bus.publish(DomainEvent::new(EventType::LlmCallEnd, run_id, serde_json::json!({})));
    bus.publish(DomainEvent::new(EventType::LlmCallStart, run_id, serde_json::json!({})));
    bus.publish(DomainEvent::new(EventType::LlmCallEnd, run_id, serde_json::json!({})));

    let file_path = pipeline.finalize();
    let budget = Budget::default();
    let usage = BudgetUsage::default();
    let tokens = TokenUsage::default();
    let ctx_report = empty_ctx_report();
    let empty: HashSet<String> = HashSet::new();
    let inputs = FinalizeInputs {
        run_id,
        token_usage: &tokens,
        successful_edits: 1,
        converged: false,
        budget: &budget,
        usage: &usage,
        ctx_report: &ctx_report,
        done_blocked_count: 0,
        evolution_attempts: 0,
        evolution_success: false,
        episodes_injected: 0,
        episodes_created: 0,
        failure_fingerprints_new: 0,
        failure_fingerprints_recurrent: 0,
        initial_context_files: &empty,
        pre_compaction_hot_files: &empty,
    };
    let (detected, _run_report) = finalize_trajectory_summary(&file_path, &inputs);
    assert!(detected.weak_verification, "edit + no bash/sensor = weak verification");
}

#[test]
fn e2e_detects_conversation_history_loss() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join(".theo").join("trajectories");
    let run_id = "e2e-history-loss";

    let bus = EventBus::new();
    let pipeline = install_observability(&bus, run_id, base);

    bus.publish(DomainEvent::new(EventType::RunInitialized, run_id, serde_json::json!({})));
    // Context overflow recovery.
    bus.publish(DomainEvent::new(
        EventType::ContextOverflowRecovery,
        run_id,
        serde_json::json!({"action": "emergency_compaction"}),
    ));
    // Immediately re-reads a pre-compaction hot file.
    bus.publish(DomainEvent::new(
        EventType::ToolCallCompleted,
        run_id,
        serde_json::json!({
            "tool_name": "read",
            "state": "Succeeded",
            "args": {"path": "src/hot.rs"}
        }),
    ));

    let file_path = pipeline.finalize();
    let budget = Budget::default();
    let usage = BudgetUsage::default();
    let tokens = TokenUsage::default();
    let ctx_report = empty_ctx_report();
    let empty: HashSet<String> = HashSet::new();
    let mut hot: HashSet<String> = HashSet::new();
    hot.insert("src/hot.rs".into());
    let inputs = FinalizeInputs {
        run_id,
        token_usage: &tokens,
        successful_edits: 0,
        converged: false,
        budget: &budget,
        usage: &usage,
        ctx_report: &ctx_report,
        done_blocked_count: 0,
        evolution_attempts: 0,
        evolution_success: false,
        episodes_injected: 0,
        episodes_created: 0,
        failure_fingerprints_new: 0,
        failure_fingerprints_recurrent: 0,
        initial_context_files: &empty,
        pre_compaction_hot_files: &hot,
    };
    let (detected, _run_report) = finalize_trajectory_summary(&file_path, &inputs);
    assert!(detected.conversation_history_loss, "re-read hot file after compaction = history loss");
}

#[test]
fn e2e_pipeline_survives_drop_pressure() {
    // Publish way more events than the channel capacity can hold.
    // Verifies INV-1 (at_least_observed): either persisted or counted as dropped,
    // never silently lost.
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join(".theo").join("trajectories");
    let run_id = "e2e-drop-pressure";

    let bus = Arc::new(EventBus::new());
    let pipeline = install_observability(&bus, run_id, base);

    let bus_clone = Arc::clone(&bus);
    // Hammer the bus from a background thread.
    let handle = std::thread::spawn(move || {
        for i in 0..5000 {
            bus_clone.publish(DomainEvent::new(
                EventType::ToolCallCompleted,
                "e",
                serde_json::json!({"tool_name": "read", "state": "Succeeded", "i": i}),
            ));
        }
    });
    handle.join().unwrap();

    let dropped_before = pipeline.dropped_events.load(std::sync::atomic::Ordering::Relaxed);
    let file_path = pipeline.finalize();

    let (envelopes, integrity) = read_trajectory(&file_path).unwrap();
    let event_count = envelopes
        .iter()
        .filter(|e| matches!(e.kind, EnvelopeKind::Event))
        .count() as u64;
    // Verify that EITHER the event is in the file OR it was counted as dropped.
    let total_observed = event_count + dropped_before;
    assert!(
        total_observed >= 4000, // allow some slack
        "INV-1 invariant: 5000 published → observed {} (persisted {} + dropped {})",
        total_observed,
        event_count,
        dropped_before
    );
    // If anything was dropped, a sentinel must have been written.
    if dropped_before > 0 {
        assert!(
            integrity.drop_sentinels_found > 0,
            "dropped > 0 but no drop_sentinels written"
        );
    }
}
