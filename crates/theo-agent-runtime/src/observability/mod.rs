use std::io::Write;
use std::sync::Mutex;

use theo_domain::event::DomainEvent;

use crate::event_bus::EventListener;

// Submodules of the observability pipeline.
pub mod context_metrics;
pub mod metrics;
pub mod envelope;
pub mod listener;
pub mod writer;
pub mod reader;
pub mod projection;
pub mod derived_metrics;
pub mod normalizer;
pub mod loop_detector;
pub mod failure_sensors;
pub mod otel;
// -42 (otlp-exporter-plan) — gated by feature `otel`. Default
// builds skip these modules entirely (zero compile-time/runtime cost).
#[cfg(feature = "otel")]
pub mod otel_exporter;
#[cfg(feature = "otel")]
pub mod otel_listener;
pub mod report;

pub use envelope::{TrajectoryEnvelope, ENVELOPE_SCHEMA_VERSION};
pub use listener::ObservabilityListener;
pub use writer::{spawn_writer_thread, WriterHandle};
pub use reader::{read_trajectory, IntegrityReport};
pub use projection::{project, ProjectedStep, StepOutcome, TrajectoryProjection};
pub use derived_metrics::{compute_all, DerivedMetrics, SurrogateMetric};
pub use normalizer::{default_normalizer, ToolNormalizer};
pub use loop_detector::{LoopDetector, LoopVerdict};
pub use failure_sensors::{
    detect_conversation_history_loss, detect_premature_termination, detect_task_derailment,
    detect_weak_verification,
};
pub use report::{
    compute_context_health, compute_error_taxonomy, compute_loop_metrics,
    compute_memory_metrics, compute_subagent_metrics, compute_token_metrics,
    compute_tool_breakdown, BudgetUtilization, ContextHealthMetrics, ErrorTaxonomy,
    LoopMetrics, MemoryMetrics, PhaseMetric, RunReport, SubagentMetrics, TokenMetrics,
    ToolBreakdown,
};

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;

use crate::event_bus::EventBus;

/// Convenience pipeline bundling: ObservabilityListener, the background
/// writer, and the trajectory file path. Holds the writer join-handle
/// so the caller can drain and fsync at shutdown.
pub struct ObservabilityPipeline {
    pub run_id: String,
    pub file_path: PathBuf,
    pub writer_handle: Option<writer::WriterHandle>,
    /// Sender is stored so dropping the pipeline drops the sender, which
    /// signals the writer to drain & flush.
    pub sender: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    pub dropped_events: Arc<AtomicU64>,
    pub serialization_errors: Arc<AtomicU64>,
    /// Strong reference to the listener so `finalize()` can explicitly
    /// close the listener's clone of the sender. Without this, the bus
    /// would keep the listener (and the sender clone) alive forever and
    /// the writer thread would never observe hang-up.
    listener: Arc<ObservabilityListener>,
}

