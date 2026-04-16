#!/usr/bin/env python3
"""
SWE-bench adapter for Theo Code.

Downloads instances from HuggingFace, sets up each repo at the failing
commit, invokes `theo --headless` with the issue text, captures the
generated patch, and writes results in SWE-bench submission format.

Supports two evaluation modes:
  1. Patch generation only (fast, no Docker)
  2. Official grader via `swebench` package (Docker required, authoritative)

Usage:
    # Smoke run (5 instances, patch generation only)
    python3 swe/adapter.py --limit 5

    # Full Lite with official grader (requires Docker + swebench package)
    python3 swe/adapter.py --dataset lite --grade

    # Resume an interrupted run
    python3 swe/adapter.py --dataset lite --resume

    # Multiple runs for statistical significance
    python3 swe/adapter.py --dataset lite --limit 20 --runs 3

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
REPORTS_DIR = ROOT / "apps" / "theo-benchmark" / "reports"

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from _headless import run_headless, run_headless_multi, HeadlessResult, _resolve_bin


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


def load_completed(report_path: Path) -> set[str]:
    """Load instance IDs already completed for --resume support."""
    if not report_path.exists():
        return set()
    completed = set()
    with report_path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
                completed.add(entry["instance_id"])
            except (json.JSONDecodeError, KeyError):
                continue
    return completed


def setup_repo(instance: dict, workdir: Path) -> bool:
    """Clone repo at the base commit."""
    repo = instance["repo"]
    base_commit = instance["base_commit"]
    repo_url = f"https://github.com/{repo}.git"

    try:
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


def build_prompt(instance: dict, *, include_tests: bool = True) -> str:
    """Build the prompt for the agent from the SWE-bench instance.

    Args:
        include_tests: If True, include FAIL_TO_PASS test names (oracle mode).
                       Set to False for non-oracle evaluation.
    """
    issue = instance.get("problem_statement", "")
    hints = instance.get("hints_text", "")
    fail_to_pass = instance.get("FAIL_TO_PASS", "") if include_tests else ""

    prompt = f"""Fix this bug in an open-source project. Be precise — correctness matters more than speed.

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

1. Read the failing test file(s) to understand WHAT behavior the test expects.
2. Understand the test assertions — they define the correct behavior.
3. Use grep to find the function/class being tested in the source code.
4. Read the relevant source code to understand WHY it fails.
5. Make the MINIMAL change to fix the bug. Match exactly what the tests expect.
6. Re-read the changed file to verify your edit is correct.
7. Call done with a summary of what you changed and why.

