"""
Cost Efficiency Analysis — Task 3.6 (benchmark-sota-metrics-plan).

Analyzes cost per pass, token efficiency, cache utilization, cost
breakdown by token type, wasted spend, and marginal cost curve.
"""

from __future__ import annotations

from .stats_utils import mean, percentile, safe_div


def analyze_cost(results: list[dict]) -> dict:
    """Analyze cost efficiency across benchmark results.

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with per_task, efficiency, breakdown, wasted, and
        marginal_cost_curve.
    """
    if not results:
        return _empty_result()

    costs: list[float] = []
    passed_costs: list[float] = []
    failed_costs: list[float] = []
    passed_tokens: list[int] = []
    failed_tokens: list[int] = []
    total_iterations = 0
    passed_count = 0

    input_tokens_total = 0
    output_tokens_total = 0
    cache_read_total = 0
    reasoning_total = 0
    tokens_total = 0

    cache_hit_rates: list[float] = []
    tpse_values: list[float] = []  # tokens_per_successful_edit

    for r in results:
        cost = float(r.get("cost_usd", 0) or 0)
        costs.append(cost)
        success = bool(r.get("success", False))
        iters = int(r.get("iterations", 0) or 0)
        total_iterations += iters

        tok_in = int(r.get("tokens_input", 0) or 0)
        tok_out = int(r.get("tokens_output", 0) or 0)
        tok_total = int(r.get("tokens_total", 0) or 0)
        cache_read = int(r.get("cache_read_tokens", 0) or 0)
        reasoning = int(r.get("reasoning_tokens", 0) or 0)

        input_tokens_total += tok_in
        output_tokens_total += tok_out
        cache_read_total += cache_read
        reasoning_total += reasoning
        tokens_total += tok_total

        chr_val = float(r.get("cache_hit_rate", 0) or 0)
        cache_hit_rates.append(chr_val)

        tpse = float(r.get("tokens_per_successful_edit", 0) or 0)
        if tpse > 0:
            tpse_values.append(tpse)

        if success:
            passed_count += 1
            passed_costs.append(cost)
            passed_tokens.append(tok_total)
        else:
            failed_costs.append(cost)
            failed_tokens.append(tok_total)

    total_cost = sum(costs)
    total_passed_tokens = sum(passed_tokens)
    total_failed_tokens = sum(failed_tokens)

    # Marginal cost curve: sort tasks by cost ASC, accumulate
    indexed = [(costs[i], bool(results[i].get("success", False))) for i in range(len(results))]
    indexed.sort(key=lambda x: x[0])

    marginal_curve: list[dict] = []
    cumulative_cost = 0.0
    cumulative_pass = 0
    for j, (c, s) in enumerate(indexed):
        cumulative_cost += c
        if s:
            cumulative_pass += 1
        pass_rate_pct = round(safe_div(cumulative_pass, j + 1) * 100, 2)
        marginal_curve.append({
            "pass_rate_pct": pass_rate_pct,
            "cumulative_cost_usd": round(cumulative_cost, 6),
        })

    # Token breakdown percentages (relative to tokens_total)
    grand_tokens = tokens_total if tokens_total > 0 else 1

    return {
        "per_task": {
            "avg_cost_usd": round(mean(costs), 6),
            "median_cost_usd": round(percentile(costs, 50), 6),
            "p95_cost_usd": round(percentile(costs, 95), 6),
            "max_cost_usd": round(max(costs), 6) if costs else 0.0,
            "min_cost_usd": round(min(costs), 6) if costs else 0.0,
        },
        "efficiency": {
            "cost_per_pass_usd": round(safe_div(total_cost, max(1, passed_count)), 6),
            "cost_per_iteration_usd": round(
                safe_div(total_cost, max(1, total_iterations)), 6
            ),
            "tokens_per_pass": round(
                safe_div(total_passed_tokens, max(1, passed_count)), 2
            ),
            "tokens_per_iteration": round(
                safe_div(tokens_total, max(1, total_iterations)), 2
            ),
            "cache_hit_rate_avg": round(mean(cache_hit_rates), 4),
            "tokens_per_successful_edit_avg": round(mean(tpse_values), 2),
        },
        "breakdown": {
            "input_tokens_pct": round(
                safe_div(input_tokens_total, grand_tokens) * 100, 2
            ),
            "output_tokens_pct": round(
                safe_div(output_tokens_total, grand_tokens) * 100, 2
            ),
            "cache_read_tokens_pct": round(
                safe_div(cache_read_total, grand_tokens) * 100, 2
            ),
            "reasoning_tokens_pct": round(
                safe_div(reasoning_total, grand_tokens) * 100, 2
            ),
        },
        "wasted": {
            "failed_task_cost_usd": round(sum(failed_costs), 6),
            "failed_task_tokens": total_failed_tokens,
            "wasted_pct_of_total_cost": round(
                safe_div(sum(failed_costs), total_cost) * 100, 2
            ),
            "wasted_pct_of_total_tokens": round(
                safe_div(total_failed_tokens, tokens_total) * 100, 2
            ),
        },
        "marginal_cost_curve": marginal_curve,
    }


def _empty_result() -> dict:
    return {
        "per_task": {
            "avg_cost_usd": 0.0,
            "median_cost_usd": 0.0,
            "p95_cost_usd": 0.0,
            "max_cost_usd": 0.0,
            "min_cost_usd": 0.0,
        },
        "efficiency": {
            "cost_per_pass_usd": 0.0,
            "cost_per_iteration_usd": 0.0,
            "tokens_per_pass": 0.0,
            "tokens_per_iteration": 0.0,
            "cache_hit_rate_avg": 0.0,
            "tokens_per_successful_edit_avg": 0.0,
        },
        "breakdown": {
            "input_tokens_pct": 0.0,
            "output_tokens_pct": 0.0,
            "cache_read_tokens_pct": 0.0,
            "reasoning_tokens_pct": 0.0,
        },
        "wasted": {
            "failed_task_cost_usd": 0.0,
            "failed_task_tokens": 0,
            "wasted_pct_of_total_cost": 0.0,
            "wasted_pct_of_total_tokens": 0.0,
        },
        "marginal_cost_curve": [],
    }
