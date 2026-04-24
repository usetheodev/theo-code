"""
Post-process SWE-bench Lite output (swe/adapter.py) → per-instance records
for the aggregator.

`swe/adapter.py --report patches.json` emits a JSON list of patch attempts.
`swe/adapter.py --grade --report graded.json` adds resolved/unresolved
verdicts after running the official Docker grader.

This module merges both files (graded preferred when present) and writes
one .json per instance into <output-dir>.
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


def _load_list(path: Path | None) -> list[dict]:
    if not path or not path.exists():
        return []
    raw = json.loads(path.read_text())
    if isinstance(raw, list):
        return raw
    if isinstance(raw, dict) and "results" in raw:
        return raw["results"]
    return []


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="SWE-bench post-processing")
    ap.add_argument("--report", required=True, type=Path,
                    help="patches.json from swe/adapter.py")
    ap.add_argument("--graded", type=Path,
                    help="graded.json (optional) — adds resolved/unresolved")
    ap.add_argument("--output-dir", required=True, type=Path)
    args = ap.parse_args(argv)

    args.output_dir.mkdir(parents=True, exist_ok=True)
    patches = _load_list(args.report)
    graded = {g.get("instance_id"): g for g in _load_list(args.graded)}

    n = 0
    resolved = 0
    total_cost = 0.0
    for p in patches:
        iid = p.get("instance_id", "")
        if not iid:
            continue
        g = graded.get(iid, {})
        is_resolved = bool(g.get("resolved", False))
        tokens = p.get("tokens", {}) or {}
        model = p.get("model", "")
        cost = compute_cost(
            int(tokens.get("input", 0) or 0),
            int(tokens.get("output", 0) or 0),
            model,
        )
        rec = {
            "task_id": iid,
            "passed": is_resolved,
            "success": is_resolved,
            "model": model,
            "tokens": {
                "input": int(tokens.get("input", 0) or 0),
                "output": int(tokens.get("output", 0) or 0),
                "total": int(tokens.get("total", 0) or 0),
            },
            "cost_usd": round(cost, 6),
            "iterations": int(p.get("iterations", 0) or 0),
            "tools": {
                "total": int(p.get("tools", {}).get("total", 0) or 0),
                "success": int(p.get("tools", {}).get("success", 0) or 0),
                "success_rate": (
                    round(int(p.get("tools", {}).get("success", 0) or 0)
                          / int(p.get("tools", {}).get("total", 1) or 1), 4)
                    if int(p.get("tools", {}).get("total", 0) or 0) else 0.0
                ),
            },
            "duration_ms_wall": int(p.get("duration_ms", 0) or 0),
            "patch_generated": bool(p.get("patch", "")),
            "failure_modes": p.get("failure_modes", []) or [],
        }
        out_path = args.output_dir / f"{iid.replace('/', '_')}.json"
        out_path.write_text(json.dumps(rec, indent=2))
        n += 1
        if is_resolved:
            resolved += 1
        total_cost += cost

    summary = {
        "bench": "swebench-lite",
        "tasks": n,
        "resolved": resolved,
        "resolved_rate": round(resolved / n, 4) if n else 0.0,
        "total_cost_usd": round(total_cost, 4),
        "graded": bool(graded),
    }
    (args.output_dir / "summary.json").write_text(json.dumps(summary, indent=2))
    print(f"[swebench-post] {n} instances, {resolved} resolved "
          f"({summary['resolved_rate']*100:.1f}%), ${total_cost:.2f} total cost"
          + (" (graded)" if graded else " (patches only)"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
