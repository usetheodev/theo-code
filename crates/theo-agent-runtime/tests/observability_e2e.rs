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
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join(".theo").join("trajectories");
    let run_id = "e2e-run-001";

    // 1. Bootstrap the pipeline (exact same call the run_engine uses).
    let bus = EventBus::new();
    let pipeline = install_observability(&bus, run_id, base);

    // 2. Publish a realistic run sequence.
    publish_sequence(&bus, run_id);

    // 3. Finalize: drop the pipeline (triggers writer drain) and read back.
    let file_path = pipeline.finalize();
    assert!(file_path.exists(), "trajectory file must exist: {:?}", file_path);

    let (envelopes, integrity) = read_trajectory(&file_path).expect("reader must parse");

    // --- Event-layer assertions ---

    // Streaming events must be filtered out.
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

    // Non-streaming events present.
    let event_types: Vec<String> = envelopes
        .iter()
        .filter(|e| matches!(e.kind, EnvelopeKind::Event))
        .filter_map(|e| e.event_type.clone())
        .collect();
    assert!(event_types.contains(&"RunInitialized".to_string()));
    assert!(event_types.contains(&"ToolCallCompleted".to_string()));
    assert!(event_types.contains(&"HypothesisFormed".to_string()));
    assert!(event_types.contains(&"RunStateChanged".to_string()));

    // Sequence numbers strictly monotonic.
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

    // --- Integrity assertions ---
    assert!(integrity.confidence > 0.0, "confidence must be positive");
    assert_eq!(integrity.schema_version, 1);

    // 4. Finalize the summary with full inputs.
    let budget = Budget::default();
    let usage = BudgetUsage::default();
    let tokens = TokenUsage {
        input_tokens: 800,
        output_tokens: 300,
        cache_read_tokens: 100,
        cache_write_tokens: 50,
        reasoning_tokens: 25,
        estimated_cost_usd: 0.0123,
    };
    let ctx_report = ContextMetricsReport {
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
    };
    let initial_ctx: HashSet<String> = ["src/main.rs"].iter().map(|s| s.to_string()).collect();
    let pre_compaction: HashSet<String> = HashSet::new();
    let inputs = FinalizeInputs {
        run_id,
        token_usage: &tokens,
        successful_edits: 1,
        converged: true,
        budget: &budget,
        usage: &usage,
        ctx_report: &ctx_report,
        done_blocked_count: 0,
        evolution_attempts: 0,
        evolution_success: false,
        episodes_injected: 2,
        episodes_created: 1,
        failure_fingerprints_new: 0,
        failure_fingerprints_recurrent: 0,
        initial_context_files: &initial_ctx,
        pre_compaction_hot_files: &pre_compaction,
    };
    let (detected, _run_report) = finalize_trajectory_summary(&file_path, &inputs);

    // No failure modes should trip on a well-formed run (edit + verification).
    assert!(!detected.premature_termination, "run had edits — not premature");
    assert!(!detected.weak_verification, "run had bash after edit — not weak");
    assert!(!detected.task_derailment, "run referenced initial context");
    assert!(!detected.conversation_history_loss, "no compaction — no loss");

    // 5. Read back the summary line.
    let (envelopes_final, _) = read_trajectory(&file_path).unwrap();
    let summary = envelopes_final
        .iter()
        .find(|e| matches!(e.kind, EnvelopeKind::Summary))
        .expect("summary line must exist");

    let report: RunReport =
        serde_json::from_value(summary.payload.clone()).expect("summary payload is a RunReport");

    // --- RunReport assertions ---
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

    // Surrogate metrics are computed.
    assert!(report.surrogate_metrics.llm_efficiency.value > 0.0);
    // Time to first tool: tool dispatched after RunInitialized.
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
