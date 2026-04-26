#!/usr/bin/env python3
"""
Deep analysis of collected bench data — Phase 47 + datadriven decisions.

Reads .theo/bench-data/<date>/ produced by collect-everything.sh and
emits:
  - findings.md      — high-level patterns + concrete bugs/inefficiencies
  - per_trial.csv    — wide table for spreadsheet analysis
  - distributions.txt — histograms of iters, tools, duration, cost, tokens
  - failure_clusters.json — trials grouped by failure root-cause

Key questions answered:
  1. What % of failures are iter-limit vs early-give-up vs tool-error?
  2. Distribution of iterations actually used (does max_iter binding everywhere?)
  3. Most expensive tasks (long-tail)?
  4. Per-tool success/failure breakdown
  5. Time-to-first-action latency from monitor snapshots
  6. Are there tasks that consistently fail in similar ways?

Usage:
  python3 scripts/bench/analyze-deep.py <date>
"""

from __future__ import annotations

import csv
import json
import re
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path
from datetime import datetime


def _strip_ansi(s: str) -> str:
    return re.sub(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])", "", s)


def parse_iso(s: str) -> datetime | None:
    if not s:
        return None
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))
    except Exception:
        return None


def percentile(values, pct):
    if not values:
        return 0
    s = sorted(values)
    k = (pct / 100) * (len(s) - 1)
    f = int(k)
    c = min(f + 1, len(s) - 1)
    return s[f] if f == c else s[f] + (s[c] - s[f]) * (k - f)


def histogram(values, bins=10, label=""):
    if not values:
        return f"{label}: no data"
    s = sorted(values)
    lo, hi = s[0], s[-1]
    if lo == hi:
        return f"{label}: all values == {lo}"
    width = (hi - lo) / bins
    buckets = [0] * bins
    for v in values:
        idx = min(int((v - lo) / width), bins - 1)
        buckets[idx] += 1
    out = [f"{label} (n={len(values)}, min={lo}, max={hi}):"]
    max_count = max(buckets)
    for i, c in enumerate(buckets):
        bar = "#" * int(c * 40 / max_count) if max_count else ""
        rng = f"{lo + i*width:.1f}-{lo + (i+1)*width:.1f}"
        out.append(f"  {rng:<20} {c:>3} {bar}")
    return "\n".join(out)


