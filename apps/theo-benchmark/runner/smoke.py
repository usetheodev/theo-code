#!/usr/bin/env python3
"""
Smoke runner for Theo Code — Phase 0 of the benchmark plan.

Spins up an isolated tmpdir per scenario, materializes the setup_files
listed in the scenario TOML, invokes `theo --headless`, and runs the
shell success_check. Aggregates everything into a single OTel-style
JSON report under apps/theo-benchmark/reports/.

Usage:
    python3 apps/theo-benchmark/runner/smoke.py
    python3 apps/theo-benchmark/runner/smoke.py --filter 03
    python3 apps/theo-benchmark/runner/smoke.py --bin /custom/theo
    python3 apps/theo-benchmark/runner/smoke.py --keep-tmp

Environment:
    THEO_BIN  override path to the theo binary
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

try:
    import tomllib  # py>=3.11
except ModuleNotFoundError:
    import tomli as tomllib  # type: ignore

ROOT = Path(__file__).resolve().parents[3]
SCENARIOS_DIR = ROOT / "apps" / "theo-benchmark" / "scenarios" / "smoke"
REPORTS_DIR = ROOT / "apps" / "theo-benchmark" / "reports"
DEFAULT_BIN = ROOT / "target" / "release" / "theo"


def load_scenarios(filter_substr: str | None) -> list[dict]:
    scenarios = []
    for path in sorted(SCENARIOS_DIR.glob("*.toml")):
        if filter_substr and filter_substr not in path.name:
            continue
        with path.open("rb") as fp:
            data = tomllib.load(fp)
        data["__path"] = str(path)
        scenarios.append(data)
    return scenarios


def materialize_setup(scenario: dict, workdir: Path) -> None:
    workdir.mkdir(parents=True, exist_ok=True)
    for entry in scenario.get("setup_files", []):
        target = workdir / entry["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(entry["content"])
    # Initialize git so theo treats it as a project root
    subprocess.run(
        ["git", "init", "-q"],
        cwd=workdir,
        check=False,
        capture_output=True,
    )
    subprocess.run(
        ["git", "add", "-A"],
        cwd=workdir,
        check=False,
        capture_output=True,
    )
    subprocess.run(
        ["git", "-c", "user.email=t@t", "-c", "user.name=t",
         "commit", "-q", "-m", "init"],
        cwd=workdir,
        check=False,
        capture_output=True,
    )


def run_scenario(scenario: dict, theo_bin: Path, keep_tmp: bool) -> dict:
    sid = scenario["id"]
    timeout = scenario.get("timeout_secs", 180)
    mode = scenario.get("mode", "agent")
    prompt = scenario["prompt"]

    workdir = Path(tempfile.mkdtemp(prefix=f"theo-smoke-{sid}-"))
    started = time.monotonic()
    headless: dict | None = None
    error: str | None = None
    exit_code = -1
    stderr_tail = ""

    try:
        materialize_setup(scenario, workdir)

        proc = subprocess.run(
            [
                str(theo_bin),
                "--headless",
                "--repo", str(workdir),
                "--mode", mode,
                "--max-iter", "20",
                prompt,
            ],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        exit_code = proc.returncode
        stderr_tail = proc.stderr[-2000:] if proc.stderr else ""

        # Find the JSON line in stdout (last non-empty line)
        json_line = None
        for line in reversed(proc.stdout.splitlines()):
            line = line.strip()
            if line.startswith("{"):
                json_line = line
                break
        if json_line:
            try:
                headless = json.loads(json_line)
            except json.JSONDecodeError as e:
                error = f"json decode failed: {e}"
        else:
            error = "no JSON line on stdout"

        # Run success_check
        check_passed = False
        check_stderr = ""
        check_script = scenario.get("success_check", "false")
        env = os.environ.copy()
        env["THEO_SUMMARY"] = (headless or {}).get("summary", "") if headless else ""
        check_proc = subprocess.run(
            ["bash", "-c", check_script],
            cwd=workdir,
            capture_output=True,
            text=True,
            env=env,
            timeout=30,
        )
        check_passed = check_proc.returncode == 0
        check_stderr = (
            f"rc={check_proc.returncode} "
            f"stdout={check_proc.stdout[-200:]!r} "
            f"stderr={check_proc.stderr[-200:]!r}"
        )

    except subprocess.TimeoutExpired:
        error = f"timeout after {timeout}s"
        check_passed = False
        check_stderr = ""
    except Exception as e:  # pragma: no cover
        error = f"runner exception: {e}"
        check_passed = False
        check_stderr = ""
    finally:
        duration = time.monotonic() - started
        if not keep_tmp:
            subprocess.run(["rm", "-rf", str(workdir)], check=False)

    return {
        "id": sid,
        "category": scenario.get("category", "unknown"),
        "mode": mode,
        "exit_code": exit_code,
        "error": error,
        "wallclock_secs": round(duration, 2),
        "check_passed": check_passed,
        "check_stderr": check_stderr,
        "headless": headless,
        "stderr_tail": stderr_tail,
        "workdir": str(workdir) if keep_tmp else None,
    }


def aggregate(results: list[dict]) -> dict:
    total = len(results)
    passed = sum(1 for r in results if r["check_passed"])
    by_cat: dict[str, dict] = {}
    total_input = total_output = total_iter = total_tools = 0
    total_llm_calls = total_retries = 0
    total_duration_ms = 0
    for r in results:
        cat = r["category"]
        slot = by_cat.setdefault(cat, {"total": 0, "passed": 0})
        slot["total"] += 1
        if r["check_passed"]:
            slot["passed"] += 1
        h = r.get("headless") or {}
        toks = h.get("tokens", {}) or {}
        tools = h.get("tools", {}) or {}
        llm = h.get("llm", {}) or {}
        total_input += toks.get("input", 0) or 0
        total_output += toks.get("output", 0) or 0
        total_iter += h.get("iterations", 0) or 0
        total_tools += tools.get("total", 0) or 0
        total_llm_calls += llm.get("calls", 0) or 0
        total_retries += llm.get("retries", 0) or 0
        total_duration_ms += h.get("duration_ms", 0) or 0

    return {
        "schema": "theo.smoke.v1",
        "scenarios_total": total,
        "scenarios_passed": passed,
        "pass_rate": round(passed / total, 3) if total else 0.0,
        "totals": {
            "input_tokens": total_input,
            "output_tokens": total_output,
            "iterations": total_iter,
            "tool_calls": total_tools,
            "llm_calls": total_llm_calls,
            "retries": total_retries,
            "duration_ms": total_duration_ms,
        },
        "by_category": by_cat,
    }


def print_summary(report: dict, results: list[dict]) -> None:
    print()
    print("=" * 70)
    print(f"  THEO SMOKE — {report['scenarios_passed']}/{report['scenarios_total']} passed "
          f"({report['pass_rate']*100:.0f}%)")
    print("=" * 70)
    for r in results:
        mark = "PASS" if r["check_passed"] else "FAIL"
        h = r.get("headless") or {}
        toks = (h.get("tokens", {}) or {}).get("total", 0)
        iters = h.get("iterations", "?")
        secs = r["wallclock_secs"]
        print(f"  [{mark}] {r['id']:<22} cat={r['category']:<11} "
              f"iter={iters:<3} tok={toks:<6} {secs:>5.1f}s")
        if not r["check_passed"]:
            if r.get("error"):
                print(f"         error: {r['error']}")
            if r.get("check_stderr"):
                tail = r["check_stderr"].strip().splitlines()[-3:]
                for line in tail:
                    print(f"         check: {line}")
    print()
    t = report["totals"]
    print(f"  totals: {t['iterations']} iters, {t['tool_calls']} tool calls, "
          f"{t['llm_calls']} llm calls, {t['retries']} retries")
    print(f"          {t['input_tokens']} in / {t['output_tokens']} out tokens, "
          f"{t['duration_ms']/1000:.1f}s total")
    print()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--filter", help="substring filter on scenario filename")
    ap.add_argument("--bin", help="path to theo binary",
                    default=os.environ.get("THEO_BIN", str(DEFAULT_BIN)))
    ap.add_argument("--keep-tmp", action="store_true",
                    help="keep tmpdirs for inspection")
    ap.add_argument("--report",
                    help="report output path (default: reports/smoke-<ts>.json)")
    args = ap.parse_args()

    theo_bin = Path(args.bin)
    if not theo_bin.exists():
        print(f"ERROR: theo binary not found at {theo_bin}", file=sys.stderr)
        print("Build with: cargo build -p theo --release", file=sys.stderr)
        return 2

    scenarios = load_scenarios(args.filter)
    if not scenarios:
        print("ERROR: no scenarios matched", file=sys.stderr)
        return 2

    REPORTS_DIR.mkdir(parents=True, exist_ok=True)

    print(f"Running {len(scenarios)} scenarios via {theo_bin}", file=sys.stderr)
    results = []
    for sc in scenarios:
        print(f"  → {sc['id']}", file=sys.stderr, flush=True)
        results.append(run_scenario(sc, theo_bin, args.keep_tmp))

    report = aggregate(results)
    report["results"] = results
    report["timestamp"] = int(time.time())
    report["theo_bin"] = str(theo_bin)

    out_path = Path(args.report) if args.report else (
        REPORTS_DIR / f"smoke-{int(time.time())}.json"
    )
    out_path.write_text(json.dumps(report, indent=2))

    print_summary(report, results)
    print(f"  report → {out_path}")

    return 0 if report["pass_rate"] >= 0.8 else 1


if __name__ == "__main__":
    sys.exit(main())
