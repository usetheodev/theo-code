//! Aggregate run report — composes all metric categories into a single
//! `RunReport` that is persisted as the summary line of the trajectory JSONL
//! and surfaced by the Dashboard.
//!
//! Categories:
//! - T3.7 `DerivedMetrics` (5 surrogate metrics)
//! - T3.8 `TokenMetrics`
//! - T3.9 `LoopMetrics`
//! - T3.10 `ToolBreakdown[]`
//! - T3.11 `ContextHealthMetrics`
//! - T3.12 `MemoryMetrics`
//! - T2.3 `IntegrityReport`

mod metrics;
pub use metrics::{
    compute_context_health, compute_error_taxonomy, compute_loop_metrics,
    compute_memory_metrics, compute_subagent_metrics, compute_token_metrics,
    compute_tool_breakdown, BudgetUtilization, ContextHealthMetrics, ErrorTaxonomy,
    LoopMetrics, MemoryMetrics, PhaseMetric, SubagentMetrics, TokenMetrics, ToolBreakdown,
};

use serde::{Deserialize, Serialize};

#[cfg(test)]
use theo_domain::budget::{Budget, BudgetUsage, TokenUsage};

use crate::observability::derived_metrics::DerivedMetrics;
#[cfg(test)]
use crate::observability::projection::{ProjectedStep, StepOutcome};
use crate::observability::reader::IntegrityReport;

