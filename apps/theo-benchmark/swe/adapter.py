#!/usr/bin/env python3
"""
SWE-bench Verified adapter for Theo Code.

Downloads instances from HuggingFace, sets up each repo at the failing
commit, invokes `theo --headless` with the issue text, captures the
generated patch, and writes results in SWE-bench submission format.

Usage:
    # Smoke run (5 instances)
    python3 apps/theo-benchmark/swe/adapter.py --limit 5

    # Filter by repo
    python3 apps/theo-benchmark/swe/adapter.py --filter django --limit 10

    # Full Verified run (500 instances, expensive!)
    python3 apps/theo-benchmark/swe/adapter.py --dataset verified

Environment:
    THEO_BIN    path to theo binary (default: target/release/theo)
    HF_TOKEN    HuggingFace token (optional, for gated datasets)
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DEFAULT_BIN = ROOT / "target" / "release" / "theo"
REPORTS_DIR = ROOT / "apps" / "theo-benchmark" / "reports"


def load_dataset(dataset: str, limit: int | None, filter_str: str | None) -> list[dict]:
    """Load SWE-bench instances from HuggingFace."""
    try:
        from datasets import load_dataset as hf_load
    except ImportError:
        print("ERROR: pip install datasets", file=sys.stderr)
        sys.exit(2)

    ds_map = {
        "lite": "princeton-nlp/SWE-bench_Lite",
        "verified": "princeton-nlp/SWE-bench_Verified",
        "full": "princeton-nlp/SWE-bench",
    }
    ds_name = ds_map.get(dataset, dataset)
    print(f"Loading {ds_name}...", file=sys.stderr)
    ds = hf_load(ds_name, split="test")

    instances = list(ds)
    if filter_str:
        instances = [i for i in instances if filter_str.lower() in i.get("repo", "").lower()
                     or filter_str.lower() in i.get("instance_id", "").lower()]
    if limit:
        instances = instances[:limit]

    print(f"Loaded {len(instances)} instances", file=sys.stderr)
    return instances


def setup_repo(instance: dict, workdir: Path) -> bool:
    """Clone repo at the base commit."""
    repo = instance["repo"]
    base_commit = instance["base_commit"]
    repo_url = f"https://github.com/{repo}.git"

    try:
        # Try shallow clone first (fast), fall back to full clone if commit is old
        shallow = subprocess.run(
            ["git", "clone", "--depth", "50", repo_url, str(workdir)],
            capture_output=True, text=True, timeout=180,
        )
        if shallow.returncode != 0:
            subprocess.run(
                ["git", "clone", repo_url, str(workdir)],
                capture_output=True, text=True, timeout=600, check=True,
            )

        co = subprocess.run(
            ["git", "checkout", base_commit],
            cwd=workdir, capture_output=True, text=True, timeout=30,
        )
        if co.returncode != 0:
            # Commit not in shallow history — fetch it or do full clone
            subprocess.run(
                ["git", "fetch", "--unshallow"],
                cwd=workdir, capture_output=True, text=True, timeout=600,
            )
            subprocess.run(
                ["git", "checkout", base_commit],
                cwd=workdir, capture_output=True, text=True, timeout=30, check=True,
            )
        return True
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
        print(f"  SKIP {instance['instance_id']}: clone/checkout failed: {e}",
              file=sys.stderr)
        return False


def build_prompt(instance: dict) -> str:
    """Build the prompt for the agent from the SWE-bench instance."""
    issue = instance.get("problem_statement", "")
    hints = instance.get("hints_text", "")
    fail_to_pass = instance.get("FAIL_TO_PASS", "")

    prompt = f"""Fix this bug. Be FAST and MINIMAL — aim for under 15 iterations.

## Issue

{issue}
"""
    if hints:
        prompt += f"""
## Hints

{hints}
"""
    if fail_to_pass:
        prompt += f"""
## Failing Tests (CRITICAL — your fix MUST make these pass)

{fail_to_pass}

Read these test files FIRST to understand what the expected behavior is.
The test assertions tell you exactly what the code should do.
"""
    prompt += """
## Strategy (follow this order)

1. Read the failing test file(s) to understand WHAT the test expects.
2. Use grep to find the function/class being tested in the source code.
3. Read the relevant source code section (not the whole file).
4. Make the MINIMAL change to fix the bug. One-line fixes are ideal.
5. Verify your edit by re-reading the changed section.
6. Call done immediately. Do NOT run tests (no test environment available).

CRITICAL RULES:
- Do NOT add new tests or modify test files.
- Do NOT refactor unrelated code.
- Do NOT create new files.
- Prefer editing existing code over adding new code.
- If the fix is a one-liner, just do it. Don't overthink."""

    return prompt


