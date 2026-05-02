"""
Flakiness Analysis — benchmark-sota-metrics-plan.

Detects non-deterministic (flaky) tasks by analyzing pass/fail variance
across multiple runs of the same task.

Flakiness score = 4 * pass_rate * (1 - pass_rate), which peaks at 1.0
when pass_rate = 0.5 (maximum uncertainty) and equals 0.0 when the task
is perfectly deterministic (always passes or always fails).
"""

from __future__ import annotations

from .stats_utils import safe_div

# Minimum number of runs required to compute meaningful flakiness.
_MIN_RUNS = 3

# Tasks with flakiness_score above this threshold are considered flaky.
_FLAKY_THRESHOLD = 0.2


def analyze_flakiness(task_runs: dict[str, list[bool]]) -> dict:
    """Analyze pass/fail determinism across multiple runs per task.

    Args:
        task_runs: maps task_id to a list of pass/fail booleans across
                   repeated runs of the same task.

    Returns:
        Plain dict with per_task flakiness details and a summary.
    """
    if not task_runs:
        return _empty_result()

    per_task: dict[str, dict] = {}
    flaky_count = 0
    deterministic_count = 0
    total_tasks = 0

    for task_id, runs in task_runs.items():
        n = len(runs)
        if n == 0:
            continue

        total_tasks += 1
        passes = sum(1 for r in runs if r)
        pass_rate = safe_div(passes, n)

        # flakiness_score = 4 * p * (1-p), range [0, 1]
        flakiness_score = 4.0 * pass_rate * (1.0 - pass_rate)

        # Only flag as flaky when we have enough data
        is_flaky = flakiness_score > _FLAKY_THRESHOLD and n >= _MIN_RUNS

        per_task[task_id] = {
            "pass_rate": round(pass_rate, 4),
            "n_runs": n,
            "flakiness_score": round(flakiness_score, 4),
            "is_flaky": is_flaky,
        }

        if is_flaky:
            flaky_count += 1
        elif n >= _MIN_RUNS and flakiness_score == 0.0:
            deterministic_count += 1

    # Top-5 most flaky tasks
    ranked = sorted(
        per_task.items(),
        key=lambda kv: kv[1]["flakiness_score"],
        reverse=True,
    )
    most_flaky = [
        (tid, entry["flakiness_score"])
        for tid, entry in ranked[:5]
        if entry["flakiness_score"] > 0
    ]

    # Deterministic pct: tasks with flakiness_score == 0 and enough runs
    eligible = sum(1 for entry in per_task.values() if entry["n_runs"] >= _MIN_RUNS)
    deterministic_pct = safe_div(deterministic_count, eligible) * 100

    return {
        "per_task": per_task,
        "summary": {
            "total_tasks": total_tasks,
            "flaky_tasks": flaky_count,
            "flaky_pct": round(safe_div(flaky_count, total_tasks) * 100, 2),
            "most_flaky": most_flaky,
            "deterministic_tasks_pct": round(deterministic_pct, 2),
        },
    }


def _empty_result() -> dict:
    return {
        "per_task": {},
        "summary": {
            "total_tasks": 0,
            "flaky_tasks": 0,
            "flaky_pct": 0.0,
            "most_flaky": [],
            "deterministic_tasks_pct": 0.0,
        },
    }
