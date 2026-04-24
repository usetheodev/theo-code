//! All metrics computations used by `RunReport`. Split out of `mod.rs` so
//! the aggregate report type stays at the top and the per-category
//! computations are isolated here.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Extracted from `observability/report.rs`.
//! Behavior is byte-identical; each struct + its `compute_*` function are
//! public and re-exported from `mod.rs`.
//!
//! Categories:
//! - T3.8 `TokenMetrics`
//! - T3.9 `LoopMetrics` (+ `PhaseMetric`, `BudgetUtilization`)
//! - T3.10 `ToolBreakdown[]`
//! - T3.11 `ContextHealthMetrics`
//! - T3.12 `MemoryMetrics`
//! - `SubagentMetrics`
//! - `ErrorTaxonomy`

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use theo_domain::budget::{Budget, BudgetUsage, TokenUsage};

use crate::observability::projection::{ProjectedStep, StepOutcome};

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
    let dist: HashMap<String, PhaseMetric>;
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
    /// Phase 64 (benchmark-sota-metrics-plan Task 3.11): per-tool error
    /// breakdown by category (e.g. "timeout", "permission_denied", "other").
    #[serde(default)]
    pub error_categories: HashMap<String, u32>,
}

/// Classify a tool error payload into a category string.
fn classify_tool_error(payload: &str) -> String {
    let lower = payload.to_lowercase();
    if lower.contains("permission") || lower.contains("sandbox") || lower.contains("seccomp") || lower.contains("landlock") {
        "permission_denied".to_string()
    } else if lower.contains("not found") || lower.contains("no such file") || lower.contains("enoent") {
        "not_found".to_string()
    } else if lower.contains("invalid") || lower.contains("validation") || lower.contains("schema") {
        "validation_error".to_string()
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "timeout".to_string()
    } else {
        "execution_error".to_string()
    }
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
                let cat = classify_tool_error(&s.payload_summary);
                *entry.error_categories.entry(cat).or_insert(0) += 1;
            }
            Some(StepOutcome::Timeout) => {
                entry.failure_count += 1;
                *entry.error_categories.entry("timeout".to_string()).or_insert(0) += 1;
            }
            Some(StepOutcome::Skipped) => {
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
    let is_subagent = |s: &&ProjectedStep| {
        matches!(s.tool_name.as_deref(), Some("subagent" | "subagent_parallel"))
    };
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