def run_instance(
    instance: dict,
    theo_bin: Path,
    timeout: int,
    keep_tmp: bool,
) -> dict:
    """Run theo on a single SWE-bench instance."""
    iid = instance["instance_id"]
    workdir = Path(tempfile.mkdtemp(prefix=f"swe-{iid}-"))
    started = time.monotonic()

    result: dict = {
        "instance_id": iid,
        "repo": instance["repo"],
        "model_patch": "",
        "model_name_or_path": "theo-code",
    }
    headless: dict | None = None
    error: str | None = None

    try:
        if not setup_repo(instance, workdir):
            result["error"] = "setup_failed"
            return result

        prompt = build_prompt(instance)

        proc = subprocess.run(
            [
                str(theo_bin),
                "--headless",
                "--repo", str(workdir),
                "--max-iter", "30",
                prompt,
            ],
            capture_output=True,
            text=True,
            timeout=timeout,
        )

        # Parse headless JSON
        for line in reversed(proc.stdout.splitlines()):
            line = line.strip()
            if line.startswith("{"):
                try:
                    headless = json.loads(line)
                except json.JSONDecodeError:
                    pass
                break

        # Capture git diff as the model patch
        diff_proc = subprocess.run(
            ["git", "diff"],
            cwd=workdir,
            capture_output=True,
            text=True,
            timeout=30,
        )
        result["model_patch"] = diff_proc.stdout

        # Also capture diff of untracked files
        untracked = subprocess.run(
            ["git", "diff", "--cached"],
            cwd=workdir,
            capture_output=True,
            text=True,
            timeout=30,
        )
        if untracked.stdout:
            result["model_patch"] += "\n" + untracked.stdout

    except subprocess.TimeoutExpired:
        error = f"timeout after {timeout}s"
    except Exception as e:
        error = f"exception: {e}"
    finally:
        duration = time.monotonic() - started
        if not keep_tmp:
            subprocess.run(["rm", "-rf", str(workdir)], check=False)

    result["duration_secs"] = round(duration, 1)
    result["error"] = error
    result["headless"] = headless
    result["has_patch"] = bool(result.get("model_patch", "").strip())
    return result


def write_submission(results: list[dict], out_path: Path) -> None:
    """Write results in SWE-bench submission format (predictions.jsonl)."""
    predictions = []
    for r in results:
        predictions.append({
            "instance_id": r["instance_id"],
            "model_patch": r.get("model_patch", ""),
            "model_name_or_path": "theo-code",
        })
    jsonl_path = out_path.with_suffix(".jsonl")
    with jsonl_path.open("w") as f:
        for p in predictions:
            f.write(json.dumps(p) + "\n")
    print(f"  submission → {jsonl_path}", file=sys.stderr)


def print_summary(results: list[dict]) -> None:
    total = len(results)
    has_patch = sum(1 for r in results if r.get("has_patch"))
    errors = sum(1 for r in results if r.get("error"))
    total_secs = sum(r.get("duration_secs", 0) for r in results)
    total_tokens = 0
    for r in results:
        h = r.get("headless") or {}
        t = h.get("tokens") or {}
        total_tokens += t.get("total", 0)

    print()
    print("=" * 70)
    print(f"  SWE-bench — {has_patch}/{total} produced patches "
          f"({errors} errors, {total_secs:.0f}s total)")
    print("=" * 70)
    for r in results:
        patch_len = len(r.get("model_patch", ""))
        status = "PATCH" if r.get("has_patch") else "EMPTY"
        if r.get("error"):
            status = "ERROR"
        h = r.get("headless") or {}
        iters = h.get("iterations", "?")
        t = (h.get("tokens") or {}).get("total", 0)
        print(f"  [{status:>5}] {r['instance_id']:<40} "
              f"iter={iters:<3} tok={t:<7} patch={patch_len}B "
              f"{r.get('duration_secs', 0):>5.0f}s")
    print()
    print(f"  totals: {total_tokens} tokens, {total_secs:.0f}s wall-clock")
    print(f"  NOTE: patch generation ≠ correctness. Run SWE-bench grader to")
    print(f"        evaluate which patches actually pass tests.")
    print()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dataset", default="lite",
                    help="lite, verified, or full (default: lite)")
    ap.add_argument("--limit", type=int, help="max instances to run")
    ap.add_argument("--filter", help="filter by repo/instance_id substring")
    ap.add_argument("--timeout", type=int, default=600,
                    help="timeout per instance in seconds (default: 600)")
    ap.add_argument("--bin", default=os.environ.get("THEO_BIN", str(DEFAULT_BIN)),
                    help="path to theo binary")
    ap.add_argument("--keep-tmp", action="store_true")
    ap.add_argument("--report", help="output report path")
    args = ap.parse_args()

    theo_bin = Path(args.bin)
    if not theo_bin.exists():
        print(f"ERROR: theo binary not found at {theo_bin}", file=sys.stderr)
        return 2

    instances = load_dataset(args.dataset, args.limit, args.filter)
    if not instances:
        print("ERROR: no instances matched", file=sys.stderr)
        return 2

    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    results = []
    for i, inst in enumerate(instances):
        print(f"  [{i+1}/{len(instances)}] {inst['instance_id']}", file=sys.stderr, flush=True)
        results.append(run_instance(inst, theo_bin, args.timeout, args.keep_tmp))

    out_path = Path(args.report) if args.report else (
        REPORTS_DIR / f"swe-{args.dataset}-{int(time.time())}.json"
    )
    report = {
        "schema": "theo.swe.v1",
        "dataset": args.dataset,
        "total": len(results),
        "with_patch": sum(1 for r in results if r.get("has_patch")),
        "errors": sum(1 for r in results if r.get("error")),
        "results": results,
        "timestamp": int(time.time()),
    }
    out_path.write_text(json.dumps(report, indent=2))

    write_submission(results, out_path)
    print_summary(results)
    print(f"  report → {out_path}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
