"""
Agent Loop Analysis — Task 3.3 (benchmark-sota-metrics-plan).

Analyzes convergence patterns, budget utilization, phase distribution,
and evolution behavior across benchmark tasks.
"""

from __future__ import annotations

from typing import Any

from .stats_utils import mean, percentile, point_biserial, safe_div


def analyze_loop(results: list[dict]) -> dict:
    """Analyze agent loop behavior across benchmark results.

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with convergence, budget_utilization, phase_distribution,
        evolution, correlations, and alerts.
    """
    if not results:
        return _empty_result()

    conv_rates: list[float] = []
    iterations: list[float] = []
    budget_iter_pcts: list[float] = []
    budget_token_pcts: list[float] = []
    budget_time_pcts: list[float] = []
    successes: list[bool] = []

    # Phase accumulators: {phase_name: [pct_values]}
    phase_pcts: dict[str, list[float]] = {}

    evolution_attempts_total = 0
    evolution_success_count = 0

    alerts: list[dict[str, Any]] = []

    for i, r in enumerate(results):
        cr = float(r.get("convergence_rate", 0) or 0)
        conv_rates.append(cr)
        iterations.append(float(r.get("iterations", 0) or 0))
        successes.append(bool(r.get("success", False)))

        bi = float(r.get("budget_utilization_iterations_pct", 0) or 0)
        bt = float(r.get("budget_utilization_tokens_pct", 0) or 0)
        btime = float(r.get("budget_utilization_time_pct", 0) or 0)
        budget_iter_pcts.append(bi)
        budget_token_pcts.append(bt)
        budget_time_pcts.append(btime)

        # Budget-bound alert
        if bi >= 0.95:
            alerts.append({
                "type": "budget_bound",
                "task_index": i,
                "task_id": r.get("task_id", str(i)),
                "iterations_pct": round(bi, 4),
            })

        # Zero convergence alert
        if cr == 0.0:
            alerts.append({
                "type": "zero_convergence",
                "task_index": i,
                "task_id": r.get("task_id", str(i)),
            })

        # Phase distribution
        pd = r.get("phase_distribution", {}) or {}
        for phase_name, phase_data in pd.items():
            if phase_name not in phase_pcts:
                phase_pcts[phase_name] = []
            pct = 0.0
            if isinstance(phase_data, dict):
                pct = float(phase_data.get("pct", 0) or 0)
            else:
                pct = float(phase_data or 0)
            phase_pcts[phase_name].append(pct)

        # Evolution
        ea = int(r.get("evolution_attempts", 0) or 0)
        evolution_attempts_total += ea

    # Converged = convergence_rate > 0
    tasks_converged = sum(1 for cr in conv_rates if cr > 0)

    # Budget-bound tasks
    tasks_budget_bound = sum(1 for bi in budget_iter_pcts if bi >= 0.95)

    # Iterations to converge: only for successful tasks
    converged_iters = [
        it for it, s in zip(iterations, successes) if s
    ]

    # Phase distribution averages
    planning_avg = mean(phase_pcts.get("planning", []))
    executing_avg = mean(phase_pcts.get("executing", []))
    evaluating_avg = mean(phase_pcts.get("evaluating", []))
    other_phases = {}
    for pname, pvals in phase_pcts.items():
        if pname not in ("planning", "executing", "evaluating"):
            other_phases[pname] = round(mean(pvals), 4)

    n = len(results)

    return {
        "convergence": {
            "avg_convergence_rate": round(mean(conv_rates), 4),
            "tasks_converged_pct": round(safe_div(tasks_converged, n) * 100, 2),
            "tasks_budget_bound_pct": round(safe_div(tasks_budget_bound, n) * 100, 2),
            "avg_iterations_to_converge": round(mean(converged_iters), 2),
            "median_iterations": round(percentile(iterations, 50), 2),
        },
        "budget_utilization": {
            "avg_iterations_pct": round(mean(budget_iter_pcts), 4),
            "avg_tokens_pct": round(mean(budget_token_pcts), 4),
            "avg_time_pct": round(mean(budget_time_pcts), 4),
            "tasks_hitting_iter_limit_pct": round(
                safe_div(sum(1 for b in budget_iter_pcts if b >= 0.95), n) * 100, 2
            ),
            "tasks_hitting_token_limit_pct": round(
                safe_div(sum(1 for b in budget_token_pcts if b >= 0.95), n) * 100, 2
            ),
            "tasks_hitting_time_limit_pct": round(
                safe_div(sum(1 for b in budget_time_pcts if b >= 0.95), n) * 100, 2
            ),
        },
        "phase_distribution": {
            "planning_avg_pct": round(planning_avg, 4),
            "executing_avg_pct": round(executing_avg, 4),
            "evaluating_avg_pct": round(evaluating_avg, 4),
            "other_phases": other_phases,
        },
        "evolution": {
            "total_evolution_attempts": evolution_attempts_total,
            "evolution_success_rate": 0.0,  # No per-attempt success tracking yet
        },
        "correlations": {
            "iterations_vs_success": round(
                point_biserial(successes, iterations), 4
            ),
            "budget_util_vs_success": round(
                point_biserial(successes, budget_iter_pcts), 4
            ),
            "convergence_rate_vs_success": round(
                point_biserial(successes, conv_rates), 4
            ),
        },
        "alerts": alerts,
    }


def _empty_result() -> dict:
    return {
        "convergence": {
            "avg_convergence_rate": 0.0,
            "tasks_converged_pct": 0.0,
            "tasks_budget_bound_pct": 0.0,
            "avg_iterations_to_converge": 0.0,
            "median_iterations": 0.0,
        },
        "budget_utilization": {
            "avg_iterations_pct": 0.0,
            "avg_tokens_pct": 0.0,
            "avg_time_pct": 0.0,
            "tasks_hitting_iter_limit_pct": 0.0,
            "tasks_hitting_token_limit_pct": 0.0,
            "tasks_hitting_time_limit_pct": 0.0,
        },
        "phase_distribution": {
            "planning_avg_pct": 0.0,
            "executing_avg_pct": 0.0,
            "evaluating_avg_pct": 0.0,
            "other_phases": {},
        },
        "evolution": {
            "total_evolution_attempts": 0,
            "evolution_success_rate": 0.0,
        },
        "correlations": {
            "iterations_vs_success": 0.0,
            "budget_util_vs_success": 0.0,
            "convergence_rate_vs_success": 0.0,
        },
        "alerts": [],
    }
