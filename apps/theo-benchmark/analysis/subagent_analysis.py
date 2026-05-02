"""
Subagent Analysis — Task 3.8 (benchmark-sota-metrics-plan).

Analyzes sub-agent delegation: spawn rates, success rates, duration,
and correlation with task success.
"""

from __future__ import annotations

from .stats_utils import mean, percentile, point_biserial, safe_div


def analyze_subagents(results: list[dict]) -> dict:
    """Analyze sub-agent usage and performance across benchmark results.

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with usage, performance, and correlations.
    """
    if not results:
        return _empty_result()

    total_spawned = 0
    total_succeeded = 0
    total_failed = 0
    tasks_using = 0
    durations: list[float] = []
    successes: list[bool] = []
    used_subagent: list[float] = []  # 1.0 if used, 0.0 if not
    subagent_sr: list[float] = []  # per-task subagent success rate

    for r in results:
        spawned = int(r.get("subagent_spawned", 0) or 0)
        succeeded = int(r.get("subagent_succeeded", 0) or 0)
        failed = int(r.get("subagent_failed", 0) or 0)

        total_spawned += spawned
        total_succeeded += succeeded
        total_failed += failed

        if spawned > 0:
            tasks_using += 1
            used_subagent.append(1.0)
            subagent_sr.append(safe_div(succeeded, spawned))
        else:
            used_subagent.append(0.0)
            subagent_sr.append(0.0)

        avg_dur = float(r.get("subagent_avg_duration_ms", 0) or 0)
        if spawned > 0 and avg_dur > 0:
            durations.append(avg_dur)

        successes.append(bool(r.get("success", False)))

    n = len(results)

    return {
        "usage": {
            "total_spawned": total_spawned,
            "total_succeeded": total_succeeded,
            "total_failed": total_failed,
            "overall_success_rate": round(
                safe_div(total_succeeded, total_spawned), 4
            ),
            "tasks_using_subagents_pct": round(
                safe_div(tasks_using, n) * 100, 2
            ),
        },
        "performance": {
            "avg_duration_ms": round(mean(durations), 2),
            "p50_duration_ms": round(percentile(durations, 50), 2),
            "p95_duration_ms": round(percentile(durations, 95), 2),
        },
        "correlations": {
            "subagent_use_vs_task_success": round(
                point_biserial(successes, used_subagent), 4
            ),
            "subagent_success_rate_vs_task_success": round(
                point_biserial(successes, subagent_sr), 4
            ),
        },
    }


def _empty_result() -> dict:
    return {
        "usage": {
            "total_spawned": 0,
            "total_succeeded": 0,
            "total_failed": 0,
            "overall_success_rate": 0.0,
            "tasks_using_subagents_pct": 0.0,
        },
        "performance": {
            "avg_duration_ms": 0.0,
            "p50_duration_ms": 0.0,
            "p95_duration_ms": 0.0,
        },
        "correlations": {
            "subagent_use_vs_task_success": 0.0,
            "subagent_success_rate_vs_task_success": 0.0,
        },
    }
