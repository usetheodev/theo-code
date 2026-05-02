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

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))


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
    summary = {
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

    # SOTA-enriched metrics (Phase 64) — extracted from v4 fields when present
    try:
        sota_fields = [
            "context_waste_ratio", "convergence_rate", "doom_loop_frequency",
            "cache_hit_rate", "time_to_first_tool_ms", "context_avg_size_tokens",
            "context_growth_rate", "llm_efficiency",
        ]
        for fld in sota_fields:
            vals = [float(r.get(fld, 0) or 0) for r in records if r.get(fld) is not None]
            if vals:
                sorted_vals = sorted(vals)
                n_vals = len(sorted_vals)
                summary[f"sota_{fld}_mean"] = round(sum(vals) / n_vals, 4)
                summary[f"sota_{fld}_p50"] = round(
                    sorted_vals[n_vals // 2], 4
                )
    except Exception:
        pass  # SOTA enrichment must not break existing flow

    return summary


def write_markdown(
    by_bench: dict[str, list[dict]],
    output: Path,
    manifest: dict | None = None,
    report_dir: Path | None = None,
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

    # SOTA cross-benchmark comparison (Phase 64)
    try:
        sota_keys = [
            "sota_context_waste_ratio_mean", "sota_convergence_rate_mean",
            "sota_doom_loop_frequency_mean", "sota_cache_hit_rate_mean",
            "sota_time_to_first_tool_ms_mean", "sota_llm_efficiency_mean",
        ]
        # Check if any benchmark has SOTA data
        any_sota = False
        bench_summaries = {}
        for bench, recs in sorted(by_bench.items()):
            s = benchmark_summary(recs)
            bench_summaries[bench] = s
            if any(s.get(k) is not None for k in sota_keys):
                any_sota = True

        if any_sota:
            lines.append("## SOTA Metrics (cross-benchmark)")
            lines.append("")
            header_labels = [
                ("ctx_waste", "sota_context_waste_ratio_mean"),
                ("conv_rate", "sota_convergence_rate_mean"),
                ("doom_loop", "sota_doom_loop_frequency_mean"),
                ("cache_hit", "sota_cache_hit_rate_mean"),
                ("ttft_ms", "sota_time_to_first_tool_ms_mean"),
                ("llm_eff", "sota_llm_efficiency_mean"),
            ]
            header = "| Benchmark | " + " | ".join(h[0] for h in header_labels) + " |"
            sep = "|---|" + "---:|" * len(header_labels)
            lines.append(header)
            lines.append(sep)
            for bench in sorted(bench_summaries):
                s = bench_summaries[bench]
                cells = []
                for _, key in header_labels:
                    v = s.get(key)
                    cells.append(f"{v:.4f}" if v is not None else "—")
                lines.append(f"| {bench} | " + " | ".join(cells) + " |")
            lines.append("")

        # Also try to include per-benchmark SOTA reports if available
        for bench in sorted(by_bench):
            sota_path = report_dir / bench / "sota_report.json"
            if sota_path.exists():
                try:
                    from analysis.report_builder import report_to_markdown
                    sota_data = json.loads(sota_path.read_text())
                    lines.append(f"## SOTA Detail: {bench}")
                    lines.append("")
                    lines.append(f"Pass rate: {sota_data.get('pass_rate', 0)*100:.1f}% "
                                 f"(CI: {sota_data.get('ci_95', [0,0])})")
                    lines.append(f"Tasks: {sota_data.get('n_tasks', 0)}")
                    lines.append("")
                except Exception:
                    pass
    except Exception:
        pass  # SOTA section must not break existing report

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
    write_markdown(by_bench, args.output, manifest, report_dir=args.report_dir)
    print(f"[aggregate] wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
