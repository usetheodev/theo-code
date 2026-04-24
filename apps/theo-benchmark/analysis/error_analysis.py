"""
Error Taxonomy Analysis — Task 3.5 (benchmark-sota-metrics-plan).

Analyzes error categories, cost-of-failure, and failure mode breakdown
across benchmark tasks.
"""

from __future__ import annotations

from collections import Counter

from .stats_utils import mean, safe_div


_ERROR_CATEGORIES = [
    "network",
    "llm",
    "tool",
    "sandbox",
    "budget",
    "validation",
]


def analyze_errors(results: list[dict]) -> dict:
    """Analyze error taxonomy and cost-of-failure across benchmark results.

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with taxonomy, summary, failure_modes, and cost_of_failure.
    """
    if not results:
        return _empty_result()

    # Accumulate error counts per category
    category_counts: dict[str, int] = {c: 0 for c in _ERROR_CATEGORIES}
    total_errors = 0

    # Cost tracking
    passed_costs: list[float] = []
    failed_costs: list[float] = []
    total_cost = 0.0

    # Failure modes
    all_failure_modes: Counter[str] = Counter()
    tasks_with_failure_modes = 0

    tasks_with_errors = 0

    for r in results:
        cost = float(r.get("cost_usd", 0) or 0)
        total_cost += cost
        success = bool(r.get("success", False))

        if success:
            passed_costs.append(cost)
        else:
            failed_costs.append(cost)

        # Error counts per category
        task_errors = 0
        for cat in _ERROR_CATEGORIES:
            count = int(r.get(f"error_{cat}", 0) or 0)
            category_counts[cat] += count
            task_errors += count

        et = int(r.get("error_total", 0) or 0)
        # Use explicit error_total if available, else sum of categories
        if et > 0:
            total_errors += et
            tasks_with_errors += 1
        elif task_errors > 0:
            total_errors += task_errors
            tasks_with_errors += 1

        # Failure modes (list of strings or dict)
        fm = r.get("failure_modes", [])
        if isinstance(fm, dict):
            modes = [k for k, v in fm.items() if v]
        elif isinstance(fm, list):
            modes = fm
        else:
            modes = []
        if modes:
            tasks_with_failure_modes += 1
            all_failure_modes.update(modes)

    n = len(results)

    # Build taxonomy with percentages and cost attribution
    # Cost per error category is estimated proportionally
    taxonomy: dict = {}
    for cat in _ERROR_CATEGORIES:
        count = category_counts[cat]
        pct = safe_div(count, total_errors) * 100 if total_errors > 0 else 0.0
        # Proportional cost: (category_errors / total_errors) * total_failed_cost
        failed_total = sum(failed_costs)
        cat_cost = safe_div(count, total_errors) * failed_total if total_errors > 0 else 0.0
        taxonomy[cat] = {
            "count": count,
            "pct": round(pct, 2),
            "cost_usd": round(cat_cost, 6),
        }

    # Most expensive / most frequent
    most_expensive = ""
    most_frequent = ""
    if any(taxonomy[c]["count"] > 0 for c in _ERROR_CATEGORIES):
        most_expensive = max(
            _ERROR_CATEGORIES,
            key=lambda c: taxonomy[c]["cost_usd"],
        )
        most_frequent = max(
            _ERROR_CATEGORIES,
            key=lambda c: taxonomy[c]["count"],
        )

    # Failure modes
    top_5 = all_failure_modes.most_common(5)

    wasted_cost = sum(failed_costs)

    return {
        "taxonomy": taxonomy,
        "summary": {
            "total_errors": total_errors,
            "total_error_cost_usd": round(wasted_cost, 6),
            "error_rate": round(safe_div(tasks_with_errors, n), 4),
            "most_expensive_category": most_expensive,
            "most_frequent_category": most_frequent,
        },
        "failure_modes": {
            "modes": dict(all_failure_modes),
            "top_5": top_5,
            "tasks_with_failure_modes_pct": round(
                safe_div(tasks_with_failure_modes, n) * 100, 2
            ),
        },
        "cost_of_failure": {
            "avg_cost_failed_task_usd": round(mean(failed_costs), 6),
            "avg_cost_passed_task_usd": round(mean(passed_costs), 6),
            "wasted_cost_usd": round(wasted_cost, 6),
            "wasted_pct": round(safe_div(wasted_cost, total_cost) * 100, 2),
        },
    }


def _empty_result() -> dict:
    taxonomy = {
        cat: {"count": 0, "pct": 0.0, "cost_usd": 0.0}
        for cat in _ERROR_CATEGORIES
    }
    return {
        "taxonomy": taxonomy,
        "summary": {
            "total_errors": 0,
            "total_error_cost_usd": 0.0,
            "error_rate": 0.0,
            "most_expensive_category": "",
            "most_frequent_category": "",
        },
        "failure_modes": {
            "modes": {},
            "top_5": [],
            "tasks_with_failure_modes_pct": 0.0,
        },
        "cost_of_failure": {
            "avg_cost_failed_task_usd": 0.0,
            "avg_cost_passed_task_usd": 0.0,
            "wasted_cost_usd": 0.0,
            "wasted_pct": 0.0,
        },
    }
