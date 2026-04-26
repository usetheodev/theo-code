//! `ObservabilityContext` — aggregates the observability + working-set
//! state previously held as flat fields on
//! [`crate::run_engine::AgentRunEngine`].
//!
//! T3.1 PR2 of the AgentRunEngine god-object split. Per
//! `docs/plans/T3.1-god-object-split-roadmap.md`.

use std::collections::HashSet;
use std::sync::Arc;

use crate::context_metrics::ContextMetrics;
use crate::metrics::MetricsCollector;
use crate::observability::ObservabilityPipeline;

/// Observability + working-set state bundle.
pub struct ObservabilityContext {
    pub metrics: Arc<MetricsCollector>,
    pub working_set: theo_domain::working_set::WorkingSet,
    pub context_metrics: ContextMetrics,
    pub pipeline: Option<ObservabilityPipeline>,
    pub episodes_injected: u32,
    pub episodes_created: u32,
    pub initial_context_files: HashSet<String>,
    pub pre_compaction_hot_files: HashSet<String>,
    /// Phase 64 (benchmark-sota-metrics-plan): RunReport captured after
    /// finalize_observability. The caller reads this to embed in AgentResult.
    pub last_run_report: Option<crate::observability::report::RunReport>,
}

impl ObservabilityContext {
    pub fn new(
        metrics: Arc<MetricsCollector>,
        pipeline: Option<ObservabilityPipeline>,
    ) -> Self {
        Self {
            metrics,
            working_set: theo_domain::working_set::WorkingSet::new(),
            context_metrics: ContextMetrics::new(),
            pipeline,
            episodes_injected: 0,
            episodes_created: 0,
            initial_context_files: HashSet::new(),
            pre_compaction_hot_files: HashSet::new(),
            last_run_report: None,
        }
    }
}