impl ObservabilityPipeline {
    /// Build a new pipeline and subscribe the listener to the event bus.
    pub fn install(
        event_bus: &EventBus,
        run_id: &str,
        base_path: PathBuf,
    ) -> Self {
        let (tx, rx) = sync_channel::<Vec<u8>>(listener::DEFAULT_CHANNEL_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let serr = Arc::new(AtomicU64::new(0));
        let listener = Arc::new(ObservabilityListener::new(
            tx.clone(),
            dropped.clone(),
            serr.clone(),
        ));
        event_bus.subscribe(listener.clone());
        let handle = writer::spawn_writer_thread(
            rx,
            run_id.to_string(),
            base_path.clone(),
            dropped.clone(),
            serr.clone(),
        );
        let file_path = writer::trajectory_path(&base_path, run_id);
        Self {
            run_id: run_id.to_string(),
            file_path,
            writer_handle: Some(handle),
            sender: Some(tx),
            dropped_events: dropped,
            serialization_errors: serr,
            listener,
        }
    }

    /// Shut the pipeline down by dropping the sender and joining the writer.
    pub fn finalize(mut self) -> std::path::PathBuf {
        // Close the listener's sender clone — otherwise the bus keeps the
        // listener Arc alive and the writer thread never sees hang-up.
        self.listener.close();
        // Drop our own sender so the writer-thread loop terminates.
        drop(self.sender.take());
        if let Some(h) = self.writer_handle.take() {
            let _ = h.join.join();
        }
        self.file_path.clone()
    }

    /// Append a summary envelope to the trajectory JSONL.
    pub fn append_summary(&self, seq: u64, payload: serde_json::Value) -> std::io::Result<()> {
        writer::append_summary_line(&self.file_path, &self.run_id, seq, payload)
    }
}

/// One-shot installer: subscribes both the `ObservabilityListener` and the
/// `LoopDetectingListener` to the event bus and returns the pipeline handle.
///
/// when the `otel` feature is active AND
/// the global TracerProvider has been installed , also
/// subscribes an `OtelExportingListener` so DomainEvents reach the OTLP
/// collector in addition to the local trajectory JSONL (D5).
pub fn install_observability(
    event_bus: &EventBus,
    run_id: &str,
    base_path: PathBuf,
) -> ObservabilityPipeline {
    let pipeline = ObservabilityPipeline::install(event_bus, run_id, base_path);
    let detector = Arc::new(std::sync::Mutex::new(LoopDetector::new()));
    event_bus.subscribe(Arc::new(loop_detector::LoopDetectingListener::new(detector)));
    #[cfg(feature = "otel")]
    {
        let svc = theo_domain::environment::theo_var("OTLP_SERVICE_NAME")
            .unwrap_or_else(|| "theo".to_string());
        event_bus.subscribe(Arc::new(otel_listener::OtelExportingListener::new(svc)));
    }
    pipeline
}

/// Summary of which failure modes were detected at run exit.
#[derive(Debug, Default, Clone, Copy)]
pub struct DetectedFailureModes {
    pub premature_termination: bool,
    pub weak_verification: bool,
    pub task_derailment: bool,
    pub conversation_history_loss: bool,
}

/// Convenience: build `FinalizeInputs`, run `finalize_trajectory_summary`.
/// Extracted so the run engine doesn't carry 15+ local bindings.
#[allow(clippy::too_many_arguments)]
pub fn finalize_run_observability(
    file_path: &std::path::Path,
    run_id: &str,
    converged: bool,
    successful_edits: u64,
    token_usage: &theo_domain::budget::TokenUsage,
    max_iterations: usize,
    usage: theo_domain::budget::BudgetUsage,
    ctx_report: &crate::observability::context_metrics::ContextMetricsReport,
    done_blocked_count: u32,
    episodes_injected: u32,
    episodes_created: u32,
    fp_new: u32,
    fp_recurrent: u32,
    initial_context_files: &std::collections::HashSet<String>,
    pre_compaction_hot_files: &std::collections::HashSet<String>,
) -> DetectedFailureModes {
    let budget = theo_domain::budget::Budget {
        max_iterations,
        ..theo_domain::budget::Budget::default()
    };
    let inputs = FinalizeInputs {
        run_id,
        token_usage,
        successful_edits,
        converged,
        budget: &budget,
        usage: &usage,
        ctx_report,
        done_blocked_count,
        evolution_attempts: 0,
        evolution_success: false,
        episodes_injected,
        episodes_created,
        failure_fingerprints_new: fp_new,
        failure_fingerprints_recurrent: fp_recurrent,
        initial_context_files,
        pre_compaction_hot_files,
    };
    finalize_trajectory_summary(file_path, &inputs)
}

impl DetectedFailureModes {
    /// Publish one `DomainEvent(Error)` per detected failure mode.
    pub fn publish_events(&self, event_bus: &EventBus, run_id: &str) {
        use theo_domain::event::{DomainEvent, EventType};
        let emit = |mode: &str| {
            event_bus.publish(DomainEvent::new(
                EventType::Error,
                run_id,
                serde_json::json!({"failure_mode": mode}),
            ));
        };
        if self.premature_termination {
            emit("PrematureTermination");
        }
        if self.weak_verification {
            emit("WeakVerification");
        }
        if self.task_derailment {
            emit("TaskDerailment");
        }
        if self.conversation_history_loss {
            emit("ConversationHistoryLoss");
        }
    }
}

/// Inputs the caller must provide so the observability layer can produce an
/// accurate RunReport. Grouping the parameters avoids the `too_many_arguments`
/// lint and makes it obvious which runtime metrics are plumbed.
#[derive(Debug, Clone)]
pub struct FinalizeInputs<'a> {
    pub run_id: &'a str,
    pub token_usage: &'a theo_domain::budget::TokenUsage,
    pub successful_edits: u64,
    pub converged: bool,
    pub budget: &'a theo_domain::budget::Budget,
    pub usage: &'a theo_domain::budget::BudgetUsage,
    pub ctx_report: &'a crate::observability::context_metrics::ContextMetricsReport,
    pub done_blocked_count: u32,
    pub evolution_attempts: u32,
    pub evolution_success: bool,
    pub episodes_injected: u32,
    pub episodes_created: u32,
    pub failure_fingerprints_new: u32,
    pub failure_fingerprints_recurrent: u32,
    pub initial_context_files: &'a std::collections::HashSet<String>,
    pub pre_compaction_hot_files: &'a std::collections::HashSet<String>,
}

