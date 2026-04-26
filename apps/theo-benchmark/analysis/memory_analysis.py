"""
Memory & Learning Analysis — Task 3.4 (benchmark-sota-metrics-plan).

Analyzes episodic memory usage, hypothesis lifecycle, constraint learning,
and failure fingerprint recurrence across benchmark tasks.
"""

from __future__ import annotations

from .stats_utils import mean, point_biserial, safe_div


def analyze_memory(results: list[dict]) -> dict:
    """Analyze memory and learning metrics across benchmark results.

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with episodes, hypotheses, learning, and correlations.
    """
    if not results:
        return _empty_result()

    episodes_injected: list[float] = []
    episodes_created: list[float] = []
    hyp_formed: list[float] = []
    hyp_invalidated: list[float] = []
    hyp_active: list[float] = []
    constraints: list[float] = []
    fp_new: list[float] = []
    fp_recurrent: list[float] = []
    successes: list[bool] = []

    for r in results:
        episodes_injected.append(float(r.get("memory_episodes_injected", 0) or 0))
        episodes_created.append(float(r.get("memory_episodes_created", 0) or 0))
        hyp_formed.append(float(r.get("memory_hypotheses_formed", 0) or 0))
        hyp_invalidated.append(float(r.get("memory_hypotheses_invalidated", 0) or 0))
        hyp_active.append(float(r.get("memory_hypotheses_active", 0) or 0))
        constraints.append(float(r.get("memory_constraints_learned", 0) or 0))
        fp_new.append(float(r.get("memory_failure_fingerprints_new", 0) or 0))
        fp_recurrent.append(float(r.get("memory_failure_fingerprints_recurrent", 0) or 0))
        successes.append(bool(r.get("success", False)))

    n = len(results)
    total_formed = sum(hyp_formed)
    total_invalidated = sum(hyp_invalidated)
    total_fp_new = sum(fp_new)
    total_fp_recurrent = sum(fp_recurrent)

    tasks_using_memory = sum(1 for ei in episodes_injected if ei > 0)
    tasks_forming_hyp = sum(1 for hf in hyp_formed if hf > 0)

    return {
        "episodes": {
            "total_injected": int(sum(episodes_injected)),
            "total_created": int(sum(episodes_created)),
            "avg_injected_per_task": round(mean(episodes_injected), 2),
            "tasks_using_memory_pct": round(safe_div(tasks_using_memory, n) * 100, 2),
        },
        "hypotheses": {
            "total_formed": int(total_formed),
            "total_invalidated": int(total_invalidated),
            "avg_active_per_task": round(mean(hyp_active), 2),
            "churn_rate": round(safe_div(total_invalidated, total_formed), 4),
            "tasks_forming_hypotheses_pct": round(
                safe_div(tasks_forming_hyp, n) * 100, 2
            ),
        },
        "learning": {
            "total_constraints_learned": int(sum(constraints)),
            "total_failure_fingerprints_new": int(total_fp_new),
            "total_failure_fingerprints_recurrent": int(total_fp_recurrent),
            "recurrence_rate": round(
                safe_div(total_fp_recurrent, total_fp_new + total_fp_recurrent), 4
            ),
        },
        "correlations": {
            "episodes_injected_vs_success": round(
                point_biserial(successes, episodes_injected), 4
            ),
            "hypotheses_formed_vs_success": round(
                point_biserial(successes, hyp_formed), 4
            ),
            "constraints_learned_vs_success": round(
                point_biserial(successes, constraints), 4
            ),
        },
    }


def _empty_result() -> dict:
    return {
        "episodes": {
            "total_injected": 0,
            "total_created": 0,
            "avg_injected_per_task": 0.0,
            "tasks_using_memory_pct": 0.0,
        },
        "hypotheses": {
            "total_formed": 0,
            "total_invalidated": 0,
            "avg_active_per_task": 0.0,
            "churn_rate": 0.0,
            "tasks_forming_hypotheses_pct": 0.0,
        },
        "learning": {
            "total_constraints_learned": 0,
            "total_failure_fingerprints_new": 0,
            "total_failure_fingerprints_recurrent": 0,
            "recurrence_rate": 0.0,
        },
        "correlations": {
            "episodes_injected_vs_success": 0.0,
            "hypotheses_formed_vs_success": 0.0,
            "constraints_learned_vs_success": 0.0,
        },
    }
