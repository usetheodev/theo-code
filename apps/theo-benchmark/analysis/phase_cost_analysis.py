"""
Phase Cost Analysis — benchmark-sota-metrics-plan.

Estimates cost per agent phase (Explore, Edit, Verify, Done) by
proportionally distributing total tokens across phases based on their
iteration counts from loop_metrics.phase_distribution.
"""

from __future__ import annotations

from .stats_utils import safe_div


def analyze_phase_cost(results: list[dict]) -> dict:
    """Analyze estimated cost distribution across agent phases.

    Uses phase_distribution from loop_metrics and total tokens to
    estimate token/cost allocation per phase.

    Estimation: tokens_per_phase = (phase_iterations / total_iterations) * total_tokens

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with per_phase breakdown, summary, and caveat.
    """
    if not results:
        return _empty_result()

    # Aggregate phase iterations across all tasks
    phase_totals: dict[str, int] = {}
    grand_iterations = 0
    grand_tokens = 0

    for r in results:
        phase_dist = r.get("phase_distribution") or {}
        tokens_total = int(r.get("tokens_input", 0) or 0) + int(
            r.get("tokens_output", 0) or 0
        )
        grand_tokens += tokens_total

        task_iterations = 0
        for phase_name, phase_data in phase_dist.items():
            if isinstance(phase_data, dict):
                iters = int(phase_data.get("iterations", 0) or 0)
            else:
                iters = int(phase_data or 0)
            phase_totals[phase_name] = phase_totals.get(phase_name, 0) + iters
            task_iterations += iters

        grand_iterations += task_iterations

    # Build per-phase breakdown
    per_phase: dict[str, dict] = {}
    for phase_name, iters in sorted(phase_totals.items()):
        pct_of_time = safe_div(iters, grand_iterations)
        estimated_tokens = int(pct_of_time * grand_tokens)
        per_phase[phase_name] = {
            "iterations": iters,
            "pct_of_time": round(pct_of_time * 100, 2),
            "estimated_tokens": estimated_tokens,
            "pct_of_cost": round(pct_of_time * 100, 2),
        }

    # Summary
    most_expensive = ""
    most_time_consuming = ""
    max_tokens = 0
    max_iters = 0
    planning_iters = 0
    execution_iters = 0

    planning_phases = {"Explore", "Planning", "Analyze"}
    execution_phases = {"Edit", "Verify", "Execute", "Done"}

    for phase_name, data in per_phase.items():
        if data["estimated_tokens"] > max_tokens:
            max_tokens = data["estimated_tokens"]
            most_expensive = phase_name
        if data["iterations"] > max_iters:
            max_iters = data["iterations"]
            most_time_consuming = phase_name

        if phase_name in planning_phases:
            planning_iters += data["iterations"]
        elif phase_name in execution_phases:
            execution_iters += data["iterations"]

    planning_to_execution_ratio = safe_div(planning_iters, execution_iters)

    return {
        "per_phase": per_phase,
        "summary": {
            "most_expensive_phase": most_expensive,
            "most_time_consuming_phase": most_time_consuming,
            "planning_to_execution_ratio": round(planning_to_execution_ratio, 4),
        },
        "caveat": "Cost per phase is estimated proportionally from iterations.",
    }


def _empty_result() -> dict:
    return {
        "per_phase": {},
        "summary": {
            "most_expensive_phase": "",
            "most_time_consuming_phase": "",
            "planning_to_execution_ratio": 0.0,
        },
        "caveat": "Cost per phase is estimated proportionally from iterations.",
    }