def collect_trials(reports_dir: Path) -> list[dict]:
    """Walk reports/raw/ and load (results.json, theo-headless.json, agent.log) per trial."""
    trials = []
    for results_path in reports_dir.rglob("results.json"):
        # Skip master aggregates
        if results_path.parent.name.startswith("2026-"):
            continue
        try:
            res = json.loads(results_path.read_text())
        except Exception:
            continue
        trial_dir = results_path.parent
        sidecar = {}
        sidecar_path = trial_dir / "agent-logs" / "theo-headless.json"
        if sidecar_path.exists():
            try:
                sidecar = json.loads(sidecar_path.read_text())
            except Exception:
                pass

        # Read agent.log for raw reasoning context
        agent_log = ""
        for p in [trial_dir / "sessions" / "agent.log", trial_dir / "agent-logs" / "theo-stdout.log"]:
            if p.exists():
                try:
                    agent_log = _strip_ansi(p.read_text(errors="replace"))
                    break
                except Exception:
                    pass

        # Stderr tail
        stderr = ""
        for p in [trial_dir / "agent-logs" / "theo-stderr.log"]:
            if p.exists():
                try:
                    stderr = _strip_ansi(p.read_text(errors="replace"))
                    break
                except Exception:
                    pass

        # tests log
        tests = ""
        tests_path = trial_dir / "sessions" / "tests.log"
        if tests_path.exists():
            try:
                tests = _strip_ansi(tests_path.read_text(errors="replace"))
            except Exception:
                pass

        # Classify failure root cause from sidecar summary
        summary = (sidecar.get("summary") or "").lower()
        if "budget exceeded" in summary or "iterations exceeded" in summary:
            root_cause = "iter_limit"
        elif "blocked" in summary or "can't truthfully" in summary or "unable to" in summary:
            root_cause = "early_giveup"
        elif res.get("is_resolved"):
            root_cause = "ok"
        elif res.get("failure_mode") in ("agent_timeout", "test_timeout"):
            root_cause = "timeout"
        elif res.get("failure_mode") == "agent_installation_failed":
            root_cause = "install_fail"
        elif sidecar:
            root_cause = "tests_disagree"  # theo says success but tests disagree
        else:
            root_cause = "no_sidecar"

        # Compute durations
        ad = 0.0
        a_start = parse_iso(res.get("agent_started_at"))
        a_end = parse_iso(res.get("agent_ended_at"))
        if a_start and a_end:
            ad = (a_end - a_start).total_seconds()

        # Per-tool breakdown — parse agent log for theo's tool calls.
        # The headless schema has only tools.{total,success}; per-tool
        # breakdown requires sniffing the actual log lines.
        tool_call_pattern = re.findall(r'"name"\s*:\s*"([a-z_]+)"', agent_log)
        per_tool = Counter(tool_call_pattern)

        trials.append({
            "task": res.get("task_id", "?"),
            "trial_name": res.get("trial_name", "?"),
            "resolved": bool(res.get("is_resolved")),
            "tb_failure_mode": res.get("failure_mode") or "unset",
            "root_cause": root_cause,
            "iterations": int(sidecar.get("iterations", 0) or 0),
            "llm_calls": int((sidecar.get("llm") or {}).get("calls", 0) or 0),
            "llm_retries": int((sidecar.get("llm") or {}).get("retries", 0) or 0),
            "tools_total": int((sidecar.get("tools") or {}).get("total", 0) or 0),
            "tools_success": int((sidecar.get("tools") or {}).get("success", 0) or 0),
            "tokens_input": int((sidecar.get("tokens") or {}).get("input", 0) or 0),
            "tokens_output": int((sidecar.get("tokens") or {}).get("output", 0) or 0),
            "tokens_total": int((sidecar.get("tokens") or {}).get("total", 0) or 0),
            "cost_usd": float(sidecar.get("cost_usd", 0) or 0),
            "agent_duration_s": ad,
            "internal_duration_ms": int(sidecar.get("duration_ms", 0) or 0),
            "summary": (sidecar.get("summary") or "")[:400],
            "stderr_tail": stderr[-500:],
            "files_edited_count": len(sidecar.get("files_edited", []) or []),
            "model": sidecar.get("model", ""),
            "agent_log_size": len(agent_log),
            "per_tool_counts": dict(per_tool),
            "tests_log_tail": tests[-300:],
        })
    return trials


def write_csv(trials: list[dict], out: Path) -> None:
    if not trials:
        out.write_text("")
        return
    fields = [k for k in trials[0].keys() if k not in ("per_tool_counts", "stderr_tail", "tests_log_tail", "summary")]
    with out.open("w", newline="") as fp:
        w = csv.DictWriter(fp, fieldnames=fields)
        w.writeheader()
        for t in trials:
            w.writerow({k: t[k] for k in fields})


def write_distributions(trials: list[dict], out: Path) -> None:
    completed = [t for t in trials if t["iterations"] > 0]
    out.write_text("\n\n".join([
        f"=== Distributions (n={len(trials)}, with-sidecar={len(completed)}) ===",
        histogram([t["iterations"] for t in completed], 10, "iterations"),
        histogram([t["llm_calls"] for t in completed], 10, "llm_calls"),
        histogram([t["tools_total"] for t in completed], 10, "tools_total"),
        histogram([t["agent_duration_s"] for t in completed], 10, "agent_duration_s"),
        histogram([t["cost_usd"] for t in completed], 10, "cost_usd"),
        histogram([t["tokens_total"] for t in completed], 10, "tokens_total"),
    ]) + "\n")


