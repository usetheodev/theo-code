use std::sync::Arc;

use parking_lot::RwLock;

/// Aggregated runtime metrics for observability.
#[derive(Debug, Clone, Default)]
pub struct RuntimeMetrics {
    pub total_runs: u64,
    pub total_tasks: u64,
    pub total_tool_calls: u64,
    pub successful_tool_calls: u64,
    pub total_llm_calls: u64,
    pub total_tokens_used: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_retries: u64,
    pub total_dlq_entries: u64,
    pub converged_runs: u64,

    /// Accumulated dollar cost across all LLM calls in this session.
    pub total_cost_usd: f64,

    // Timing accumulators (for computing averages)
    total_iteration_ms: u64,
    iteration_count: u64,
    total_tool_call_ms: u64,
    tool_call_count: u64,
    total_llm_call_ms: u64,
    llm_call_count: u64,
}

impl RuntimeMetrics {
    pub fn avg_iteration_ms(&self) -> f64 {
        safe_div(self.total_iteration_ms as f64, self.iteration_count as f64)
    }

    pub fn avg_tool_call_ms(&self) -> f64 {
        safe_div(self.total_tool_call_ms as f64, self.tool_call_count as f64)
    }

    pub fn avg_llm_call_ms(&self) -> f64 {
        safe_div(self.total_llm_call_ms as f64, self.llm_call_count as f64)
    }

    pub fn tool_success_rate(&self) -> f64 {
        safe_div(
            self.successful_tool_calls as f64,
            self.total_tool_calls as f64,
        )
    }

    pub fn convergence_rate(&self) -> f64 {
        safe_div(self.converged_runs as f64, self.total_runs as f64)
    }
}

/// Phase 27 (sota-gaps-followup): single routing decision recorded for
/// post-mortem analysis. Aggregated in `RuntimeMetrics::routing_decisions`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RoutingDecisionMetric {
    /// Detected task type (Retrieval/Implementation/Analysis/Planning/Generic).
    pub task_type: String,
    /// Tier chosen by `ComplexityClassifier::classify`.
    pub tier: String,
    /// Concrete model id selected from the slot config.
    pub model_id: String,
}

/// Aggregated routing histogram. Map key = (task_type, tier, model_id),
/// value = count.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RoutingHistogram {
    pub buckets: std::collections::BTreeMap<String, u64>,
}

impl RoutingHistogram {
    pub fn record(&mut self, decision: &RoutingDecisionMetric) {
        let key = format!(
            "{}|{}|{}",
            decision.task_type, decision.tier, decision.model_id
        );
        *self.buckets.entry(key).or_insert(0) += 1;
    }

    pub fn total(&self) -> u64 {
        self.buckets.values().sum()
    }

    pub fn count_for(&self, task_type: &str, tier: &str, model_id: &str) -> u64 {
        let key = format!("{}|{}|{}", task_type, tier, model_id);
        *self.buckets.get(&key).unwrap_or(&0)
    }
}