RULES:
- Do NOT add new tests or modify test files.
- Do NOT refactor unrelated code or create new files.
- Prefer editing existing code over adding new code.
- If unsure between two fixes, choose the one the test assertions imply."""

    return prompt


def run_instance(
    instance: dict,
    theo_bin: Path,
    timeout: int,
    keep_tmp: bool,
    *,
    temperature: float = 0.0,
    oracle: bool = True,
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
    error: str | None = None

    try:
        if not setup_repo(instance, workdir):
            result["error"] = "setup_failed"
            return result

        prompt = build_prompt(instance, include_tests=oracle)

        headless = run_headless(
            prompt,
            repo=workdir,
            max_iter=30,
            timeout=timeout,
            temperature=temperature,
            theo_bin=theo_bin,
        )

        # Capture git diff as the model patch
        diff_proc = subprocess.run(
            ["git", "diff"],
            cwd=workdir,
            capture_output=True,
            text=True,
            timeout=30,
        )
        result["model_patch"] = diff_proc.stdout

        # Also capture staged changes
        staged = subprocess.run(
            ["git", "diff", "--cached"],
            cwd=workdir,
            capture_output=True,
            text=True,
            timeout=30,
        )
        if staged.stdout:
            result["model_patch"] += "\n" + staged.stdout

        result["headless"] = {
            "success": headless.success,
            "summary": headless.summary,
            "iterations": headless.iterations,
            "duration_ms": headless.duration_ms,
            "tokens": {
                "input": headless.tokens_input,
                "output": headless.tokens_output,
                "total": headless.tokens_total,
            },
            "cost_usd": headless.cost_usd,
            "model": headless.model,
        }

    except subprocess.TimeoutExpired:
        error = f"timeout after {timeout}s"
    except Exception as e:
        error = f"exception: {e}"
    finally:
        duration = time.monotonic() - started
        if not keep_tmp:
            subprocess.run(["rm", "-rf", str(workdir)], check=False,
                           capture_output=True, timeout=60)

    result["duration_secs"] = round(duration, 1)
    result["error"] = error
    result["has_patch"] = bool(result.get("model_patch", "").strip())
    return result


# ---------------------------------------------------------------------------
# Official SWE-bench grading (requires swebench + Docker)
# ---------------------------------------------------------------------------


def grade_with_official_harness(predictions_path: Path, dataset: str) -> dict | None:
    """Run the official SWE-bench evaluation harness.

    Requires:
      - pip install swebench
      - Docker running
      - Sufficient disk space for per-repo Docker images

    Returns dict with {resolved, applied, error, total} or None if unavailable.
    """
    try:
        import swebench  # noqa: F401
    except ImportError:
        print("\n  WARNING: swebench package not installed. Skipping official grading.",
              file=sys.stderr)
        print("  Install with: pip install 'theo-benchmark[swe-grader]'",
              file=sys.stderr)
        return None

    ds_map = {
        "lite": "princeton-nlp/SWE-bench_Lite",
        "verified": "princeton-nlp/SWE-bench_Verified",
        "full": "princeton-nlp/SWE-bench",
    }
    ds_name = ds_map.get(dataset, dataset)

    output_dir = predictions_path.parent / "swe_eval_output"
    output_dir.mkdir(exist_ok=True)

    cmd = [
        sys.executable, "-m", "swebench.harness.run_evaluation",
        "--predictions_path", str(predictions_path),
        "--swe_bench_tasks", ds_name,
        "--log_level", "WARNING",
        "--run_id", f"theo-{int(time.time())}",
    ]

    print(f"\n  Running official SWE-bench grader...", file=sys.stderr)
    print(f"  Command: {' '.join(cmd)}", file=sys.stderr)

    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=7200,  # 2 hours max for full eval
        )
        if proc.returncode != 0:
            print(f"  Grader failed (exit {proc.returncode}): {proc.stderr[-500:]}",
                  file=sys.stderr)
            return None

        # Parse grader output for resolved count
        output = proc.stdout + proc.stderr
        resolved = output.count("RESOLVED")
        applied = output.count("APPLY_PATCH_PASS")
        error_count = output.count("APPLY_PATCH_FAIL") + output.count("ERROR")

        return {
            "resolved": resolved,
            "applied": applied,
            "error": error_count,
            "grader_output": output[-2000:],
        }

    except subprocess.TimeoutExpired:
        print("  Grader timed out after 2 hours.", file=sys.stderr)
        return None
    except Exception as e:
        print(f"  Grader exception: {e}", file=sys.stderr)
        return None


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------


def write_submission(results: list[dict], out_path: Path) -> Path:
    """Write results in SWE-bench submission format (predictions.jsonl)."""
    jsonl_path = out_path.with_suffix(".jsonl")
    with jsonl_path.open("w") as f:
        for r in results:
            f.write(json.dumps({
                "instance_id": r["instance_id"],
                "model_patch": r.get("model_patch", ""),
                "model_name_or_path": "theo-code",
            }) + "\n")
    print(f"  submission -> {jsonl_path}", file=sys.stderr)
    return jsonl_path


def print_summary(results: list[dict], grader_result: dict | None) -> None:
    total = len(results)
    has_patch = sum(1 for r in results if r.get("has_patch"))
    errors = sum(1 for r in results if r.get("error"))
    total_secs = sum(r.get("duration_secs", 0) for r in results)
    total_cost = 0.0
    total_tokens = 0
    for r in results:
        h = r.get("headless") or {}
        t = h.get("tokens") or {}
        total_tokens += t.get("total", 0)
        total_cost += h.get("cost_usd", 0)

    print()
    print("=" * 70)
    print(f"  SWE-bench -- {has_patch}/{total} produced patches "
          f"({errors} errors, {total_secs:.0f}s total)")
    if grader_result:
        resolved = grader_result.get("resolved", 0)
        print(f"  OFFICIAL GRADER: {resolved}/{total} resolved "
              f"({resolved/total*100:.1f}%)")
    else:
        print(f"  NOTE: Patch generation only. Run with --grade for official eval.")
    print(f"  Cost: ${total_cost:.2f} ({total_tokens:,} tokens)")
    print("=" * 70)

    for r in results:
        patch_len = len(r.get("model_patch", ""))
        status = "PATCH" if r.get("has_patch") else "EMPTY"
        if r.get("error"):
            status = "ERROR"
        h = r.get("headless") or {}
        iters = h.get("iterations", "?")
        t = (h.get("tokens") or {}).get("total", 0)
        cost = h.get("cost_usd", 0)
        print(f"  [{status:>5}] {r['instance_id']:<40} "
              f"iter={iters:<3} tok={t:<7} ${cost:.3f} "
              f"patch={patch_len}B "
              f"{r.get('duration_secs', 0):>5.0f}s")
    print()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dataset", default="lite",
                    help="lite, verified, or full (default: lite)")
    ap.add_argument("--limit", type=int, help="max instances to run")
    ap.add_argument("--filter", help="filter by repo/instance_id substring")
    ap.add_argument("--timeout", type=int, default=600,
                    help="timeout per instance in seconds (default: 600)")
    ap.add_argument("--bin", default=os.environ.get("THEO_BIN", ""),
                    help="path to theo binary")
    ap.add_argument("--keep-tmp", action="store_true")
    ap.add_argument("--report", help="output report path")
    ap.add_argument("--grade", action="store_true",
                    help="run official SWE-bench grader (requires Docker + swebench)")
    ap.add_argument("--temperature", type=float, default=0.0,
                    help="sampling temperature (default: 0.0 for deterministic)")
    ap.add_argument("--no-oracle", action="store_true",
                    help="don't include FAIL_TO_PASS test names in prompt")
    ap.add_argument("--resume", action="store_true",
                    help="skip instances already in the report file")
    args = ap.parse_args()

    theo_bin = Path(args.bin) if args.bin else _resolve_bin()
    if not theo_bin.exists():
        print(f"ERROR: theo binary not found at {theo_bin}", file=sys.stderr)
        return 2

    instances = load_dataset(args.dataset, args.limit, args.filter)
    if not instances:
        print("ERROR: no instances matched", file=sys.stderr)
        return 2

    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    out_path = Path(args.report) if args.report else (
        REPORTS_DIR / f"swe-{args.dataset}-{int(time.time())}.json"
    )

    # Resume support
    completed_ids: set[str] = set()
    if args.resume:
        jsonl_path = out_path.with_suffix(".jsonl")
        completed_ids = load_completed(jsonl_path)
        if completed_ids:
            print(f"  Resuming: {len(completed_ids)} already completed", file=sys.stderr)
            instances = [i for i in instances if i["instance_id"] not in completed_ids]

    results = []
    for i, inst in enumerate(instances):
        print(f"  [{i+1}/{len(instances)}] {inst['instance_id']}", file=sys.stderr, flush=True)
        result = run_instance(
            inst, theo_bin, args.timeout, args.keep_tmp,
            temperature=args.temperature,
            oracle=not args.no_oracle,
        )
        results.append(result)

        # Incremental save (append JSONL so we don't lose progress)
        jsonl_path = out_path.with_suffix(".jsonl")
        with jsonl_path.open("a") as f:
            f.write(json.dumps({
                "instance_id": result["instance_id"],
                "model_patch": result.get("model_patch", ""),
                "model_name_or_path": "theo-code",
            }) + "\n")

    # Write full report
    total_cost = sum((r.get("headless") or {}).get("cost_usd", 0) for r in results)
    report = {
        "schema": "theo.swe.v2",
        "dataset": args.dataset,
        "total": len(results),
        "with_patch": sum(1 for r in results if r.get("has_patch")),
        "errors": sum(1 for r in results if r.get("error")),
        "temperature": args.temperature,
        "oracle_mode": not args.no_oracle,
        "total_cost_usd": round(total_cost, 2),
        "results": results,
        "timestamp": int(time.time()),
    }

    # Official grading
    grader_result = None
    if args.grade:
        submission_path = write_submission(results, out_path)
        grader_result = grade_with_official_harness(submission_path, args.dataset)
        if grader_result:
            report["grader"] = grader_result

    out_path.write_text(json.dumps(report, indent=2))
    if not args.grade:
        write_submission(results, out_path)

    print_summary(results, grader_result)
    print(f"  report -> {out_path}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
