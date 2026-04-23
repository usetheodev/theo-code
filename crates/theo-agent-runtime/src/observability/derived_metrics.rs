//! Derived (surrogate) metrics computed over projected trajectories.
//!
//! Each metric is a `SurrogateMetric` — a numeric value with a confidence
//! score, numerator, denominator, and a caveat string describing its
//! surrogate nature.

use serde::{Deserialize, Serialize};

use crate::observability::projection::{ProjectedStep, StepOutcome};
use crate::observability::reader::IntegrityReport;

/// A surrogate (proxy) metric — never represents a direct ground-truth
/// measurement. Consumers are expected to read the `caveat` before using it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SurrogateMetric {
    pub value: f64,
    pub confidence: f64,
    pub numerator: f64,
    pub denominator: f64,
    pub is_surrogate: bool,
    pub caveat: std::borrow::Cow<'static, str>,
}

impl Default for SurrogateMetric {
    fn default() -> Self {
        Self {
            value: 0.0,
            confidence: 0.0,
            numerator: 0.0,
            denominator: 0.0,
            is_surrogate: true,
            caveat: std::borrow::Cow::Borrowed(""),
        }
    }
}

/// Bundle of derived metrics computed for a single run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DerivedMetrics {
    pub doom_loop_frequency: SurrogateMetric,
    pub llm_efficiency: SurrogateMetric,
    pub context_waste_ratio: SurrogateMetric,
    pub hypothesis_churn_rate: SurrogateMetric,
    pub time_to_first_tool_ms: SurrogateMetric,
}

fn safe_div(num: f64, den: f64) -> f64 {
    if den == 0.0 {
        0.0
    } else {
        num / den
    }
}

/// T3.2: doom_loop_frequency — sliding-window repetition rate of tool calls.
pub fn compute_doom_loop_frequency(steps: &[ProjectedStep]) -> SurrogateMetric {
    const W: usize = 10;
    let tool_calls: Vec<&ProjectedStep> = steps
        .iter()
        .filter(|s| s.event_type == "ToolCallCompleted" && s.tool_name.is_some())
        .collect();
    let total = tool_calls.len();
    if total == 0 {
        return SurrogateMetric {
            value: 0.0,
            confidence: 0.0,
            numerator: 0.0,
            denominator: 0.0,
            is_surrogate: true,
            caveat: std::borrow::Cow::Borrowed("Proxy for stuck loops: repeated identical tool calls in a sliding window."),
        };
    }
    let mut repetitions = 0u64;
    for i in 0..total {
        let low = i.saturating_sub(W.saturating_sub(1));
        let key_i = format!(
            "{}|{}",
            tool_calls[i].tool_name.clone().unwrap_or_default(),
            tool_calls[i].payload_summary
        );
        for j in low..i {
            let key_j = format!(
                "{}|{}",
                tool_calls[j].tool_name.clone().unwrap_or_default(),
                tool_calls[j].payload_summary
            );
            if key_i == key_j {
                repetitions += 1;
                break;
            }
        }
    }
    let num = repetitions as f64;
    let den = total as f64;
    SurrogateMetric {
        value: safe_div(num, den),
        confidence: 1.0,
        numerator: num,
        denominator: den,
        is_surrogate: true,
        caveat: std::borrow::Cow::Borrowed("Proxy for stuck loops: repeated identical tool calls in a sliding window."),
    }
}

/// T3.3: llm_efficiency — distinct successful tools per LLM call.
pub fn compute_llm_efficiency(steps: &[ProjectedStep]) -> SurrogateMetric {
    let llm_calls = steps.iter().filter(|s| s.event_type == "LlmCallStart").count();
    if llm_calls == 0 {
        return SurrogateMetric {
            value: 0.0,
            confidence: 0.0,
            numerator: 0.0,
            denominator: 0.0,
            is_surrogate: true,
            caveat: std::borrow::Cow::Borrowed("Approximates productive work: distinct successful tools per LLM call."),
        };
    }
    let mut distinct = std::collections::HashSet::new();
    for s in steps {
        if s.event_type == "ToolCallCompleted"
            && matches!(s.outcome, Some(StepOutcome::Success))
            && let Some(tn) = &s.tool_name {
                distinct.insert(tn.clone());
            }
    }
    let num = distinct.len() as f64;
    let den = llm_calls as f64;
    SurrogateMetric {
        value: safe_div(num, den),
        confidence: 1.0,
        numerator: num,
        denominator: den,
        is_surrogate: true,
        caveat: std::borrow::Cow::Borrowed("Approximates productive work: distinct successful tools per LLM call."),
    }
}

