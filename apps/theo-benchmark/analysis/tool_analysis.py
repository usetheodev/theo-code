"""
Tool Breakdown Analysis — Task 3.2 (benchmark-sota-metrics-plan).

Analyzes per-tool performance: call counts, success rates, latency
distributions, and correlations with task success.
"""

from __future__ import annotations

from collections import defaultdict

from .stats_utils import mean, percentile, point_biserial, safe_div


def analyze_tools(results: list[dict]) -> dict:
    """Analyze tool usage and performance across benchmark results.

    Args:
        results: list of dicts (from dataclasses.asdict(HeadlessResult)).

    Returns:
        Plain dict with per_tool breakdown, summary, and correlations.
    """
    if not results:
        return _empty_result()

    # Accumulate per-tool stats across all tasks
    tool_calls: dict[str, int] = defaultdict(int)
    tool_successes: dict[str, int] = defaultdict(int)
    tool_failures: dict[str, int] = defaultdict(int)
    tool_latencies: dict[str, list[float]] = defaultdict(list)
    tool_max_latencies: dict[str, float] = defaultdict(float)

    for r in results:
        breakdown = r.get("tool_breakdown", []) or []
        for tb in breakdown:
            name = tb.get("tool_name", "unknown")
            calls = int(tb.get("call_count", 0) or 0)
            succ = int(tb.get("success_count", 0) or 0)
            fail = int(tb.get("failure_count", 0) or 0)
            avg_lat = float(tb.get("avg_latency_ms", 0) or 0)
            max_lat = float(tb.get("max_latency_ms", 0) or 0)

            tool_calls[name] += calls
            tool_successes[name] += succ
            tool_failures[name] += fail
            # Record average latency per task for percentile computation
            if calls > 0:
                tool_latencies[name].append(avg_lat)
            if max_lat > tool_max_latencies[name]:
                tool_max_latencies[name] = max_lat

    total_calls_all = sum(tool_calls.values())
    total_latency_all = sum(
        mean(lats) * tool_calls[name]
        for name, lats in tool_latencies.items()
    )

    # Build per-tool output
    per_tool: dict = {}
    for name in sorted(tool_calls.keys()):
        calls = tool_calls[name]
        succ = tool_successes[name]
        fail = tool_failures[name]
        lats = tool_latencies.get(name, [])
        avg_lat = mean(lats)
        tool_total_lat = avg_lat * calls

        per_tool[name] = {
            "total_calls": calls,
            "success_rate": round(safe_div(succ, calls), 4),
            "avg_latency_ms": round(avg_lat, 2),
            "max_latency_ms": round(tool_max_latencies.get(name, 0), 2),
            "p50_latency_ms": round(percentile(lats, 50), 2),
            "p95_latency_ms": round(percentile(lats, 95), 2),
            "pct_of_total_calls": round(safe_div(calls, total_calls_all) * 100, 2),
            "pct_of_total_latency": round(
                safe_div(tool_total_lat, total_latency_all) * 100, 2
            ),
        }

    # Summary
    slowest = max(per_tool, key=lambda t: per_tool[t]["avg_latency_ms"]) if per_tool else ""
    most_used = max(per_tool, key=lambda t: per_tool[t]["total_calls"]) if per_tool else ""
    most_failing = ""
    if per_tool:
        failing_tools = {
            name: tool_failures[name] for name in per_tool if tool_failures[name] > 0
        }
        if failing_tools:
            most_failing = max(failing_tools, key=failing_tools.get)  # type: ignore[arg-type]

    # Correlations
    successes = [bool(r.get("success", False)) for r in results]

    # Tool diversity: number of unique tools used per task
    diversity = []
    for r in results:
        bd = r.get("tool_breakdown", []) or []
        diversity.append(float(len(bd)))

    # Bash calls per task
    bash_calls = []
    for r in results:
        bd = r.get("tool_breakdown", []) or []
        bc = sum(
            int(tb.get("call_count", 0) or 0)
            for tb in bd
            if tb.get("tool_name", "") == "bash"
        )
        bash_calls.append(float(bc))

    # Edit success rate per task
    edit_sr = []
    for r in results:
        bd = r.get("tool_breakdown", []) or []
        for tb in bd:
            if tb.get("tool_name", "") == "edit":
                edit_sr.append(float(tb.get("success_rate", 0) or 0))
                break
        else:
            edit_sr.append(0.0)

    return {
        "per_tool": per_tool,
        "summary": {
            "total_tools": len(per_tool),
            "total_calls": total_calls_all,
            "overall_success_rate": round(
                safe_div(
                    sum(tool_successes.values()),
                    total_calls_all,
                ),
                4,
            ),
            "slowest_tool": slowest,
            "most_used_tool": most_used,
            "most_failing_tool": most_failing,
        },
        "correlations": {
            "tool_diversity_vs_success": round(
                point_biserial(successes, diversity), 4
            ),
            "bash_calls_vs_success": round(
                point_biserial(successes, bash_calls), 4
            ),
            "edit_success_rate_vs_task_success": round(
                point_biserial(successes, edit_sr), 4
            ),
        },
    }


def _empty_result() -> dict:
    return {
        "per_tool": {},
        "summary": {
            "total_tools": 0,
            "total_calls": 0,
            "overall_success_rate": 0.0,
            "slowest_tool": "",
            "most_used_tool": "",
            "most_failing_tool": "",
        },
        "correlations": {
            "tool_diversity_vs_success": 0.0,
            "bash_calls_vs_success": 0.0,
            "edit_success_rate_vs_task_success": 0.0,
        },
    }
