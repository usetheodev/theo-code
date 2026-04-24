"""
Post-process Terminal-Bench tb run output into per-task analytic JSON
records consumable by aggregate.py.

`tb run --output-path <dir>` writes:
  <dir>/runs/<run_uuid>/results.json    (aggregate)
  <dir>/runs/<run_uuid>/<task_id>/...   (per-task artifacts: agent stdout,
                                          test logs, etc.)

This module reads the structure, extracts the headless JSON line from
each task's agent stdout, applies pricing.compute_cost, and writes
<output-dir>/<task_id>.json — one file per task, in the schema
analyze_run() emits.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from pricing import compute_cost  # noqa: E402


def _walk_tasks(raw_dir: Path):
    """Yield (task_id, task_dir) for every task in tb output."""
    runs_dir = raw_dir / "runs"
    if not runs_dir.exists():
        # Some tb versions write results directly under raw_dir
        runs_dir = raw_dir
    for run_uuid_dir in runs_dir.iterdir():
        if not run_uuid_dir.is_dir():
            continue
        for task_dir in run_uuid_dir.iterdir():
            if not task_dir.is_dir():
                continue
            yield task_dir.name, task_dir


def _extract_headless_from_stdout(task_dir: Path) -> dict:
    """Find the agent stdout file and parse the theo.headless.v* JSON line."""
    candidates = [
        task_dir / "agent.stdout",
        task_dir / "agent.log",
        task_dir / "stdout.log",
    ]
    candidates += list(task_dir.glob("*.stdout")) + list(task_dir.glob("logs/*.log"))
    for p in candidates:
        if not p.exists() or not p.is_file():
            continue
        try:
            txt = p.read_text(errors="replace")
        except Exception:
            continue
        for line in reversed(txt.splitlines()):
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                data = json.loads(line)
            except json.JSONDecodeError:
                continue
            if str(data.get("schema", "")).startswith("theo.headless"):
                return data
    return {}


def _extract_pass_status(task_dir: Path) -> bool | None:
    """Read tb's per-task verdict (test result). None if undetermined."""
    for name in ("results.json", "test_results.json", "task.json"):
        p = task_dir / name
        if not p.exists():
            continue
        try:
            d = json.loads(p.read_text())
        except Exception:
            continue
        # tb 0.2 schema variants
        if "passed" in d:
            return bool(d["passed"])
        if "success" in d:
            return bool(d["success"])
        if "test_result" in d:
            return d["test_result"] in ("PASS", "passed", True)
    return None


def analyze_task(task_id: str, task_dir: Path) -> dict:
    """Build an analytic record for one tb task."""
    summary = _extract_headless_from_stdout(task_dir)
    passed = _extract_pass_status(task_dir)
    tokens = summary.get("tokens", {}) or {}
    tools = summary.get("tools", {}) or {}
    llm = summary.get("llm", {}) or {}
    model = summary.get("model", "")
    cost = compute_cost(
        int(tokens.get("input", 0) or 0),
        int(tokens.get("output", 0) or 0),
        model,
    )
    return {
        "task_id": task_id,
        "passed": passed,
        "model": model,
        "provider": summary.get("provider", ""),
        "tokens": {
            "input": int(tokens.get("input", 0) or 0),
            "output": int(tokens.get("output", 0) or 0),
            "total": int(tokens.get("total", 0) or 0),
        },
        "cost_usd": round(cost, 6),
        "iterations": int(summary.get("iterations", 0) or 0),
        "llm_calls": int(llm.get("calls", 0) or 0),
        "retries": int(llm.get("retries", 0) or 0),
        "tools": {
            "total": int(tools.get("total", 0) or 0),
            "success": int(tools.get("success", 0) or 0),
            "success_rate": (
                round(int(tools.get("success", 0) or 0)
                      / int(tools.get("total", 1) or 1), 4)
                if int(tools.get("total", 0) or 0) else 0.0
            ),
        },
        "duration_ms_wall": int(summary.get("duration_ms", 0) or 0),
        "failure_modes": summary.get("failure_modes", []) or [],
        "success": passed,  # alias for aggregate.benchmark_summary
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Process tb run output into per-task records")
    ap.add_argument("--raw-dir", required=True, type=Path,
                    help="tb run --output-path target")
    ap.add_argument("--output-dir", required=True, type=Path,
                    help="Where per-task .json files go")
    ap.add_argument("--bench-name", default="tbench",
                    help="Used for summary header only")
    args = ap.parse_args(argv)

    args.output_dir.mkdir(parents=True, exist_ok=True)
    n = 0
    passed = 0
    total_cost = 0.0
    for task_id, task_dir in _walk_tasks(args.raw_dir):
        rec = analyze_task(task_id, task_dir)
        out_path = args.output_dir / f"{task_id}.json"
        out_path.write_text(json.dumps(rec, indent=2))
        n += 1
        if rec.get("passed"):
            passed += 1
        total_cost += rec.get("cost_usd", 0.0)

    summary_path = args.output_dir / "summary.json"
    summary = {
        "bench": args.bench_name,
        "tasks": n,
        "passed": passed,
        "pass_rate": round(passed / n, 4) if n else 0.0,
        "total_cost_usd": round(total_cost, 4),
    }
    summary_path.write_text(json.dumps(summary, indent=2))
    print(f"[{args.bench_name}-post] {n} tasks, {passed} passed ({summary['pass_rate']*100:.1f}%), "
          f"${total_cost:.2f} total cost")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