/// Finalize a trajectory by:
/// 1. Reading it back to compute the IntegrityReport
/// 2. Projecting envelopes into ProjectedSteps
/// 3. Computing the full RunReport (surrogate, tokens, loop, tools, context, memory)
/// 4. Running all 4 failure sensors (FM-3, FM-4, FM-5, FM-6)
/// 5. Appending the RunReport as a `Summary` line
///
/// Returns which failure modes were detected so the caller can emit events.
pub fn finalize_trajectory_summary(
    file_path: &std::path::Path,
    inputs: &FinalizeInputs<'_>,
) -> DetectedFailureModes {
    let (envelopes, integrity) = match reader::read_trajectory(file_path) {
        Ok(v) => v,
        Err(_) => return DetectedFailureModes::default(),
    };
    let projection = projection::project(inputs.run_id, envelopes, integrity.clone());

    let surrogate = derived_metrics::compute_all(&projection.steps, &integrity);
    let token_metrics =
        report::compute_token_metrics(inputs.token_usage, inputs.successful_edits);
    let loop_metrics = report::compute_loop_metrics(
        &projection.steps,
        inputs.budget,
        inputs.usage,
        inputs.converged,
        inputs.evolution_attempts,
        inputs.evolution_success,
        inputs.done_blocked_count,
    );
    let tool_breakdown = report::compute_tool_breakdown(&projection.steps);
    let usefulness_avg = if inputs.ctx_report.usefulness_scores.is_empty() {
        0.0
    } else {
        inputs
            .ctx_report
            .usefulness_scores
            .values()
            .copied()
            .sum::<f64>()
            / inputs.ctx_report.usefulness_scores.len() as f64
    };
    let context_health = report::compute_context_health(
        &projection.steps,
        inputs.ctx_report.refetch_rate,
        inputs.ctx_report.action_repetition_rate,
        usefulness_avg,
    );
    let memory_metrics = report::compute_memory_metrics(
        &projection.steps,
        inputs.episodes_injected,
        inputs.episodes_created,
        inputs.failure_fingerprints_new,
        inputs.failure_fingerprints_recurrent,
    );
    let subagent_metrics = report::compute_subagent_metrics(&projection.steps);
    let error_taxonomy = report::compute_error_taxonomy(&projection.steps);

    let detected = DetectedFailureModes {
        premature_termination: failure_sensors::detect_premature_termination(&projection.steps),
        weak_verification: failure_sensors::detect_weak_verification(&projection.steps),
        task_derailment: failure_sensors::detect_task_derailment(
            &projection.steps,
            inputs.initial_context_files,
        ),
        conversation_history_loss: failure_sensors::detect_conversation_history_loss(
            &projection.steps,
            inputs.pre_compaction_hot_files,
        ),
    };

    let report_payload = report::RunReport {
        surrogate_metrics: surrogate,
        token_metrics,
        loop_metrics,
        tool_breakdown,
        context_health,
        memory_metrics,
        subagent_metrics,
        error_taxonomy,
        integrity,
    };

    let summary_seq = projection
        .steps
        .last()
        .map(|s| s.sequence + 1)
        .unwrap_or(0);
    let _ = writer::append_summary_line(
        file_path,
        inputs.run_id,
        summary_seq,
        serde_json::to_value(&report_payload).unwrap_or_default(),
    );
    detected
}