def write_findings(trials: list[dict], monitor_file: Path | None,
                   collector_log: Path | None, out: Path) -> None:
    n = len(trials)
    completed = [t for t in trials if t["iterations"] > 0]
    resolved = [t for t in trials if t["resolved"]]
    by_cause: dict[str, list[dict]] = defaultdict(list)
    for t in trials:
        by_cause[t["root_cause"]].append(t)

    total_cost = sum(t["cost_usd"] for t in completed)
    total_tokens = sum(t["tokens_total"] for t in completed)
    total_llm = sum(t["llm_calls"] for t in completed)
    total_tools = sum(t["tools_total"] for t in completed)

    iter_dist = sorted([t["iterations"] for t in completed])
    iters_at_limit = sum(1 for t in completed if t["iterations"] >= 20)
    pct_at_limit = (iters_at_limit / len(completed) * 100) if completed else 0

    # Per-tool aggregate
    global_tool_counts: Counter[str] = Counter()
    for t in completed:
        for tool, n_calls in t.get("per_tool_counts", {}).items():
            global_tool_counts[tool] += n_calls

    # Monitor — completion rate over time
    monitor_summary = "(no monitor data)"
    if monitor_file and monitor_file.exists():
        snaps = []
        for line in monitor_file.read_text().splitlines():
            if line.strip():
                try:
                    snaps.append(json.loads(line))
                except Exception:
                    pass
        if len(snaps) >= 2:
            t0 = parse_iso(snaps[0]["ts"])
            tN = parse_iso(snaps[-1]["ts"])
            elapsed = (tN - t0).total_seconds() / 60
            comp = snaps[-1]["results"].get("completed_count", 0)
            rate = comp / elapsed if elapsed else 0
            avg_containers = statistics.mean(len(s.get("containers", [])) for s in snaps)
            max_containers = max(len(s.get("containers", [])) for s in snaps)
            avg_load = statistics.mean(
                s.get("host", {}).get("loadavg", [0])[0] for s in snaps
            )
            monitor_summary = (
                f"- Snapshots: {len(snaps)} over {elapsed:.1f} min\n"
                f"- Completion rate: {rate:.2f} tasks/min\n"
                f"- Container parallelism: avg={avg_containers:.1f}, max={max_containers}\n"
                f"- Host load1 avg: {avg_load:.2f} on {snaps[0].get('host', {}).get('mem_total_mb', 0)//1024}GB host"
            )

    # OTLP collector check
    otlp_status = "(no collector log)"
    if collector_log and collector_log.exists():
        log = collector_log.read_text()
        accepted = log.count("Accepted spans")
        if accepted == 0:
            otlp_status = "**NO SPANS RECEIVED** — collector log shows zero exports. Theo's `--features otel` build may not be active or `OTLP_ENDPOINT` env not reaching the container."
        else:
            otlp_status = f"Collector received spans (count={accepted})"

    # Cost outliers
    cost_sorted = sorted(completed, key=lambda x: x["cost_usd"], reverse=True)
    expensive = cost_sorted[:5]
    cheap = sorted(completed, key=lambda x: x["cost_usd"])[:5]

    # Iterations efficiency
    iter_avg = statistics.mean(t["iterations"] for t in completed) if completed else 0
    llm_avg = statistics.mean(t["llm_calls"] for t in completed) if completed else 0
    tools_avg = statistics.mean(t["tools_total"] for t in completed) if completed else 0

    # Tools ratio
    tool_success = sum(t["tools_success"] for t in completed)
    tool_total = sum(t["tools_total"] for t in completed)
    tool_rate = (tool_success / tool_total * 100) if tool_total else 0

    text = f"""# Bench Findings — Deep Analysis

## Overview

- **Trials processed**: {n}
- **With sidecar (theo telemetry captured)**: {len(completed)}
- **Resolved**: {len(resolved)}/{n} = **{len(resolved)/n*100:.1f}%**
- **Total cost**: ${total_cost:.2f}
- **Total tokens**: {total_tokens:,}
- **Total LLM calls**: {total_llm}
- **Total tool calls**: {total_tools}
- **Avg iterations/trial**: {iter_avg:.1f}
- **Avg LLM calls/trial**: {llm_avg:.1f}
- **Avg tool calls/trial**: {tools_avg:.1f}
- **Tool dispatch success rate**: {tool_rate:.1f}% ({tool_success}/{tool_total})

## Failure root cause taxonomy

| Root cause | Count | % | Notes |
|---|---:|---:|---|
"""
    for cause, items in sorted(by_cause.items(), key=lambda x: -len(x[1])):
        pct = len(items) / n * 100
        text += f"| `{cause}` | {len(items)} | {pct:.1f}% | "
        if cause == "iter_limit":
            text += "Theo hit max-iter (20). Most tasks stop here."
        elif cause == "early_giveup":
            text += "Theo declared task impossible before exhausting iters. **Investigate theo prompt**."
        elif cause == "tests_disagree":
            text += "Theo says success but tb tests fail. **Investigate verification gap**."
        elif cause == "ok":
            text += "Resolved (passed tests)"
        elif cause == "no_sidecar":
            text += "Sidecar capture failed — check perform_task instrumentation"
        text += " |\n"

    text += f"""
## Iterations distribution

- Min: {iter_dist[0] if iter_dist else 0}
- p50: {iter_dist[len(iter_dist)//2] if iter_dist else 0}
- p95: {iter_dist[int(len(iter_dist)*0.95)] if iter_dist else 0}
- Max: {iter_dist[-1] if iter_dist else 0}
- **At limit (>=20)**: {iters_at_limit}/{len(completed)} = **{pct_at_limit:.1f}%**

If pct_at_limit > 60%, max_iter=20 is the binding constraint for most failures.

## Tool usage patterns

Top 15 tools called across all trials:

| Tool | Total calls | Avg per trial |
|---|---:|---:|
"""
    for tool, count in global_tool_counts.most_common(15):
        avg = count / len(completed) if completed else 0
        text += f"| `{tool}` | {count} | {avg:.1f} |\n"

    text += f"""
## Cost outliers

### Top 5 most expensive trials

| Task | Iters | Tools | Tokens | Cost |
|---|---:|---:|---:|---:|
"""
    for t in expensive:
        text += f"| {t['task']} | {t['iterations']} | {t['tools_total']} | {t['tokens_total']:,} | ${t['cost_usd']:.4f} |\n"

    text += "\n### Top 5 cheapest trials\n\n| Task | Iters | Tools | Tokens | Cost |\n|---|---:|---:|---:|---:|\n"
    for t in cheap:
        text += f"| {t['task']} | {t['iterations']} | {t['tools_total']} | {t['tokens_total']:,} | ${t['cost_usd']:.4f} |\n"

    text += f"""
## Resolved tasks (success cases)

"""
    if resolved:
        text += "| Task | Iters | Tools | Tokens | Cost |\n|---|---:|---:|---:|---:|\n"
        for t in resolved:
            text += f"| {t['task']} | {t['iterations']} | {t['tools_total']} | {t['tokens_total']:,} | ${t['cost_usd']:.4f} |\n"
    else:
        text += "**None.** Every trial completed without resolving.\n"

    text += f"""
## Early give-up cases (theo declared impossible)

"""
    for t in by_cause.get("early_giveup", [])[:10]:
        text += f"### {t['task']}\n\n- iterations: {t['iterations']}/{20}\n- summary: {t['summary'][:200]}\n\n"

    text += f"""
## Monitor / system observations

{monitor_summary}

## OTLP / observability pipeline

{otlp_status}

## Concrete bugs / inefficiencies identified

(see findings + agent.log review for hypotheses to act on)
"""
    out.write_text(text)