// ----- T3.13 RunReport -----

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunReport {
    pub surrogate_metrics: DerivedMetrics,
    pub token_metrics: TokenMetrics,
    pub loop_metrics: LoopMetrics,
    pub tool_breakdown: Vec<ToolBreakdown>,
    pub context_health: ContextHealthMetrics,
    pub memory_metrics: MemoryMetrics,
    #[serde(default)]
    pub subagent_metrics: SubagentMetrics,
    #[serde(default)]
    pub error_taxonomy: ErrorTaxonomy,
    pub integrity: IntegrityReport,
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::event::EventKind;

    fn step(
        seq: u64,
        ts: u64,
        et: &str,
        tool: Option<&str>,
        outcome: Option<StepOutcome>,
        duration_ms: Option<u64>,
    ) -> ProjectedStep {
        ProjectedStep {
            sequence: seq,
            event_type: et.into(),
            event_kind: Some(EventKind::Tooling),
            timestamp: ts,
            entity_id: format!("e{}", seq),
            payload_summary: "".into(),
            duration_ms,
            tool_name: tool.map(String::from),
            outcome,
        }
    }

    #[test]
    fn test_token_breakdown_all_fields_populated() {
        let u = TokenUsage {
            input_tokens: 800,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_write_tokens: 50,
            reasoning_tokens: 30,
            estimated_cost_usd: 1.23,
        };
        let m = compute_token_metrics(&u, 2);
        assert_eq!(m.input_tokens, 800);
        assert_eq!(m.total_cost_usd, 1.23);
    }

    #[test]
    fn test_cache_hit_rate_zero_when_no_cache() {
        let u = TokenUsage {
            input_tokens: 1000,
            cache_read_tokens: 0,
            ..Default::default()
        };
        let m = compute_token_metrics(&u, 0);
        assert_eq!(m.cache_hit_rate, 0.0);
    }

    #[test]
    fn test_cache_hit_rate_computed_correctly() {
        let u = TokenUsage {
            input_tokens: 800,
            cache_read_tokens: 200,
            ..Default::default()
        };
        let m = compute_token_metrics(&u, 0);
        assert!((m.cache_hit_rate - 0.2).abs() < 1e-9);
    }

    #[test]
    fn test_tokens_per_edit_zero_when_no_edits() {
        let u = TokenUsage {
            input_tokens: 1000,
            ..Default::default()
        };
        let m = compute_token_metrics(&u, 0);
        assert_eq!(m.tokens_per_successful_edit, 0.0);
    }

    #[test]
    fn test_cost_usd_accumulated_correctly() {
        let u = TokenUsage {
            estimated_cost_usd: 3.14,
            ..Default::default()
        };
        let m = compute_token_metrics(&u, 0);
        assert_eq!(m.total_cost_usd, 3.14);
    }

    #[test]
    fn test_phase_distribution_has_four_phases() {
        let s = vec![step(0, 0, "LlmCallStart", None, None, None)];
        let budget = Budget::default();
        let usage = BudgetUsage::default();
        let lm = compute_loop_metrics(&s, &budget, &usage, false, 0, false, 0);
        assert_eq!(lm.phase_distribution.len(), 4);
    }

    #[test]
    fn test_done_blocked_tracked() {
        let lm = compute_loop_metrics(
            &[],
            &Budget::default(),
            &BudgetUsage::default(),
            false,
            0,
            false,
            3,
        );
        assert_eq!(lm.done_blocked_count, 3);
    }

    #[test]
    fn test_budget_utilization_correct() {
        let b = Budget {
            max_iterations: 200,
            max_tokens: 1000,
            max_time_secs: 100,
            max_tool_calls: 100,
        };
        let u = BudgetUsage {
            iterations_used: 50,
            tokens_used: 500,
            elapsed_secs: 50,
            tool_calls_used: 20,
        };
        let lm = compute_loop_metrics(&[], &b, &u, false, 0, false, 0);
        assert!((lm.budget_utilization.iterations_pct - 0.25).abs() < 1e-9);
        assert!((lm.budget_utilization.tokens_pct - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_evolution_attempts_counted() {
        let lm = compute_loop_metrics(
            &[],
            &Budget::default(),
            &BudgetUsage::default(),
            true,
            3,
            true,
            0,
        );
        assert_eq!(lm.evolution_attempts, 3);
        assert!(lm.evolution_success);
    }

    #[test]
    fn test_per_tool_counts_correct() {
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("read_file"), Some(StepOutcome::Success), Some(10)),
            step(1, 1, "ToolCallCompleted", Some("read_file"), Some(StepOutcome::Success), Some(20)),
            step(2, 2, "ToolCallCompleted", Some("read_file"), Some(StepOutcome::Success), Some(30)),
            step(3, 3, "ToolCallCompleted", Some("read_file"), Some(StepOutcome::Success), None),
            step(4, 4, "ToolCallCompleted", Some("read_file"), Some(StepOutcome::Failure { retryable: false }), None),
        ];
        let b = compute_tool_breakdown(&s);
        let r = b.iter().find(|b| b.tool_name == "read_file").unwrap();
        assert_eq!(r.call_count, 5);
        assert_eq!(r.success_count, 4);
        assert_eq!(r.failure_count, 1);
    }

    #[test]
    fn test_per_tool_latency_computed() {
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("t"), Some(StepOutcome::Success), Some(10)),
            step(1, 1, "ToolCallCompleted", Some("t"), Some(StepOutcome::Success), Some(30)),
        ];
        let b = compute_tool_breakdown(&s);
        assert!((b[0].avg_latency_ms - 20.0).abs() < 1e-9);
        assert_eq!(b[0].max_latency_ms, 30);
    }

    #[test]
    fn test_per_tool_sorted_by_call_count() {
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("a"), Some(StepOutcome::Success), None),
            step(1, 1, "ToolCallCompleted", Some("b"), Some(StepOutcome::Success), None),
            step(2, 2, "ToolCallCompleted", Some("b"), Some(StepOutcome::Success), None),
        ];
        let b = compute_tool_breakdown(&s);
        assert_eq!(b[0].tool_name, "b");
    }

    #[test]
    fn test_per_tool_empty_when_no_tools() {
        let b = compute_tool_breakdown(&[]);
        assert!(b.is_empty());
    }

    #[test]
    fn test_context_growth_positive_when_growing() {
        let mut s: Vec<ProjectedStep> = Vec::new();
        for (i, size) in [100u64, 200, 300, 400].iter().enumerate() {
            s.push(ProjectedStep {
                sequence: i as u64,
                event_type: "LlmCallEnd".into(),
                event_kind: None,
                timestamp: i as u64,
                entity_id: "e".into(),
                payload_summary: format!("{{\"context_tokens\":{}}}", size),
                duration_ms: None,
                tool_name: None,
                outcome: None,
            });
        }
        let m = compute_context_health(&s, 0.0, 0.0, 0.0);
        assert!(m.context_growth_rate > 0.0);
    }

    #[test]
    fn test_compaction_savings_correct() {
        let s = vec![ProjectedStep {
            sequence: 0,
            event_type: "ContextOverflowRecovery".into(),
            event_kind: None,
            timestamp: 0,
            entity_id: "e".into(),
            payload_summary: "{\"tokens_before\":10000,\"tokens_after\":3000}".into(),
            duration_ms: None,
            tool_name: None,
            outcome: None,
        }];
        let m = compute_context_health(&s, 0.0, 0.0, 0.0);
        assert!((m.compaction_savings_ratio - 0.7).abs() < 1e-9);
    }

    #[test]
    fn test_refetch_rate_from_context_metrics() {
        let m = compute_context_health(&[], 0.25, 0.0, 0.0);
        assert_eq!(m.refetch_rate, 0.25);
    }

    #[test]
    fn test_usefulness_avg_computed() {
        let m = compute_context_health(&[], 0.0, 0.0, 0.5);
        assert_eq!(m.usefulness_avg, 0.5);
    }

    #[test]
    fn test_episode_counts_from_events() {
        let m = compute_memory_metrics(&[], 3, 2, 0, 0);
        assert_eq!(m.episodes_injected, 3);
        assert_eq!(m.episodes_created, 2);
    }

    #[test]
    fn test_hypothesis_counts_from_events() {
        let s = vec![
            step(0, 0, "HypothesisFormed", None, None, None),
            step(1, 1, "HypothesisFormed", None, None, None),
            step(2, 2, "HypothesisInvalidated", None, None, None),
        ];
        let m = compute_memory_metrics(&s, 0, 0, 0, 0);
        assert_eq!(m.hypotheses_formed, 2);
        assert_eq!(m.hypotheses_invalidated, 1);
        assert_eq!(m.hypotheses_active, 1);
    }

    #[test]
    fn test_constraints_counted_from_events() {
        let s = vec![step(0, 0, "ConstraintLearned", None, None, None)];
        let m = compute_memory_metrics(&s, 0, 0, 0, 0);
        assert_eq!(m.constraints_learned, 1);
    }

    #[test]
    fn test_fingerprints_new_vs_recurrent() {
        let m = compute_memory_metrics(&[], 0, 0, 5, 2);
        assert_eq!(m.failure_fingerprints_new, 5);
        assert_eq!(m.failure_fingerprints_recurrent, 2);
    }

    // --- T3.13 ---

    #[test]
    fn test_run_report_serde_roundtrip() {
        let r = RunReport::default();
        let j = serde_json::to_string(&r).unwrap();
        let back: RunReport = serde_json::from_str(&j).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn test_run_report_all_sections_populated() {
        let r = RunReport::default();
        // Just ensure all sections exist
        let _ = r.surrogate_metrics;
        let _ = r.token_metrics;
        let _ = r.loop_metrics;
        let _ = r.tool_breakdown;
        let _ = r.context_health;
        let _ = r.memory_metrics;
        let _ = r.subagent_metrics;
        let _ = r.error_taxonomy;
        let _ = r.integrity;
    }

    // --- SubagentMetrics ---

    #[test]
    fn test_subagent_metrics_empty_when_no_subagent_calls() {
        let s = vec![step(0, 0, "ToolCallCompleted", Some("bash"), Some(StepOutcome::Success), None)];
        let m = compute_subagent_metrics(&s);
        assert_eq!(m.spawned, 0);
        assert_eq!(m.success_rate, 0.0);
    }

    #[test]
    fn test_subagent_metrics_counts_subagent_tool() {
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("subagent"), Some(StepOutcome::Success), Some(5000)),
            step(1, 1, "ToolCallCompleted", Some("subagent"), Some(StepOutcome::Failure { retryable: false }), Some(3000)),
            step(2, 2, "ToolCallCompleted", Some("subagent_parallel"), Some(StepOutcome::Success), Some(8000)),
        ];
        let m = compute_subagent_metrics(&s);
        assert_eq!(m.spawned, 3);
        assert_eq!(m.succeeded, 2);
        assert_eq!(m.failed, 1);
        assert!((m.success_rate - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(m.max_duration_ms, 8000);
    }

    // --- ErrorTaxonomy ---

    #[test]
    fn test_error_taxonomy_empty_when_no_errors() {
        let tax = compute_error_taxonomy(&[]);
        assert_eq!(tax.total_errors, 0);
    }

    #[test]
    fn test_error_taxonomy_classifies_network() {
        let s = vec![step(0, 0, "Error", None, None, None)];
        let s = vec![ProjectedStep {
            payload_summary: "network timeout connecting to api.openai.com".into(),
            ..s[0].clone()
        }];
        let tax = compute_error_taxonomy(&s);
        assert_eq!(tax.network_errors, 1);
        assert_eq!(tax.total_errors, 1);
    }

    #[test]
    fn test_error_taxonomy_classifies_budget() {
        let s = vec![step(0, 0, "BudgetExceeded", None, None, None)];
        let tax = compute_error_taxonomy(&s);
        assert_eq!(tax.budget_errors, 1);
        assert_eq!(tax.total_errors, 1);
    }

    #[test]
    fn test_error_taxonomy_classifies_failure_mode() {
        let s = vec![ProjectedStep {
            sequence: 0,
            event_type: "Error".into(),
            event_kind: Some(EventKind::Failure),
            timestamp: 0,
            entity_id: "e".into(),
            payload_summary: "{\"failure_mode\":\"WeakVerification\"}".into(),
            duration_ms: None,
            tool_name: None,
            outcome: None,
        }];
        let tax = compute_error_taxonomy(&s);
        assert_eq!(tax.failure_mode_errors, 1);
    }
}