/// Event listener that writes structured JSON lines to a writer.
///
/// Each DomainEvent is serialized as a single JSON line for log aggregation.
#[deprecated(note = "Use ObservabilityListener + writer thread for new call sites")]
pub struct StructuredLogListener {
    writer: Mutex<Box<dyn Write + Send>>,
}

#[allow(deprecated)]
impl StructuredLogListener {
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer: Mutex::new(writer),
        }
    }

    /// Creates a listener that writes to stdout.
    pub fn stdout() -> Self {
        Self::new(Box::new(std::io::stdout()))
    }

    /// Creates a listener that writes to a file.
    pub fn file(path: &std::path::Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self::new(Box::new(file)))
    }
}

#[allow(deprecated)]
impl EventListener for StructuredLogListener {
    fn on_event(&self, event: &DomainEvent) {
        if let Ok(json) = serde_json::to_string(event)
            && let Ok(mut writer) = self.writer.lock() {
                let _ = writeln!(writer, "{}", json);
            }
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use theo_domain::event::{ALL_EVENT_TYPES, EventType};

    fn make_event(event_type: EventType) -> DomainEvent {
        DomainEvent::new(event_type, "test-entity", serde_json::Value::Null)
    }

    #[test]
    fn writes_valid_json_line() {
        let buffer = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let b = buffer.clone();
            struct VecWriter(Arc<Mutex<Vec<u8>>>);
            impl Write for VecWriter {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    self.0.lock().unwrap().extend_from_slice(buf);
                    Ok(buf.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            VecWriter(b)
        };

        let listener = StructuredLogListener::new(Box::new(writer));
        listener.on_event(&make_event(EventType::TaskCreated));

        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines.len(), 1);

        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["event_type"], "TaskCreated");
        assert_eq!(parsed["entity_id"], "test-entity");
    }

    #[test]
    fn handles_all_event_types_without_panic() {
        let listener = StructuredLogListener::new(Box::new(std::io::sink()));
        for et in &ALL_EVENT_TYPES {
            listener.on_event(&make_event(*et)); // must not panic
        }
    }

    #[test]
    fn multiple_events_write_multiple_lines() {
        let buffer = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let b = buffer.clone();
            struct VecWriter(Arc<Mutex<Vec<u8>>>);
            impl Write for VecWriter {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    self.0.lock().unwrap().extend_from_slice(buf);
                    Ok(buf.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            VecWriter(b)
        };

        let listener = StructuredLogListener::new(Box::new(writer));
        listener.on_event(&make_event(EventType::TaskCreated));
        listener.on_event(&make_event(EventType::RunStateChanged));
        listener.on_event(&make_event(EventType::Error));

        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines.len(), 3);

        // Each line is valid JSON
        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
    }
}
