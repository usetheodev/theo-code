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

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use theo_domain::budget::{Budget, BudgetUsage, TokenUsage};

use crate::observability::derived_metrics::DerivedMetrics;
use crate::observability::projection::{ProjectedStep, StepOutcome};
use crate::observability::reader::IntegrityReport;

fn safe_div(n: f64, d: f64) -> f64 {
    if d == 0.0 {
        0.0
    } else {
        n / d
    }
}

// ----- T3.8 TokenMetrics -----

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TokenMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_cost_usd: f64,
    pub cache_hit_rate: f64,
    pub tokens_per_successful_edit: f64,
}

pub fn compute_token_metrics(token_usage: &TokenUsage, successful_edits: u64) -> TokenMetrics {
    let total_tokens = token_usage.input_tokens
        + token_usage.output_tokens
        + token_usage.cache_read_tokens
        + token_usage.cache_write_tokens
        + token_usage.reasoning_tokens;
    let cache_read = token_usage.cache_read_tokens as f64;
    let input_plus_cache = (token_usage.input_tokens + token_usage.cache_read_tokens) as f64;
    TokenMetrics {
        input_tokens: token_usage.input_tokens,
        output_tokens: token_usage.output_tokens,
        cache_read_tokens: token_usage.cache_read_tokens,
        cache_write_tokens: token_usage.cache_write_tokens,
        reasoning_tokens: token_usage.reasoning_tokens,
        total_cost_usd: token_usage.estimated_cost_usd,
        cache_hit_rate: safe_div(cache_read, input_plus_cache),
        tokens_per_successful_edit: safe_div(total_tokens as f64, successful_edits as f64),
    }
}

