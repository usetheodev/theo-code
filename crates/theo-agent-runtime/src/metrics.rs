use std::sync::{Arc, RwLock};

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

/// Thread-safe metrics collector.
///
/// Uses RwLock to allow concurrent reads (snapshot) and exclusive writes (record).
pub struct MetricsCollector {
    metrics: Arc<RwLock<RuntimeMetrics>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(RuntimeMetrics::default())),
        }
    }

    pub fn record_llm_call(&self, duration_ms: u64, tokens: u64) {
        let mut m = self.metrics.write().expect("metrics write lock poisoned");
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
        let mut m = self.metrics.write().expect("metrics write lock poisoned");
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
        let mut m = self.metrics.write().expect("metrics write lock poisoned");
        m.total_tokens_used += tokens;
    }

    pub fn record_tool_call(&self, _tool_name: &str, duration_ms: u64, success: bool) {
        let mut m = self.metrics.write().expect("metrics write lock poisoned");
        m.total_tool_calls += 1;
        if success {
            m.successful_tool_calls += 1;
        }
        m.total_tool_call_ms += duration_ms;
        m.tool_call_count += 1;
    }

    pub fn record_retry(&self) {
        let mut m = self.metrics.write().expect("metrics write lock poisoned");
        m.total_retries += 1;
    }

    pub fn record_dlq_entry(&self) {
        let mut m = self.metrics.write().expect("metrics write lock poisoned");
        m.total_dlq_entries += 1;
    }

    pub fn record_run_complete(&self, converged: bool) {
        let mut m = self.metrics.write().expect("metrics write lock poisoned");
        m.total_runs += 1;
        if converged {
            m.converged_runs += 1;
        }
    }

    /// Accumulate dollar cost from an LLM call.
    pub fn record_cost(&self, cost_usd: f64) {
        let mut m = self.metrics.write().expect("metrics write lock poisoned");
        m.total_cost_usd += cost_usd;
    }

    pub fn record_iteration(&self, duration_ms: u64) {
        let mut m = self.metrics.write().expect("metrics write lock poisoned");
        m.total_iteration_ms += duration_ms;
        m.iteration_count += 1;
    }

    /// Returns a snapshot of current metrics (clone, does not consume).
    pub fn snapshot(&self) -> RuntimeMetrics {
        self.metrics
            .read()
            .expect("metrics read lock poisoned")
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
}
