"""
Derived / Surrogate Metrics Analysis — Task 3.9 (benchmark-sota-metrics-plan).

Analyzes doom-loop frequency, LLM efficiency, context waste ratio,
hypothesis churn, and time-to-first-tool from the runtime's derived
metrics.
"""

from __future__ import annotations

from .stats_utils import mean, percentile, point_biserial, safe_div


# Thresholds (documented per spec):
#   doom_loop_frequency > 0.1  -> "doom loop detected"
#   hypothesis_churn_rate > 0.5 -> "high churn"
_DOOM_LOOP_THRESHOLD = 0.1
_HIGH_CHURN_THRESHOLD = 0.5


def analyze_derived(results: list[dict]) -> dict:
    """Analyze derived / surrogate metrics across benchmark results.

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with doom_loop, llm_efficiency, context_waste,
        hypothesis_churn, time_to_first_tool, and correlations.
    """
    if not results:
        return _empty_result()

    doom_values: list[float] = []
    efficiency_values: list[float] = []
    waste_values: list[float] = []
    churn_values: list[float] = []
    ttft_values: list[float] = []
    successes: list[bool] = []

    for r in results:
        doom_values.append(float(r.get("doom_loop_frequency", 0) or 0))
        efficiency_values.append(float(r.get("llm_efficiency", 0) or 0))
        waste_values.append(float(r.get("context_waste_ratio", 0) or 0))
        churn_values.append(float(r.get("hypothesis_churn_rate", 0) or 0))
        ttft_values.append(float(r.get("time_to_first_tool_ms", 0) or 0))
        successes.append(bool(r.get("success", False)))

    n = len(results)
    doom_detected = sum(1 for d in doom_values if d > _DOOM_LOOP_THRESHOLD)
    high_churn = sum(1 for c in churn_values if c > _HIGH_CHURN_THRESHOLD)

    return {
        "doom_loop": {
            "mean": round(mean(doom_values), 4),
            "p50": round(percentile(doom_values, 50), 4),
            "p95": round(percentile(doom_values, 95), 4),
            "tasks_with_doom_loop_pct": round(
                safe_div(doom_detected, n) * 100, 2
            ),
        },
        "llm_efficiency": {
            "mean": round(mean(efficiency_values), 4),
            "p50": round(percentile(efficiency_values, 50), 4),
            "min": round(min(efficiency_values), 4) if efficiency_values else 0.0,
        },
        "context_waste": {
            "mean": round(mean(waste_values), 4),
            "p50": round(percentile(waste_values, 50), 4),
            "p95": round(percentile(waste_values, 95), 4),
        },
        "hypothesis_churn": {
            "mean": round(mean(churn_values), 4),
            "tasks_with_high_churn_pct": round(
                safe_div(high_churn, n) * 100, 2
            ),
        },
        "time_to_first_tool": {
            "mean_ms": round(mean(ttft_values), 2),
            "p50_ms": round(percentile(ttft_values, 50), 2),
            "p95_ms": round(percentile(ttft_values, 95), 2),
        },
        "correlations": {
            "doom_loop_vs_success": round(
                point_biserial(successes, doom_values), 4
            ),
            "llm_efficiency_vs_success": round(
                point_biserial(successes, efficiency_values), 4
            ),
            "context_waste_vs_success": round(
                point_biserial(successes, waste_values), 4
            ),
        },
    }


def _empty_result() -> dict:
    return {
        "doom_loop": {
            "mean": 0.0,
            "p50": 0.0,
            "p95": 0.0,
            "tasks_with_doom_loop_pct": 0.0,
        },
        "llm_efficiency": {
            "mean": 0.0,
            "p50": 0.0,
            "min": 0.0,
        },
        "context_waste": {
            "mean": 0.0,
            "p50": 0.0,
            "p95": 0.0,
        },
        "hypothesis_churn": {
            "mean": 0.0,
            "tasks_with_high_churn_pct": 0.0,
        },
        "time_to_first_tool": {
            "mean_ms": 0.0,
            "p50_ms": 0.0,
            "p95_ms": 0.0,
        },
        "correlations": {
            "doom_loop_vs_success": 0.0,
            "llm_efficiency_vs_success": 0.0,
            "context_waste_vs_success": 0.0,
        },
    }