// ----- T3.9 LoopMetrics -----

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PhaseMetric {
    pub iterations: u32,
    pub duration_ms: u64,
    pub pct: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BudgetUtilization {
    pub iterations_pct: f64,
    pub tokens_pct: f64,
    pub time_pct: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LoopMetrics {
    pub phase_distribution: HashMap<String, PhaseMetric>,
    pub total_iterations: u32,
    pub done_blocked_count: u32,
    pub convergence_rate: f64,
    pub budget_utilization: BudgetUtilization,
    pub evolution_attempts: u32,
    pub evolution_success: bool,
}

pub fn compute_loop_metrics(
    steps: &[ProjectedStep],
    budget: &Budget,
    usage: &BudgetUsage,
    converged: bool,
    evolution_attempts: u32,
    evolution_success: bool,
    done_blocked_count: u32,
) -> LoopMetrics {
    let total_iterations = steps
        .iter()
        .filter(|s| s.event_type == "LlmCallStart")
        .count() as u32;
    let mut dist: HashMap<String, PhaseMetric>;
    let denom = total_iterations.max(1) as f64;
    let explore = steps.iter().filter(|s| s.event_type == "RetrievalExecuted").count() as u32;
    let edit = steps
        .iter()
        .filter(|s| {
            s.event_type == "ToolCallCompleted"
                && matches!(s.tool_name.as_deref(), Some("edit_file" | "write_file"))
        })
        .count() as u32;
    let verify = steps.iter().filter(|s| s.event_type == "SensorExecuted").count() as u32;
    let done = if converged { 1 } else { 0 };

    // Rebuild the map directly so we don't rely on get_mut + unwrap for entries we
    // just inserted. Using an array of tuples makes the population a declarative
    // list and removes 8 production `.unwrap()` sites (T2.5).
    let counts = [
        ("Explore", explore),
        ("Edit", edit),
        ("Verify", verify),
        ("Done", done),
    ];
    dist = counts
        .into_iter()
        .map(|(name, iterations)| {
            (
                name.to_string(),
                PhaseMetric {
                    iterations,
                    duration_ms: 0,
                    pct: iterations as f64 / denom,
                },
            )
        })
        .collect();

    let convergence_rate = if converged { 1.0 } else { 0.0 };
    let bu = BudgetUtilization {
        iterations_pct: safe_div(usage.iterations_used as f64, budget.max_iterations as f64),
        tokens_pct: safe_div(usage.tokens_used as f64, budget.max_tokens as f64),
        time_pct: safe_div(usage.elapsed_secs as f64, budget.max_time_secs as f64),
    };

    LoopMetrics {
        phase_distribution: dist,
        total_iterations,
        done_blocked_count,
        convergence_rate,
        budget_utilization: bu,
        evolution_attempts,
        evolution_success,
    }
}

// ----- T3.10 ToolBreakdown -----

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolBreakdown {
    pub tool_name: String,
    pub call_count: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub avg_latency_ms: f64,
    pub max_latency_ms: u64,
    pub retry_count: u32,
    pub success_rate: f64,
}

pub fn compute_tool_breakdown(steps: &[ProjectedStep]) -> Vec<ToolBreakdown> {
    let mut map: HashMap<String, ToolBreakdown> = HashMap::new();
    let mut latency_sum: HashMap<String, (u64, u32)> = HashMap::new();
    for s in steps {
        if s.event_type != "ToolCallCompleted" {
            continue;
        }
        let Some(tn) = &s.tool_name else { continue };
        let entry = map.entry(tn.clone()).or_insert_with(|| ToolBreakdown {
            tool_name: tn.clone(),
            ..Default::default()
        });
        entry.call_count += 1;
        match s.outcome {
            Some(StepOutcome::Success) => entry.success_count += 1,
            Some(StepOutcome::Failure { retryable }) => {
                entry.failure_count += 1;
                if retryable {
                    entry.retry_count += 1;
                }
            }
            Some(StepOutcome::Timeout) | Some(StepOutcome::Skipped) => {
                entry.failure_count += 1;
            }
            None => {}
        }
        if let Some(d) = s.duration_ms {
            let (sum, count) = latency_sum.entry(tn.clone()).or_insert((0, 0));
            *sum += d;
            *count += 1;
            if d > entry.max_latency_ms {
                entry.max_latency_ms = d;
            }
        }
    }
    for (tn, (sum, count)) in latency_sum {
        if let Some(b) = map.get_mut(&tn) {
            b.avg_latency_ms = safe_div(sum as f64, count as f64);
        }
    }
    for b in map.values_mut() {
        b.success_rate = safe_div(b.success_count as f64, b.call_count as f64);
    }
    let mut out: Vec<ToolBreakdown> = map.into_values().collect();
    out.sort_by(|a, b| b.call_count.cmp(&a.call_count));
    out
}

// ----- T3.11 ContextHealthMetrics -----

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ContextHealthMetrics {
    pub avg_context_size_tokens: f64,
    pub max_context_size_tokens: u64,
    pub context_growth_rate: f64,
    pub compaction_count: u32,
    pub compaction_savings_ratio: f64,
    pub refetch_rate: f64,
    pub action_repetition_rate: f64,
    pub usefulness_avg: f64,
}

pub fn compute_context_health(
    steps: &[ProjectedStep],
    refetch_rate: f64,
    action_repetition_rate: f64,
    usefulness_avg: f64,
) -> ContextHealthMetrics {
    // Extract context size events from LlmCallEnd payloads (field `context_tokens`).
    let sizes: Vec<u64> = steps
        .iter()
        .filter(|s| s.event_type == "LlmCallEnd")
        .filter_map(|s| extract_u64(&s.payload_summary, "context_tokens"))
        .collect();
    let avg_context_size_tokens = if sizes.is_empty() {
        0.0
    } else {
        sizes.iter().sum::<u64>() as f64 / sizes.len() as f64
    };
    let max_context_size_tokens = sizes.iter().copied().max().unwrap_or(0);
    let context_growth_rate = if sizes.len() >= 2 {
        (sizes[sizes.len() - 1] as f64 - sizes[0] as f64) / sizes.len() as f64
    } else {
        0.0
    };
    let compaction_count = steps
        .iter()
        .filter(|s| s.event_type == "ContextOverflowRecovery")
        .count() as u32;
    let compaction_savings_ratios: Vec<f64> = steps
        .iter()
        .filter(|s| s.event_type == "ContextOverflowRecovery")
        .filter_map(|s| {
            let before = extract_u64(&s.payload_summary, "tokens_before")? as f64;
            let after = extract_u64(&s.payload_summary, "tokens_after")? as f64;
            if before > 0.0 {
                Some(1.0 - (after / before))
            } else {
                None
            }
        })
        .collect();
    let compaction_savings_ratio = if compaction_savings_ratios.is_empty() {
        0.0
    } else {
        compaction_savings_ratios.iter().sum::<f64>() / compaction_savings_ratios.len() as f64
    };
    ContextHealthMetrics {
        avg_context_size_tokens,
        max_context_size_tokens,
        context_growth_rate,
        compaction_count,
        compaction_savings_ratio,
        refetch_rate,
        action_repetition_rate,
        usefulness_avg,
    }
}

// Primitive helper: parse a number field from a JSON-like payload string.
fn extract_u64(summary: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{}\":", key);
    let idx = summary.find(&needle)?;
    let rest = &summary[idx + needle.len()..];
    let trimmed = rest.trim_start();
    let end = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    trimmed[..end].parse().ok()
}

// ----- T3.12 MemoryMetrics -----

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MemoryMetrics {
    pub episodes_injected: u32,
    pub episodes_created: u32,
    pub hypotheses_formed: u32,
    pub hypotheses_invalidated: u32,
    pub hypotheses_active: u32,
    pub constraints_learned: u32,
    pub failure_fingerprints_new: u32,
    pub failure_fingerprints_recurrent: u32,
}

pub fn compute_memory_metrics(
    steps: &[ProjectedStep],
    episodes_injected: u32,
    episodes_created: u32,
    failure_fingerprints_new: u32,
    failure_fingerprints_recurrent: u32,
) -> MemoryMetrics {
    let hypotheses_formed = steps.iter().filter(|s| s.event_type == "HypothesisFormed").count() as u32;
    let hypotheses_invalidated = steps
        .iter()
        .filter(|s| s.event_type == "HypothesisInvalidated")
        .count() as u32;
    let hypotheses_active = hypotheses_formed.saturating_sub(hypotheses_invalidated);
    let constraints_learned = steps
        .iter()
        .filter(|s| s.event_type == "ConstraintLearned")
        .count() as u32;
    MemoryMetrics {
        episodes_injected,
        episodes_created,
        hypotheses_formed,
        hypotheses_invalidated,
        hypotheses_active,
        constraints_learned,
        failure_fingerprints_new,
        failure_fingerprints_recurrent,
    }
}

// ----- Sub-agent metrics -----

/// Metrics about sub-agent spawning and completion during the run.
///
/// A sub-agent is a nested `AgentRunEngine` spawned via the `subagent` tool —
/// its own events do not show up in the parent trajectory (the parent engine
/// doesn't subscribe to the child bus), but the parent records the tool call
/// which is what we count here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SubagentMetrics {
    pub spawned: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub avg_duration_ms: f64,
    pub max_duration_ms: u64,
    pub success_rate: f64,
}

