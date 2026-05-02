"""
Telemetry extraction — Phase 47.

Walks a tb run output dir and produces a tidy CSV + JSON summary
suitable for analysis without ad-hoc scripting.

For each completed trial, extracts:
  - task_id, trial, resolved, failure_mode
  - agent_duration_s, trial_duration_s, test_duration_s
  - input_tokens, output_tokens (from tb's results.json)
  - From the sidecar `theo-headless.json` (Phase 47 hook):
      iterations, llm_calls, retries, model, duration_ms_internal
      tools_total, tools_success, files_edited count, summary
      cost_usd (computed from pricing.toml at extract time)
  - stderr_tail (last 20 lines) — for failure diagnosis

Output:
  <output-dir>/telemetry.csv      tidy per-trial rows
  <output-dir>/telemetry.json     same data as JSON list
  <output-dir>/failure_taxonomy.json   counts grouped by failure_mode
  <output-dir>/cost_summary.json   total cost + per-task percentiles

Usage:
  python3 runner/extract_telemetry.py \\
    --raw-dir reports/<date>/tbench-core/raw \\
    --output-dir reports/<date>/tbench-core/telemetry
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from pricing import compute_cost  # noqa: E402


def _safe_dt(s: str | None) -> datetime | None:
    if not s:
        return None
    try:
        return datetime.fromisoformat(s)
    except Exception:
        return None


def _delta_s(start: str | None, end: str | None) -> float:
    a = _safe_dt(start)
    b = _safe_dt(end)
    if a and b:
        return round((b - a).total_seconds(), 2)
    return 0.0


def _tail(path: Path, n: int = 20) -> str:
    """Last N non-empty lines of a file (text)."""
    if not path.exists():
        return ""
    try:
        lines = path.read_text(errors="replace").splitlines()
    except Exception:
        return ""
    nonblank = [ln for ln in lines if ln.strip()]
    return "\n".join(nonblank[-n:])


def _strip_ansi(s: str) -> str:
    """Strip ANSI escape codes from terminal text for cleaner storage."""
    import re
    return re.sub(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])", "", s)


def extract_trial(trial_dir: Path) -> dict:
    """Return one tidy dict per trial directory."""
    results_path = trial_dir / "results.json"
    if not results_path.exists():
        return {}
    try:
        r = json.loads(results_path.read_text())
    except Exception:
        return {}

    # Sidecar from Phase 47 perform_task hook
    sidecar_path = trial_dir / "sessions" / "theo-headless.json"
    if not sidecar_path.exists():
        # Some tb versions put sidecars next to results.json directly
        sidecar_path = trial_dir / "theo-headless.json"
    sidecar = {}
    if sidecar_path.exists():
        try:
            sidecar = json.loads(sidecar_path.read_text())
        except Exception:
            pass

    # Stderr tail for diagnosis
    stderr_path = trial_dir / "sessions" / "theo-stderr.log"
    if not stderr_path.exists():
        stderr_path = trial_dir / "theo-stderr.log"
    stderr_tail_text = _strip_ansi(_tail(stderr_path, 20))

    # Tokens — prefer sidecar (truth from theo) over tb's (often zero)
    tokens = sidecar.get("tokens", {}) or {}
    input_tok = int(tokens.get("input", 0) or r.get("total_input_tokens", 0) or 0)
    output_tok = int(tokens.get("output", 0) or r.get("total_output_tokens", 0) or 0)
    total_tok = int(tokens.get("total", 0) or (input_tok + output_tok))

    model = sidecar.get("model", "")
    cost_usd = compute_cost(input_tok, output_tok, model) if model else 0.0

    tools = sidecar.get("tools", {}) or {}
    llm = sidecar.get("llm", {}) or {}

    return {
        "task_id": r.get("task_id", "?"),
        "trial_name": r.get("trial_name", "?"),
        "resolved": bool(r.get("is_resolved")),
        "failure_mode": r.get("failure_mode", "unknown") or "unknown",
        "agent_duration_s": _delta_s(r.get("agent_started_at"), r.get("agent_ended_at")),
        "test_duration_s": _delta_s(r.get("test_started_at"), r.get("test_ended_at")),
        "trial_duration_s": _delta_s(r.get("trial_started_at"), r.get("trial_ended_at")),
        "input_tokens": input_tok,
        "output_tokens": output_tok,
        "total_tokens": total_tok,
        "model": model,
        "cost_usd": round(cost_usd, 6),
        "iterations": int(sidecar.get("iterations", 0) or 0),
        "llm_calls": int(llm.get("calls", 0) or 0),
        "retries": int(llm.get("retries", 0) or 0),
        "tools_total": int(tools.get("total", 0) or 0),
        "tools_success": int(tools.get("success", 0) or 0),
        "files_edited": len(sidecar.get("files_edited", []) or []),
        "agent_summary": (sidecar.get("summary", "") or "")[:200],
        "stderr_tail": stderr_tail_text[:1000],
        "has_sidecar": bool(sidecar),
        "has_stderr": stderr_path.exists(),
    }


def discover_trials(raw_dir: Path) -> list[Path]:
    """Yield all per-trial directories under raw_dir."""
    out: list[Path] = []
    for results in raw_dir.rglob("results.json"):
        # Skip the master aggregate at the run level (no per-trial siblings)
        if results.parent.parent == raw_dir:
            continue
        out.append(results.parent)
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--raw-dir", required=True, type=Path,
                    help="tb run --output-path target (root)")
    ap.add_argument("--output-dir", required=True, type=Path)
    args = ap.parse_args(argv)

    args.output_dir.mkdir(parents=True, exist_ok=True)
    trials = discover_trials(args.raw_dir)
    rows = [extract_trial(d) for d in trials]
    rows = [r for r in rows if r]  # drop empties

    # CSV
    csv_path = args.output_dir / "telemetry.csv"
    if rows:
        with csv_path.open("w", newline="") as fp:
            w = csv.DictWriter(fp, fieldnames=list(rows[0].keys()))
            w.writeheader()
            for r in rows:
                w.writerow(r)
    else:
        csv_path.write_text("")

    # JSON
    (args.output_dir / "telemetry.json").write_text(json.dumps(rows, indent=2))

    # Failure taxonomy
    taxonomy_counter: Counter[str] = Counter()
    by_mode: dict[str, list[str]] = defaultdict(list)
    for r in rows:
        m = r["failure_mode"]
        taxonomy_counter[m] += 1
        by_mode[m].append(r["task_id"])
    (args.output_dir / "failure_taxonomy.json").write_text(json.dumps({
        "counts": dict(taxonomy_counter),
        "tasks_by_mode": {k: sorted(v) for k, v in by_mode.items()},
    }, indent=2))

    # Cost summary
    total_cost = sum(r["cost_usd"] for r in rows)
    total_tokens = sum(r["total_tokens"] for r in rows)
    durations = sorted(r["agent_duration_s"] for r in rows)
    n = len(rows)
    if n:
        p50 = durations[n // 2]
        p95 = durations[int(n * 0.95)] if n > 1 else durations[0]
    else:
        p50 = p95 = 0.0

    cost_summary = {
        "trial_count": n,
        "resolved_count": sum(1 for r in rows if r["resolved"]),
        "pass_rate": round(sum(1 for r in rows if r["resolved"]) / n, 4) if n else 0.0,
        "total_cost_usd": round(total_cost, 4),
        "avg_cost_per_trial_usd": round(total_cost / n, 6) if n else 0.0,
        "total_tokens": total_tokens,
        "avg_tokens_per_trial": total_tokens // n if n else 0,
        "agent_duration_p50_s": p50,
        "agent_duration_p95_s": p95,
        "agent_duration_total_s": round(sum(durations), 2),
        "trials_with_sidecar_pct": round(
            sum(1 for r in rows if r["has_sidecar"]) * 100 / n, 1
        ) if n else 0.0,
    }
    (args.output_dir / "cost_summary.json").write_text(
        json.dumps(cost_summary, indent=2)
    )

    print(f"[extract] {n} trials processed")
    print(f"  resolved={cost_summary['resolved_count']}/{n} "
          f"({cost_summary['pass_rate']*100:.1f}%)")
    print(f"  cost=${cost_summary['total_cost_usd']:.4f} "
          f"(avg ${cost_summary['avg_cost_per_trial_usd']:.4f}/trial)")
    print(f"  tokens={cost_summary['total_tokens']:,}")
    print(f"  agent_duration p50={cost_summary['agent_duration_p50_s']}s "
          f"p95={cost_summary['agent_duration_p95_s']}s")
    print(f"  sidecar coverage={cost_summary['trials_with_sidecar_pct']}%")
    print(f"  failure modes: {dict(taxonomy_counter.most_common())}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