def write_failure_clusters(trials: list[dict], out: Path) -> None:
    """Group trials by failure root_cause + by exact summary prefix."""
    clusters: dict[str, list[dict]] = defaultdict(list)
    for t in trials:
        key = t["root_cause"]
        clusters[key].append({
            "task": t["task"],
            "iterations": t["iterations"],
            "tools_total": t["tools_total"],
            "summary": t["summary"][:300],
        })
    out.write_text(json.dumps(clusters, indent=2))


def main():
    if len(sys.argv) < 2:
        date = Path(".theo/secrets/current-bench-date").read_text().strip()
    else:
        date = sys.argv[1]
    base = Path(f".theo/bench-data/{date}")
    if not base.exists():
        print(f"no data at {base} — run collect-everything.sh first")
        return 1

    print(f"[analyze] reading {base}")
    trials = collect_trials(base / "reports")
    print(f"[analyze] loaded {len(trials)} trials")

    out = base / "analysis"
    out.mkdir(exist_ok=True)
    write_csv(trials, out / "per_trial.csv")
    write_distributions(trials, out / "distributions.txt")
    monitor = base / "reports" / "tbench-core" / "monitor.jsonl"
    collector = base / "docker-logs" / "collector.log"
    write_findings(trials, monitor, collector, out / "findings.md")
    write_failure_clusters(trials, out / "failure_clusters.json")

    print(f"[analyze] wrote:")
    for p in out.iterdir():
        print(f"  {p} ({p.stat().st_size} bytes)")


if __name__ == "__main__":
    sys.exit(main() or 0)
