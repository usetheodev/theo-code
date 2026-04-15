#!/usr/bin/env python3
"""
Local SWE-bench runner — runs against hand-picked instances from local_instances.json.
No HuggingFace datasets dependency needed. Good for validating the adapter pipeline.

Usage:
    python3 apps/theo-benchmark/swe/local_runner.py
    python3 apps/theo-benchmark/swe/local_runner.py --limit 1
    python3 apps/theo-benchmark/swe/local_runner.py --filter django
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
INSTANCES_FILE = Path(__file__).parent / "local_instances.json"


def load_instances(filter_str: str | None, limit: int | None) -> list[dict]:
    instances = json.loads(INSTANCES_FILE.read_text())
    if filter_str:
        instances = [i for i in instances if filter_str.lower() in i.get("repo", "").lower()
                     or filter_str.lower() in i.get("instance_id", "").lower()]
    if limit:
        instances = instances[:limit]
    return instances


def setup_repo(instance: dict, workdir: Path) -> bool:
    repo = instance["repo"]
    base_commit = instance["base_commit"]
    repo_url = f"https://github.com/{repo}.git"

    try:
        subprocess.run(
            ["git", "clone", repo_url, str(workdir)],
            capture_output=True, text=True, timeout=300, check=True,
        )
        subprocess.run(
            ["git", "checkout", base_commit],
            cwd=workdir, capture_output=True, text=True, timeout=30, check=True,
        )
        return True
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
        print(f"  SKIP {instance['instance_id']}: {e}", file=sys.stderr)
        return False


def build_prompt(instance: dict) -> str:
    issue = instance["problem_statement"]
    hints = instance.get("hints_text", "")
    prompt = f"""Fix this bug in an open-source project.

## Issue
{issue}
"""
    if hints:
        prompt += f"""
## Hints
{hints}
"""
    prompt += """
## Instructions
1. Read relevant source code.
2. Make the minimal fix.
3. Do NOT add tests or modify test files.
4. Do NOT refactor unrelated code.
Fix it now."""
    return prompt


def run_instance(instance: dict, theo_bin: Path, timeout: int) -> dict:
    iid = instance["instance_id"]
    workdir = Path(tempfile.mkdtemp(prefix=f"swe-{iid}-"))
    started = time.monotonic()
    result: dict = {"instance_id": iid, "repo": instance["repo"]}
    headless = None

    try:
        if not setup_repo(instance, workdir):
            result["error"] = "setup_failed"
            return result

        prompt = build_prompt(instance)
        proc = subprocess.run(
            [str(theo_bin), "--headless", "--repo", str(workdir), "--max-iter", "30", prompt],
            capture_output=True, text=True, timeout=timeout,
        )

        for line in reversed(proc.stdout.splitlines()):
            if line.strip().startswith("{"):
                try:
                    headless = json.loads(line.strip())
                except json.JSONDecodeError:
                    pass
                break

        diff_proc = subprocess.run(
            ["git", "diff"], cwd=workdir, capture_output=True, text=True, timeout=30,
        )
        result["model_patch"] = diff_proc.stdout
        result["has_patch"] = bool(diff_proc.stdout.strip())

    except subprocess.TimeoutExpired:
        result["error"] = f"timeout after {timeout}s"
        result["has_patch"] = False
    except Exception as e:
        result["error"] = str(e)
        result["has_patch"] = False
    finally:
        result["duration_secs"] = round(time.monotonic() - started, 1)
        result["headless"] = headless
        subprocess.run(["rm", "-rf", str(workdir)], check=False)

    return result


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--limit", type=int)
    ap.add_argument("--filter")
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("--bin", default=os.environ.get("THEO_BIN", str(DEFAULT_BIN)))
    args = ap.parse_args()

    theo_bin = Path(args.bin)
    if not theo_bin.exists():
        print(f"ERROR: {theo_bin} not found", file=sys.stderr)
        return 2

    instances = load_instances(args.filter, args.limit)
    if not instances:
        print("ERROR: no instances", file=sys.stderr)
        return 2

    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    results = []
    for i, inst in enumerate(instances):
        print(f"  [{i+1}/{len(instances)}] {inst['instance_id']}", file=sys.stderr, flush=True)
        results.append(run_instance(inst, theo_bin, args.timeout))

    # Summary
    total = len(results)
    patched = sum(1 for r in results if r.get("has_patch"))
    errors = sum(1 for r in results if r.get("error"))
    print(f"\n{'='*60}")
    print(f"  SWE-bench Local — {patched}/{total} produced patches ({errors} errors)")
    print(f"{'='*60}")
    for r in results:
        h = r.get("headless") or {}
        t = (h.get("tokens") or {}).get("total", 0)
        status = "PATCH" if r.get("has_patch") else ("ERROR" if r.get("error") else "EMPTY")
        print(f"  [{status:>5}] {r['instance_id']:<35} tok={t:<7} {r.get('duration_secs',0):>5.0f}s")
        if r.get("error"):
            print(f"         {r['error']}")
    print()

    report_path = REPORTS_DIR / f"swe-local-{int(time.time())}.json"
    report_path.write_text(json.dumps({"results": results, "timestamp": int(time.time())}, indent=2))
    print(f"  report → {report_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