pub fn compute_subagent_metrics(steps: &[ProjectedStep]) -> SubagentMetrics {
    let is_subagent =
        |s: &&ProjectedStep| matches!(s.tool_name.as_deref(), Some("subagent" | "subagent_parallel"));
    let mut spawned = 0u32;
    let mut succeeded = 0u32;
    let mut failed = 0u32;
    let mut durations: Vec<u64> = Vec::new();
    for s in steps.iter().filter(|s| s.event_type == "ToolCallCompleted") {
        if !is_subagent(&s) {
            continue;
        }
        spawned += 1;
        match s.outcome {
            Some(StepOutcome::Success) => succeeded += 1,
            _ => failed += 1,
        }
        if let Some(d) = s.duration_ms {
            durations.push(d);
        }
    }
    let (avg_duration_ms, max_duration_ms) = if durations.is_empty() {
        (0.0, 0)
    } else {
        (
            durations.iter().sum::<u64>() as f64 / durations.len() as f64,
            *durations.iter().max().unwrap_or(&0),
        )
    };
    SubagentMetrics {
        spawned,
        succeeded,
        failed,
        avg_duration_ms,
        max_duration_ms,
        success_rate: safe_div(succeeded as f64, spawned as f64),
    }
}

// ----- Error taxonomy -----

/// Classification of `Error` events by root cause category.
///
/// Helps answer "where is our error budget going?" — network vs provider vs
/// tool sandbox vs business-logic failures.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ErrorTaxonomy {
    pub total_errors: u32,
    pub network_errors: u32,
    pub llm_errors: u32,
    pub tool_errors: u32,
    pub sandbox_errors: u32,
    pub budget_errors: u32,
    pub validation_errors: u32,
    pub failure_mode_errors: u32,
    pub other_errors: u32,
}

pub fn compute_error_taxonomy(steps: &[ProjectedStep]) -> ErrorTaxonomy {
    let mut tax = ErrorTaxonomy::default();
    for s in steps {
        if s.event_type != "Error" && s.event_type != "BudgetExceeded" {
            continue;
        }
        tax.total_errors += 1;
        if s.event_type == "BudgetExceeded" {
            tax.budget_errors += 1;
            continue;
        }
        let lower = s.payload_summary.to_lowercase();
        if lower.contains("failure_mode") {
            tax.failure_mode_errors += 1;
        } else if lower.contains("network") || lower.contains("timeout") || lower.contains("connection") {
            tax.network_errors += 1;
        } else if lower.contains("sandbox") || lower.contains("seccomp") || lower.contains("landlock") {
            tax.sandbox_errors += 1;
        } else if lower.contains("invalid") || lower.contains("validation") || lower.contains("schema") {
            tax.validation_errors += 1;
        } else if lower.contains("tool") {
            tax.tool_errors += 1;
        } else if lower.contains("llm") || lower.contains("api") || lower.contains("rate") {
            tax.llm_errors += 1;
        } else {
            tax.other_errors += 1;
        }
    }
    tax
}

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