/// T3.4: context_waste_ratio — context overflow events per iteration.
pub fn compute_context_waste_ratio(steps: &[ProjectedStep]) -> SurrogateMetric {
    let iterations = steps
        .iter()
        .filter(|s| s.event_type == "LlmCallStart")
        .count();
    if iterations == 0 {
        return SurrogateMetric {
            value: 0.0,
            confidence: 0.0,
            numerator: 0.0,
            denominator: 0.0,
            is_surrogate: true,
            caveat: std::borrow::Cow::Borrowed("Proxy for context bloat: overflow events per iteration."),
        };
    }
    let overflows = steps
        .iter()
        .filter(|s| s.event_type == "ContextOverflowRecovery")
        .count();
    let num = overflows as f64;
    let den = iterations as f64;
    SurrogateMetric {
        value: safe_div(num, den),
        confidence: 1.0,
        numerator: num,
        denominator: den,
        is_surrogate: true,
        caveat: std::borrow::Cow::Borrowed("Proxy for context bloat: overflow events per iteration."),
    }
}

/// T3.5: hypothesis_churn_rate — invalidated / formed.
pub fn compute_hypothesis_churn_rate(steps: &[ProjectedStep]) -> SurrogateMetric {
    let formed = steps.iter().filter(|s| s.event_type == "HypothesisFormed").count();
    if formed == 0 {
        return SurrogateMetric {
            value: 0.0,
            confidence: 0.0,
            numerator: 0.0,
            denominator: 0.0,
            is_surrogate: true,
            caveat: std::borrow::Cow::Borrowed("Approximates reasoning churn: invalidated / formed hypotheses."),
        };
    }
    let invalidated = steps
        .iter()
        .filter(|s| s.event_type == "HypothesisInvalidated")
        .count();
    let num = invalidated as f64;
    let den = formed as f64;
    SurrogateMetric {
        value: safe_div(num, den),
        confidence: 1.0,
        numerator: num,
        denominator: den,
        is_surrogate: true,
        caveat: std::borrow::Cow::Borrowed("Approximates reasoning churn: invalidated / formed hypotheses."),
    }
}

/// T3.6: time_to_first_tool_ms — duration from RunInitialized to first ToolCallDispatched.
pub fn compute_time_to_first_tool(steps: &[ProjectedStep]) -> SurrogateMetric {
    let caveat: std::borrow::Cow<'static, str> = std::borrow::Cow::Borrowed(
        "Proxy for startup latency: ms from RunInitialized to first ToolCallDispatched.",
    );
    let init = steps.iter().find(|s| s.event_type == "RunInitialized");
    let Some(init) = init else {
        return SurrogateMetric {
            value: 0.0,
            confidence: 0.0,
            numerator: 0.0,
            denominator: 0.0,
            is_surrogate: true,
            caveat,
        };
    };
    let first_tool = steps
        .iter()
        .find(|s| s.event_type == "ToolCallDispatched")
        .map(|s| s.timestamp);
    let last_ts = steps.last().map(|s| s.timestamp).unwrap_or(init.timestamp);
    let delta = match first_tool {
        Some(t) => t.saturating_sub(init.timestamp) as f64,
        None => last_ts.saturating_sub(init.timestamp) as f64,
    };
    SurrogateMetric {
        value: delta,
        confidence: 1.0,
        numerator: delta,
        denominator: 1.0,
        is_surrogate: true,
        caveat,
    }
}

