"""
Aggregate per-task analyses into a comparison report — Phase 47.

Reads every JSON file in `--report-dir` matching `<benchmark>/<task>.json`
shape, groups by benchmark name (subdirectory), produces a Markdown
report with per-benchmark + cross-benchmark statistics.

Output sections:
  1. Header — date, theo SHA, model, provider
  2. Per-benchmark table — pass rate, $/task, latency p50/p95
  3. Cross-benchmark cost summary
  4. Failure mode taxonomy (top-N modes)
  5. Reproduction commands

Usage:
  python3 analysis/aggregate.py --report-dir reports/2026-04-24T12-00Z \\
                                 --output reports/2026-04-24T12-00Z/comparison.md
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path


def _safe_load_json(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text())
    except Exception:
        return None


def collect_records(report_dir: Path) -> dict[str, list[dict]]:
    """Group analyzed records by benchmark name (= subdirectory name)."""
    out: dict[str, list[dict]] = defaultdict(list)
    for sub in report_dir.iterdir():
        if not sub.is_dir():
            continue
        for f in sub.glob("*.json"):
            if f.name in ("summary.json", "manifest.json"):
                continue
            d = _safe_load_json(f)
            if d:
                out[sub.name].append(d)
    return out


def benchmark_summary(records: list[dict]) -> dict:
    """Compute summary stats for one benchmark's records."""
    if not records:
        return {"task_count": 0}
    total_cost = sum(float(r.get("cost_usd", 0) or 0) for r in records)
    total_tokens = sum(int(r.get("tokens", {}).get("total", 0) or 0) for r in records)
    durations = [int(r.get("duration_ms_wall", 0) or 0) for r in records]
    durations.sort()
    p50 = durations[len(durations) // 2] if durations else 0
    p95 = durations[int(len(durations) * 0.95)] if durations else 0
    success_count = sum(
        1 for r in records
        if r.get("tools", {}).get("success_rate", 0) == 1.0
        or r.get("success") is True  # SWE-bench style
    )
    failure_modes_counter: Counter[str] = Counter()
    for r in records:
        for fm in r.get("failure_modes", []) or []:
            failure_modes_counter[fm] += 1
    return {
        "task_count": len(records),
        "total_cost_usd": round(total_cost, 4),
        "avg_cost_usd": round(total_cost / len(records), 6),
        "total_tokens": total_tokens,
        "avg_tokens": total_tokens // len(records),
        "duration_p50_ms": p50,
        "duration_p95_ms": p95,
        "tool_success_count": success_count,
        "tool_success_rate": round(success_count / len(records), 4),
        "top_failure_modes": failure_modes_counter.most_common(5),
    }


def write_markdown(
    by_bench: dict[str, list[dict]],
    output: Path,
    manifest: dict | None = None,
) -> None:
    """Write the comparison.md report."""
    lines: list[str] = []
    lines.append("# Theo Code — Benchmark Validation Report")
    lines.append("")
    if manifest:
        lines.append("## Manifest")
        for k, v in manifest.items():
            lines.append(f"- **{k}**: `{v}`")
        lines.append("")

    lines.append("## Per-benchmark summary")
    lines.append("")
    lines.append(
        "| Benchmark | Tasks | Tool success | Cost (total) | $/task | tok/task | p50 dur ms | p95 dur ms |"
    )
    lines.append(
        "|---|---:|---:|---:|---:|---:|---:|---:|"
    )
    grand_cost = 0.0
    grand_tasks = 0
    for bench, recs in sorted(by_bench.items()):
        s = benchmark_summary(recs)
        if s["task_count"] == 0:
            continue
        grand_cost += s["total_cost_usd"]
        grand_tasks += s["task_count"]
        lines.append(
            f"| {bench} | {s['task_count']} | "
            f"{s['tool_success_count']}/{s['task_count']} ({s['tool_success_rate']*100:.1f}%) | "
            f"${s['total_cost_usd']:.2f} | ${s['avg_cost_usd']:.4f} | "
            f"{s['avg_tokens']:,} | {s['duration_p50_ms']:,} | {s['duration_p95_ms']:,} |"
        )
    lines.append("")
    lines.append(f"**Grand total**: {grand_tasks} tasks, ${grand_cost:.2f}.")
    lines.append("")

    # Failure mode taxonomy
    all_failures: Counter[str] = Counter()
    for recs in by_bench.values():
        for r in recs:
            for fm in r.get("failure_modes", []) or []:
                all_failures[fm] += 1
    if all_failures:
        lines.append("## Failure mode taxonomy")
        lines.append("")
        lines.append("| Mode | Count | % of tasks |")
        lines.append("|---|---:|---:|")
        for mode, n in all_failures.most_common(10):
            pct = (n / grand_tasks * 100) if grand_tasks else 0.0
            lines.append(f"| {mode} | {n} | {pct:.1f}% |")
        lines.append("")

    output.write_text("\n".join(lines) + "\n")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Aggregate bench reports into comparison.md")
    ap.add_argument("--report-dir", required=True, type=Path)
    ap.add_argument("--output", required=True, type=Path)
    ap.add_argument("--manifest", type=Path, help="Optional manifest.json to embed in header")
    args = ap.parse_args(argv)

    by_bench = collect_records(args.report_dir)
    if not by_bench:
        print(f"[aggregate] WARN: no benchmark records under {args.report_dir}", file=sys.stderr)
    manifest = _safe_load_json(args.manifest) if args.manifest else None
    write_markdown(by_bench, args.output, manifest)
    print(f"[aggregate] wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
