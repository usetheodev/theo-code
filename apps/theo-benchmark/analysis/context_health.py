"""
Context Health Analysis — Task 3.1 (benchmark-sota-metrics-plan).

Analyzes context window utilization, growth rate, compaction effectiveness,
and usefulness across benchmark tasks.
"""

from __future__ import annotations

import math
from typing import Any

from .stats_utils import mean, percentile, point_biserial, safe_div


def analyze_context_health(results: list[dict]) -> dict:
    """Analyze context health metrics across benchmark results.

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with summary, distributions, correlations, and alerts.
    """
    if not results:
        return _empty_result()

    # Collect per-task values
    avg_sizes: list[float] = []
    max_sizes: list[float] = []
    growth_rates: list[float] = []
    compaction_counts: list[float] = []
    refetch_rates: list[float] = []
    usefulness_values: list[float] = []
    successes: list[bool] = []

    for r in results:
        avg_sizes.append(float(r.get("context_avg_size_tokens", 0) or 0))
        max_sizes.append(float(r.get("context_max_size_tokens", 0) or 0))
        growth_rates.append(float(r.get("context_growth_rate", 0) or 0))
        compaction_counts.append(float(r.get("context_compaction_count", 0) or 0))
        refetch_rates.append(float(r.get("context_refetch_rate", 0) or 0))
        usefulness_values.append(float(r.get("context_usefulness_avg", 0) or 0))
        successes.append(bool(r.get("success", False)))

    # Alerts: high growth rate (> mean + 2*std)
    alerts: list[dict[str, Any]] = []
    mean_gr = mean(growth_rates)
    if len(growth_rates) >= 2:
        variance = sum((g - mean_gr) ** 2 for g in growth_rates) / len(growth_rates)
        std_gr = math.sqrt(variance)
        threshold = mean_gr + 2 * std_gr
        for i, r in enumerate(results):
            gr = float(r.get("context_growth_rate", 0) or 0)
            if gr > threshold and std_gr > 0:
                alerts.append({
                    "type": "high_growth_rate",
                    "task_index": i,
                    "task_id": r.get("task_id", str(i)),
                    "value": round(gr, 4),
                })

    # Zero usefulness alerts
    for i, r in enumerate(results):
        u = float(r.get("context_usefulness_avg", 0) or 0)
        if u == 0.0 and float(r.get("context_avg_size_tokens", 0) or 0) > 0:
            alerts.append({
                "type": "zero_usefulness",
                "task_index": i,
                "task_id": r.get("task_id", str(i)),
            })

    return {
        "summary": {
            "avg_context_size_tokens": round(mean(avg_sizes), 2),
            "max_context_size_tokens": int(max(max_sizes)) if max_sizes else 0,
            "avg_growth_rate": round(mean(growth_rates), 4),
            "total_compactions": int(sum(compaction_counts)),
            "avg_refetch_rate": round(mean(refetch_rates), 4),
            "avg_usefulness": round(mean(usefulness_values), 4),
        },
        "distributions": {
            "context_size_p50": round(percentile(max_sizes, 50), 2),
            "context_size_p95": round(percentile(max_sizes, 95), 2),
            "growth_rate_p50": round(percentile(growth_rates, 50), 4),
            "growth_rate_p95": round(percentile(growth_rates, 95), 4),
        },
        "correlations": {
            "context_size_vs_success": round(
                point_biserial(successes, max_sizes), 4
            ),
            "compaction_count_vs_success": round(
                point_biserial(successes, compaction_counts), 4
            ),
            "usefulness_vs_success": round(
                point_biserial(successes, usefulness_values), 4
            ),
        },
        "alerts": alerts,
    }


def _empty_result() -> dict:
    return {
        "summary": {
            "avg_context_size_tokens": 0.0,
            "max_context_size_tokens": 0,
            "avg_growth_rate": 0.0,
            "total_compactions": 0,
            "avg_refetch_rate": 0.0,
            "avg_usefulness": 0.0,
        },
        "distributions": {
            "context_size_p50": 0.0,
            "context_size_p95": 0.0,
            "growth_rate_p50": 0.0,
            "growth_rate_p95": 0.0,
        },
        "correlations": {
            "context_size_vs_success": 0.0,
            "compaction_count_vs_success": 0.0,
            "usefulness_vs_success": 0.0,
        },
        "alerts": [],
    }