/// T3.7: compute all 5 metrics, scaling their confidence by integrity.confidence.
pub fn compute_all(steps: &[ProjectedStep], integrity: &IntegrityReport) -> DerivedMetrics {
    let mut m = DerivedMetrics {
        doom_loop_frequency: compute_doom_loop_frequency(steps),
        llm_efficiency: compute_llm_efficiency(steps),
        context_waste_ratio: compute_context_waste_ratio(steps),
        hypothesis_churn_rate: compute_hypothesis_churn_rate(steps),
        time_to_first_tool_ms: compute_time_to_first_tool(steps),
    };
    for metric in [
        &mut m.doom_loop_frequency,
        &mut m.llm_efficiency,
        &mut m.context_waste_ratio,
        &mut m.hypothesis_churn_rate,
        &mut m.time_to_first_tool_ms,
    ] {
        metric.confidence *= integrity.confidence;
    }
    m
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
        summary: &str,
    ) -> ProjectedStep {
        ProjectedStep {
            sequence: seq,
            event_type: et.into(),
            event_kind: Some(EventKind::Tooling),
            timestamp: ts,
            entity_id: format!("e{}", seq),
            payload_summary: summary.into(),
            duration_ms: None,
            tool_name: tool.map(String::from),
            outcome,
        }
    }

    fn full_integrity() -> IntegrityReport {
        IntegrityReport {
            confidence: 1.0,
            ..Default::default()
        }
    }

    // --- T3.1 ---

    #[test]
    fn test_surrogate_metric_serde_roundtrip() {
        let m = SurrogateMetric {
            value: 0.5,
            confidence: 0.9,
            numerator: 5.0,
            denominator: 10.0,
            is_surrogate: true,
            caveat: std::borrow::Cow::Borrowed("test"),
        };
        let j = serde_json::to_string(&m).unwrap();
        let back: SurrogateMetric = serde_json::from_str(&j).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn test_surrogate_metric_always_marked_surrogate() {
        let m = SurrogateMetric::default();
        assert!(m.is_surrogate);
    }

    #[test]
    fn test_derived_metrics_default_all_zero() {
        let m = DerivedMetrics::default();
        assert_eq!(m.doom_loop_frequency.value, 0.0);
        assert_eq!(m.llm_efficiency.value, 0.0);
        assert_eq!(m.context_waste_ratio.value, 0.0);
    }

    // --- T3.2 ---

    #[test]
    fn test_doom_loop_zero_when_no_repetitions() {
        let steps: Vec<ProjectedStep> = (0..10)
            .map(|i| step(i, i, "ToolCallCompleted", Some(&format!("t{}", i)), None, ""))
            .collect();
        let m = compute_doom_loop_frequency(&steps);
        assert_eq!(m.value, 0.0);
    }

    #[test]
    fn test_doom_loop_detects_identical_calls() {
        let s = vec![
            step(0, 0, "ToolCallCompleted", Some("read"), None, "a"),
            step(1, 1, "ToolCallCompleted", Some("read"), None, "a"),
            step(2, 2, "ToolCallCompleted", Some("read"), None, "a"),
            step(3, 3, "ToolCallCompleted", Some("other"), None, "x"),
        ];
        let m = compute_doom_loop_frequency(&s);
        assert!(m.value > 0.0);
    }

    #[test]
    fn test_doom_loop_zero_denominator() {
        let s: Vec<ProjectedStep> = vec![];
        let m = compute_doom_loop_frequency(&s);
        assert_eq!(m.value, 0.0);
        assert_eq!(m.confidence, 0.0);
    }

    #[test]
    fn test_doom_loop_caveat_present() {
        let m = compute_doom_loop_frequency(&[]);
        assert!(!m.caveat.is_empty());
    }

    // --- T3.3 ---

    #[test]
    fn test_llm_efficiency_perfect_run() {
        let mut s = vec![];
        for i in 0..5u64 {
            s.push(step(i, i, "LlmCallStart", None, None, ""));
            s.push(step(
                i + 100,
                i + 100,
                "ToolCallCompleted",
                Some(&format!("t{}", i)),
                Some(StepOutcome::Success),
                "",
            ));
        }
        let m = compute_llm_efficiency(&s);
        assert_eq!(m.value, 1.0);
    }

    #[test]
    fn test_llm_efficiency_no_tools() {
        let s: Vec<ProjectedStep> = (0..5u64).map(|i| step(i, i, "LlmCallStart", None, None, "")).collect();
        let m = compute_llm_efficiency(&s);
        assert_eq!(m.value, 0.0);
    }

    #[test]
    fn test_llm_efficiency_no_llm_calls() {
        let m = compute_llm_efficiency(&[]);
        assert_eq!(m.value, 0.0);
        assert_eq!(m.confidence, 0.0);
    }

    #[test]
    fn test_llm_efficiency_duplicate_tools_not_counted() {
        let s = vec![
            step(0, 0, "LlmCallStart", None, None, ""),
            step(1, 1, "LlmCallStart", None, None, ""),
            step(2, 2, "LlmCallStart", None, None, ""),
            step(3, 3, "ToolCallCompleted", Some("same"), Some(StepOutcome::Success), ""),
            step(4, 4, "ToolCallCompleted", Some("same"), Some(StepOutcome::Success), ""),
            step(5, 5, "ToolCallCompleted", Some("same"), Some(StepOutcome::Success), ""),
        ];
        let m = compute_llm_efficiency(&s);
        assert!((m.value - (1.0 / 3.0)).abs() < 1e-9);
    }

    // --- T3.4 ---

    #[test]
    fn test_context_waste_zero_overflows() {
        let s: Vec<ProjectedStep> = (0..10u64).map(|i| step(i, i, "LlmCallStart", None, None, "")).collect();
        let m = compute_context_waste_ratio(&s);
        assert_eq!(m.value, 0.0);
    }

    #[test]
    fn test_context_waste_high_overflow() {
        let mut s: Vec<ProjectedStep> = (0..10u64).map(|i| step(i, i, "LlmCallStart", None, None, "")).collect();
        for i in 0..5u64 {
            s.push(step(i + 100, i + 100, "ContextOverflowRecovery", None, None, ""));
        }
        let m = compute_context_waste_ratio(&s);
        assert_eq!(m.value, 0.5);
    }

    #[test]
    fn test_context_waste_no_iterations() {
        let m = compute_context_waste_ratio(&[]);
        assert_eq!(m.value, 0.0);
        assert_eq!(m.confidence, 0.0);
    }

    // --- T3.5 ---

    #[test]
    fn test_churn_rate_no_hypotheses() {
        let m = compute_hypothesis_churn_rate(&[]);
        assert_eq!(m.value, 0.0);
        assert_eq!(m.confidence, 0.0);
    }

    #[test]
    fn test_churn_rate_no_invalidations() {
        let s: Vec<ProjectedStep> = (0..5u64).map(|i| step(i, i, "HypothesisFormed", None, None, "")).collect();
        let m = compute_hypothesis_churn_rate(&s);
        assert_eq!(m.value, 0.0);
    }

    #[test]
    fn test_churn_rate_all_invalidated() {
        let mut s = vec![];
        for i in 0..3u64 {
            s.push(step(i, i, "HypothesisFormed", None, None, ""));
            s.push(step(i + 100, i + 100, "HypothesisInvalidated", None, None, ""));
        }
        let m = compute_hypothesis_churn_rate(&s);
        assert_eq!(m.value, 1.0);
    }

    #[test]
    fn test_churn_rate_more_invalidated_than_formed() {
        let mut s = vec![];
        for i in 0..2u64 {
            s.push(step(i, i, "HypothesisFormed", None, None, ""));
        }
        for i in 0..4u64 {
            s.push(step(i + 100, i + 100, "HypothesisInvalidated", None, None, ""));
        }
        let m = compute_hypothesis_churn_rate(&s);
        assert_eq!(m.value, 2.0);
    }

    // --- T3.6 ---

    #[test]
    fn test_ttft_normal_case() {
        let s = vec![
            step(0, 1000, "RunInitialized", None, None, ""),
            step(1, 1500, "ToolCallDispatched", Some("x"), None, ""),
        ];
        let m = compute_time_to_first_tool(&s);
        assert_eq!(m.value, 500.0);
    }

    #[test]
    fn test_ttft_no_tool_calls() {
        let s = vec![
            step(0, 1000, "RunInitialized", None, None, ""),
            step(1, 5000, "RunStateChanged", None, None, ""),
        ];
        let m = compute_time_to_first_tool(&s);
        assert_eq!(m.value, 4000.0);
    }

    #[test]
    fn test_ttft_no_run_initialized() {
        let m = compute_time_to_first_tool(&[]);
        assert_eq!(m.value, 0.0);
        assert_eq!(m.confidence, 0.0);
    }

    // --- T3.7 ---

    #[test]
    fn test_compute_all_returns_all_5_metrics() {
        let m = compute_all(&[], &full_integrity());
        // default values, but all fields populated (no panic)
        let _ = m.doom_loop_frequency;
        let _ = m.llm_efficiency;
        let _ = m.context_waste_ratio;
        let _ = m.hypothesis_churn_rate;
        let _ = m.time_to_first_tool_ms;
    }

    #[test]
    fn test_compute_all_with_empty_steps() {
        let m = compute_all(&[], &full_integrity());
        assert_eq!(m.doom_loop_frequency.value, 0.0);
        assert_eq!(m.doom_loop_frequency.confidence, 0.0);
    }

    #[test]
    fn test_confidence_degraded_by_integrity() {
        let s = vec![
            step(0, 0, "LlmCallStart", None, None, ""),
            step(1, 1, "ToolCallCompleted", Some("t"), Some(StepOutcome::Success), ""),
        ];
        let half = IntegrityReport {
            confidence: 0.5,
            ..Default::default()
        };
        let m = compute_all(&s, &half);
        assert!((m.llm_efficiency.confidence - 0.5).abs() < 1e-9);
    }
}