/// Thread-safe metrics collector.
///
/// Uses RwLock to allow concurrent reads (snapshot) and exclusive writes (record).
pub struct MetricsCollector {
    metrics: Arc<RwLock<RuntimeMetrics>>,
    /// Phase 12: per-agent metrics breakdown for the dashboard (A4 gap).
    by_agent: Arc<RwLock<crate::observability::otel::MetricsByAgent>>,
    /// Phase 27 (sota-gaps-followup): routing decisions histogram.
    routing: Arc<RwLock<RoutingHistogram>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(RuntimeMetrics::default())),
            by_agent: Arc::new(RwLock::new(
                crate::observability::otel::MetricsByAgent::new(),
            )),
            routing: Arc::new(RwLock::new(RoutingHistogram::default())),
        }
    }

    /// Phase 27: record a single routing decision. Aggregated into the
    /// `RoutingHistogram` for post-mortem analysis.
    pub fn record_routing_decision(
        &self,
        task_type: &str,
        tier: &str,
        model_id: &str,
    ) {
        self.routing.write().record(&RoutingDecisionMetric {
            task_type: task_type.to_string(),
            tier: tier.to_string(),
            model_id: model_id.to_string(),
        });
    }

    /// Phase 27: snapshot of the routing histogram (cloned for safe reading).
    pub fn routing_snapshot(&self) -> RoutingHistogram {
        self.routing.read().clone()
    }

    /// Phase 12: record per-agent run completion (called from spawn_with_spec
    /// after final result is known).
    pub fn record_subagent_run(
        &self,
        agent_name: &str,
        success: bool,
        payload: crate::observability::otel::SubagentRunMetrics,
    ) {
        let mut m = self.by_agent.write();
        m.record(agent_name, success, payload);
    }

    /// Snapshot per-agent metrics breakdown.
    pub fn by_agent_snapshot(&self) -> crate::observability::otel::MetricsByAgent {
        self.by_agent
            .read()
                        .clone()
    }

    pub fn record_llm_call(&self, duration_ms: u64, tokens: u64) {
        let mut m = self.metrics.write();
        m.total_llm_calls += 1;
        m.total_tokens_used += tokens;
        m.total_llm_call_ms += duration_ms;
        m.llm_call_count += 1;
    }

    /// Record an LLM call with input/output token breakdown.
    pub fn record_llm_call_detailed(
        &self,
        duration_ms: u64,
        input_tokens: u64,
        output_tokens: u64,
    ) {
        let total = input_tokens + output_tokens;
        let mut m = self.metrics.write();
        m.total_llm_calls += 1;
        m.total_tokens_used += total;
        m.total_input_tokens += input_tokens;
        m.total_output_tokens += output_tokens;
        m.total_llm_call_ms += duration_ms;
        m.llm_call_count += 1;
    }

    /// Record tokens consumed by a delegated sub-agent or skill.
    /// Acumulates tokens WITHOUT incrementing total_llm_calls
    /// (sub-agent calls are not LLM calls of the parent).
    pub fn record_delegated_tokens(&self, tokens: u64) {
        let mut m = self.metrics.write();
        m.total_tokens_used += tokens;
    }

    pub fn record_tool_call(&self, _tool_name: &str, duration_ms: u64, success: bool) {
        let mut m = self.metrics.write();
        m.total_tool_calls += 1;
        if success {
            m.successful_tool_calls += 1;
        }
        m.total_tool_call_ms += duration_ms;
        m.tool_call_count += 1;
    }

    pub fn record_retry(&self) {
        let mut m = self.metrics.write();
        m.total_retries += 1;
    }

    pub fn record_dlq_entry(&self) {
        let mut m = self.metrics.write();
        m.total_dlq_entries += 1;
    }

    pub fn record_run_complete(&self, converged: bool) {
        let mut m = self.metrics.write();
        m.total_runs += 1;
        if converged {
            m.converged_runs += 1;
        }
    }

    /// Accumulate dollar cost from an LLM call.
    pub fn record_cost(&self, cost_usd: f64) {
        let mut m = self.metrics.write();
        m.total_cost_usd += cost_usd;
    }

    pub fn record_iteration(&self, duration_ms: u64) {
        let mut m = self.metrics.write();
        m.total_iteration_ms += duration_ms;
        m.iteration_count += 1;
    }

    /// Returns a snapshot of current metrics (clone, does not consume).
    pub fn snapshot(&self) -> RuntimeMetrics {
        self.metrics
            .read()
                        .clone()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Safe division: returns 0.0 instead of NaN when denominator is 0.
fn safe_div(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_collector_has_zero_metrics() {
        let collector = MetricsCollector::new();
        let m = collector.snapshot();
        assert_eq!(m.total_runs, 0);
        assert_eq!(m.total_llm_calls, 0);
        assert_eq!(m.total_tool_calls, 0);
        assert_eq!(m.total_tokens_used, 0);
    }

    #[test]
    fn record_llm_call_accumulates() {
        let collector = MetricsCollector::new();
        collector.record_llm_call(100, 500);
        collector.record_llm_call(200, 300);
        let m = collector.snapshot();
        assert_eq!(m.total_llm_calls, 2);
        assert_eq!(m.total_tokens_used, 800);
        assert_eq!(m.avg_llm_call_ms(), 150.0);
    }

    #[test]
    fn record_tool_call_tracks_success() {
        let collector = MetricsCollector::new();
        collector.record_tool_call("read", 50, true);
        collector.record_tool_call("edit", 100, false);
        collector.record_tool_call("bash", 75, true);
        let m = collector.snapshot();
        assert_eq!(m.total_tool_calls, 3);
        assert_eq!(m.successful_tool_calls, 2);
        assert!((m.tool_success_rate() - 2.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn record_retry_increments() {
        let collector = MetricsCollector::new();
        collector.record_retry();
        collector.record_retry();
        assert_eq!(collector.snapshot().total_retries, 2);
    }

    #[test]
    fn record_run_complete_updates_convergence() {
        let collector = MetricsCollector::new();
        collector.record_run_complete(true);
        collector.record_run_complete(false);
        collector.record_run_complete(true);
        let m = collector.snapshot();
        assert_eq!(m.total_runs, 3);
        assert_eq!(m.converged_runs, 2);
        assert!((m.convergence_rate() - 2.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn success_rate_zero_div_returns_zero() {
        let m = RuntimeMetrics::default();
        assert_eq!(m.tool_success_rate(), 0.0);
        assert_eq!(m.convergence_rate(), 0.0);
        assert_eq!(m.avg_llm_call_ms(), 0.0);
        assert_eq!(m.avg_tool_call_ms(), 0.0);
        assert_eq!(m.avg_iteration_ms(), 0.0);
        // Verify it's 0.0, not NaN
        assert!(!m.tool_success_rate().is_nan());
    }

    #[test]
    fn snapshot_does_not_consume_state() {
        let collector = MetricsCollector::new();
        collector.record_llm_call(100, 500);
        let s1 = collector.snapshot();
        let s2 = collector.snapshot();
        assert_eq!(s1.total_llm_calls, s2.total_llm_calls);
    }

    #[test]
    fn concurrent_recording_is_safe() {
        let collector = Arc::new(MetricsCollector::new());
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let c = collector.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        c.record_llm_call(10, 50);
                        c.record_tool_call("test", 5, true);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let m = collector.snapshot();
        assert_eq!(m.total_llm_calls, 1000);
        assert_eq!(m.total_tool_calls, 1000);
    }

    #[test]
    fn record_cost_accumulates_usd() {
        // Arrange
        let collector = MetricsCollector::new();

        // Act
        collector.record_cost(0.003);
        collector.record_cost(0.015);

        // Assert
        let m = collector.snapshot();
        assert!((m.total_cost_usd - 0.018).abs() < 1e-9);
    }

    #[test]
    fn new_collector_has_zero_cost() {
        let collector = MetricsCollector::new();
        let m = collector.snapshot();
        assert_eq!(m.total_cost_usd, 0.0);
    }

    #[test]
    fn record_subagent_run_aggregates_per_agent_metrics() {
        use crate::observability::otel::SubagentRunMetrics;
        let collector = MetricsCollector::new();
        collector.record_subagent_run(
            "explorer",
            true,
            SubagentRunMetrics {
                tokens_used: 1000,
                input_tokens: 700,
                output_tokens: 300,
                llm_calls: 2,
                iterations_used: 5,
                duration_ms: 200,
            },
        );
        collector.record_subagent_run(
            "explorer",
            false,
            SubagentRunMetrics {
                tokens_used: 500,
                ..Default::default()
            },
        );
        collector.record_subagent_run(
            "implementer",
            true,
            SubagentRunMetrics {
                tokens_used: 3000,
                ..Default::default()
            },
        );

        let by_agent = collector.by_agent_snapshot();
        let exp = by_agent.get("explorer").expect("explorer recorded");
        assert_eq!(exp.runs, 2);
        assert_eq!(exp.success, 1);
        assert_eq!(exp.failure, 1);
        assert_eq!(exp.total_tokens, 1500);
        assert_eq!(exp.success_rate(), 0.5);

        let imp = by_agent.get("implementer").expect("implementer recorded");
        assert_eq!(imp.runs, 1);
        assert_eq!(imp.total_tokens, 3000);
    }

    #[test]
    fn by_agent_snapshot_empty_for_new_collector() {
        let collector = MetricsCollector::new();
        let by_agent = collector.by_agent_snapshot();
        assert!(by_agent.by_agent.is_empty());
    }

    #[test]
    fn record_delegated_tokens_adds_tokens_without_incrementing_calls() {
        let collector = MetricsCollector::new();
        collector.record_llm_call(100, 1000); // parent call
        collector.record_delegated_tokens(500); // sub-agent tokens
        collector.record_delegated_tokens(300); // another sub-agent

        let m = collector.snapshot();
        assert_eq!(
            m.total_tokens_used, 1800,
            "tokens should include parent + delegated"
        );
        assert_eq!(
            m.total_llm_calls, 1,
            "delegated tokens should NOT increment llm_calls"
        );
    }

    // ── Phase 27 (sota-gaps-followup): routing telemetry ──

    pub mod routing {
        use super::*;

        #[test]
        fn metrics_collector_records_routing_decision() {
            let c = MetricsCollector::new();
            c.record_routing_decision("Retrieval", "Cheap", "haiku");
            let snap = c.routing_snapshot();
            assert_eq!(snap.total(), 1);
            assert_eq!(snap.count_for("Retrieval", "Cheap", "haiku"), 1);
        }

        #[test]
        fn metrics_collector_aggregates_decisions_by_tier() {
            let c = MetricsCollector::new();
            c.record_routing_decision("Retrieval", "Cheap", "haiku");
            c.record_routing_decision("Retrieval", "Cheap", "haiku");
            c.record_routing_decision("Planning", "Strong", "opus");
            let snap = c.routing_snapshot();
            assert_eq!(snap.total(), 3);
            assert_eq!(snap.count_for("Retrieval", "Cheap", "haiku"), 2);
            assert_eq!(snap.count_for("Planning", "Strong", "opus"), 1);
        }

        #[test]
        fn metrics_collector_routing_count_zero_for_unseen_combination() {
            let c = MetricsCollector::new();
            c.record_routing_decision("Planning", "Strong", "opus");
            assert_eq!(c.routing_snapshot().count_for("Planning", "Cheap", "haiku"), 0);
        }

        #[test]
        fn routing_histogram_serde_roundtrip() {
            let mut h = RoutingHistogram::default();
            h.record(&RoutingDecisionMetric {
                task_type: "Analysis".into(),
                tier: "Strong".into(),
                model_id: "opus".into(),
            });
            let json = serde_json::to_string(&h).unwrap();
            let back: RoutingHistogram = serde_json::from_str(&json).unwrap();
            assert_eq!(back.total(), 1);
        }

        #[test]
        fn routing_decision_metric_serde_preserves_all_fields() {
            let m = RoutingDecisionMetric {
                task_type: "Implementation".into(),
                tier: "Default".into(),
                model_id: "sonnet".into(),
            };
            let json = serde_json::to_string(&m).unwrap();
            let back: RoutingDecisionMetric = serde_json::from_str(&json).unwrap();
            assert_eq!(m, back);
        }
    }
}
