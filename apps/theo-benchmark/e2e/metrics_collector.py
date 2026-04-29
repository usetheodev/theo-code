"""Metrics Collector — aggregates results from e2e-probe, smoke, and SWE-bench.

Produces a consolidated report with pass rates by category, cost totals,
latency percentiles, and failure taxonomy.

Usage:
    python e2e/metrics_collector.py reports/e2e-probe-*.json reports/smoke-*.json
    python e2e/metrics_collector.py --latest  # auto-find latest reports
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

REPORTS_DIR = Path(__file__).resolve().parent.parent / "reports"


def load_results(paths: list[Path]) -> list[dict]:
    """Load benchmark run results from JSON report files."""
    all_results = []
    for path in paths:
        with open(path) as f:
            report = json.load(f)
        results = report.get("results", [])
        all_results.extend(results)
    return all_results


def aggregate_by_category(results: list[dict]) -> dict:
    """Group results by task_category and compute per-category stats."""
    by_cat = defaultdict(list)
    for r in results:
        cat = r.get("task_category", "unknown")
        by_cat[cat].append(r)

    summary = {}
    for cat, runs in sorted(by_cat.items()):
        passed = sum(1 for r in runs if r.get("pass"))
        total = len(runs)
        durations = [r.get("duration_ms", 0) for r in runs]
        costs = [r.get("cost_usd", 0.0) for r in runs]

        summary[cat] = {
            "total": total,
            "passed": passed,
            "pass_rate": round(passed / total, 4) if total > 0 else 0.0,
            "total_cost_usd": round(sum(costs), 4),
            "avg_duration_ms": round(sum(durations) / total) if total > 0 else 0,
            "p95_duration_ms": _percentile(durations, 95),
        }
    return summary


def aggregate_by_suite(results: list[dict]) -> dict:
    """Group results by benchmark_suite."""
    by_suite = defaultdict(list)
    for r in results:
        suite = r.get("benchmark_suite", r.get("metadata", {}).get("suite", "unknown"))
        by_suite[suite].append(r)

    summary = {}
    for suite, runs in sorted(by_suite.items()):
        passed = sum(1 for r in runs if r.get("pass"))
        total = len(runs)
        summary[suite] = {
            "total": total,
            "passed": passed,
            "pass_rate": round(passed / total, 4) if total > 0 else 0.0,
        }
    return summary


def failure_taxonomy(results: list[dict]) -> dict:
    """Count failures by error type."""
    taxonomy = defaultdict(int)
    for r in results:
        if not r.get("pass"):
            err = r.get("error", {})
            err_type = err.get("type", "unknown") if isinstance(err, dict) else "unknown"
            taxonomy[err_type] += 1
    return dict(sorted(taxonomy.items(), key=lambda x: -x[1]))


def consolidate(results: list[dict]) -> dict:
    """Produce a consolidated metrics report."""
    total = len(results)
    passed = sum(1 for r in results if r.get("pass"))
    skipped = sum(1 for r in results if r.get("metadata", {}).get("skipped"))
    executed = total - skipped
    total_cost = sum(r.get("cost_usd", 0.0) for r in results)
    total_tokens = sum(r.get("tokens", {}).get("total", 0) for r in results)

    return {
        "schema": "theo.consolidated-metrics.v1",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "totals": {
            "runs": total,
            "executed": executed,
            "passed": passed,
            "skipped": skipped,
            "pass_rate": round(passed / executed, 4) if executed > 0 else 0.0,
            "total_cost_usd": round(total_cost, 4),
            "total_tokens": total_tokens,
        },
        "by_category": aggregate_by_category(results),
        "by_suite": aggregate_by_suite(results),
        "failures": failure_taxonomy(results),
    }


def to_markdown(report: dict) -> str:
    """Render consolidated report as markdown."""
    lines = [
        "# Consolidated Benchmark Report",
        "",
        f"**Date:** {report['timestamp']}",
        "",
        "## Totals",
        "",
        f"| Metric | Value |",
        f"|--------|-------|",
        f"| Runs | {report['totals']['runs']} |",
        f"| Executed | {report['totals']['executed']} |",
        f"| Passed | {report['totals']['passed']} |",
        f"| Skipped | {report['totals']['skipped']} |",
        f"| Pass Rate | {report['totals']['pass_rate']:.1%} |",
        f"| Total Cost | ${report['totals']['total_cost_usd']:.4f} |",
        f"| Total Tokens | {report['totals']['total_tokens']:,} |",
        "",
        "## By Category",
        "",
        "| Category | Total | Passed | Rate | Avg Duration | Cost |",
        "|----------|-------|--------|------|-------------|------|",
    ]
    for cat, stats in report["by_category"].items():
        lines.append(
            f"| {cat} | {stats['total']} | {stats['passed']} | "
            f"{stats['pass_rate']:.0%} | {stats['avg_duration_ms']}ms | "
            f"${stats['total_cost_usd']:.4f} |"
        )

    if report.get("by_suite"):
        lines.extend([
            "",
            "## By Suite",
            "",
            "| Suite | Total | Passed | Rate |",
            "|-------|-------|--------|------|",
        ])
        for suite, stats in report["by_suite"].items():
            lines.append(
                f"| {suite} | {stats['total']} | {stats['passed']} | {stats['pass_rate']:.0%} |"
            )

    if report.get("failures"):
        lines.extend([
            "",
            "## Failure Taxonomy",
            "",
            "| Error Type | Count |",
            "|------------|-------|",
        ])
        for err_type, count in report["failures"].items():
            lines.append(f"| {err_type} | {count} |")

    return "\n".join(lines) + "\n"


def _percentile(data: list[int | float], pct: int) -> int:
    if not data:
        return 0
    sorted_data = sorted(data)
    idx = int(len(sorted_data) * pct / 100)
    idx = min(idx, len(sorted_data) - 1)
    return int(sorted_data[idx])


def find_latest_reports() -> list[Path]:
    """Find the most recent report from each benchmark suite."""
    latest = {}
    for p in REPORTS_DIR.glob("*.json"):
        # Extract suite from filename (e.g., e2e-probe-123.json -> e2e-probe)
        stem = p.stem
        parts = stem.rsplit("-", 1)
        if len(parts) == 2 and parts[1].isdigit():
            suite = parts[0]
            if suite not in latest or p.stat().st_mtime > latest[suite].stat().st_mtime:
                latest[suite] = p
    return list(latest.values())


def main():
    parser = argparse.ArgumentParser(description="Metrics Collector for Theo Benchmarks")
    parser.add_argument("files", nargs="*", help="Report JSON files to aggregate")
    parser.add_argument("--latest", action="store_true", help="Auto-find latest reports")
    parser.add_argument("--output", type=str, help="Custom output path")
    args = parser.parse_args()

    if args.latest:
        paths = find_latest_reports()
    elif args.files:
        paths = [Path(f) for f in args.files]
    else:
        print("Provide report files or --latest", file=sys.stderr)
        sys.exit(1)

    if not paths:
        print("No report files found.", file=sys.stderr)
        sys.exit(1)

    print(f"Loading {len(paths)} report file(s)...")
    results = load_results(paths)
    print(f"  {len(results)} total run results")

    report = consolidate(results)

    # Save JSON
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    json_path = args.output or str(
        REPORTS_DIR / f"consolidated-{int(time.time())}.json"
    )
    with open(json_path, "w") as f:
        json.dump(report, f, indent=2)

    # Save Markdown
    md_path = json_path.replace(".json", ".md")
    with open(md_path, "w") as f:
        f.write(to_markdown(report))

    print(f"\nReport: {json_path}")
    print(f"Summary: {md_path}")
    print(f"\nPass rate: {report['totals']['pass_rate']:.0%} ({report['totals']['passed']}/{report['totals']['executed']})")


if __name__ == "__main__":
    main()
