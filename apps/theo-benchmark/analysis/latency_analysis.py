"""
Latency Analysis — Task 3.7 (benchmark-sota-metrics-plan).

Analyzes wall-clock duration, first-action latency, per-tool latency
distributions, and estimated time breakdown (tool / LLM / overhead).
"""

from __future__ import annotations

from collections import defaultdict

from .stats_utils import mean, percentile, point_biserial, safe_div


def analyze_latency(results: list[dict]) -> dict:
    """Analyze latency distributions across benchmark results.

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with wall_clock, first_action, per_tool_latency,
        time_breakdown, and correlations.
    """
    if not results:
        return _empty_result()

    durations: list[float] = []
    first_tool_times: list[float] = []
    successes: list[bool] = []

    # Per-tool latency accumulators
    tool_latencies: dict[str, list[float]] = defaultdict(list)
    # For time breakdown estimation
    total_tool_time_est: list[float] = []  # per task

    for r in results:
        dur = float(r.get("duration_ms", 0) or 0)
        durations.append(dur)
        successes.append(bool(r.get("success", False)))

        ttft = float(r.get("time_to_first_tool_ms", 0) or 0)
        first_tool_times.append(ttft)

        # Estimate tool time from breakdown
        breakdown = r.get("tool_breakdown", []) or []
        task_tool_time = 0.0
        for tb in breakdown:
            name = tb.get("tool_name", "unknown")
            avg_lat = float(tb.get("avg_latency_ms", 0) or 0)
            calls = int(tb.get("call_count", 0) or 0)
            tool_latencies[name].append(avg_lat)
            task_tool_time += avg_lat * calls

        total_tool_time_est.append(task_tool_time)

    # Wall clock stats
    wall_clock = {
        "p50_ms": round(percentile(durations, 50), 2),
        "p95_ms": round(percentile(durations, 95), 2),
        "p99_ms": round(percentile(durations, 99), 2),
        "mean_ms": round(mean(durations), 2),
        "max_ms": round(max(durations), 2) if durations else 0.0,
    }

    # First action stats
    first_action = {
        "p50_ms": round(percentile(first_tool_times, 50), 2),
        "p95_ms": round(percentile(first_tool_times, 95), 2),
        "mean_ms": round(mean(first_tool_times), 2),
    }

    # Per-tool latency
    per_tool_latency: dict = {}
    for name in sorted(tool_latencies.keys()):
        lats = tool_latencies[name]
        per_tool_latency[name] = {
            "p50_ms": round(percentile(lats, 50), 2),
            "p95_ms": round(percentile(lats, 95), 2),
            "mean_ms": round(mean(lats), 2),
        }

    # Time breakdown estimation
    sum_wall = sum(durations)
    sum_tool = sum(total_tool_time_est)
    # LLM time estimated as: wall - tool (rough, since we lack direct LLM spans)
    sum_llm_est = max(0, sum_wall - sum_tool)
    overhead_est = 0.0  # With only 2-component split, overhead is 0

    tool_time_pct = safe_div(sum_tool, sum_wall) * 100
    llm_time_pct = safe_div(sum_llm_est, sum_wall) * 100
    overhead_time_pct = max(0.0, 100.0 - tool_time_pct - llm_time_pct)

    return {
        "wall_clock": wall_clock,
        "first_action": first_action,
        "per_tool_latency": per_tool_latency,
        "time_breakdown": {
            "tool_time_pct": round(tool_time_pct, 2),
            "llm_time_pct": round(llm_time_pct, 2),
            "overhead_time_pct": round(overhead_time_pct, 2),
        },
        "correlations": {
            "duration_vs_success": round(
                point_biserial(successes, durations), 4
            ),
            "first_action_latency_vs_success": round(
                point_biserial(successes, first_tool_times), 4
            ),
        },
    }


def _empty_result() -> dict:
    return {
        "wall_clock": {
            "p50_ms": 0.0,
            "p95_ms": 0.0,
            "p99_ms": 0.0,
            "mean_ms": 0.0,
            "max_ms": 0.0,
        },
        "first_action": {
            "p50_ms": 0.0,
            "p95_ms": 0.0,
            "mean_ms": 0.0,
        },
        "per_tool_latency": {},
        "time_breakdown": {
            "tool_time_pct": 0.0,
            "llm_time_pct": 0.0,
            "overhead_time_pct": 0.0,
        },
        "correlations": {
            "duration_vs_success": 0.0,
            "first_action_latency_vs_success": 0.0,
        },
    }
