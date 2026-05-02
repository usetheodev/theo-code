#!/usr/bin/env python3
"""E2E Probe Runner — validates Theo features with real or mock LLM.

Usage:
    python e2e/probe_runner.py                    # all probes, mock mode
    python e2e/probe_runner.py --real-llm         # all probes, real LLM
    python e2e/probe_runner.py --suite cli        # only CLI subcommand probes
    python e2e/probe_runner.py --filter cli-help  # single probe by ID
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path

# --- Try to import tomllib (Python 3.11+) or tomli as fallback ---
try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ImportError:
        print("ERROR: Python 3.11+ or 'pip install tomli' required for TOML parsing", file=sys.stderr)
        sys.exit(1)

PROBES_DIR = Path(__file__).parent / "probes"
REPORTS_DIR = Path(__file__).resolve().parent.parent / "reports"
THEO_BIN = os.environ.get("THEO_BIN", "theo")


def load_probes(suite: str | None = None) -> list[dict]:
    """Load probe definitions from TOML files."""
    probes = []
    for toml_file in sorted(PROBES_DIR.glob("*.toml")):
        with open(toml_file, "rb") as f:
            data = tomllib.load(f)
        for probe in data.get("probe", []):
            if suite and not toml_file.stem.startswith(suite):
                continue
            probe["_source"] = toml_file.name
            probes.append(probe)
    return probes


def run_probe(probe: dict, *, real_llm: bool, project_dir: str | None) -> dict:
    """Execute a single probe and return a benchmark-run result."""
    run_id = str(uuid.uuid4())
    start = time.monotonic()

    # Skip probes that require resources we don't have
    if probe.get("requires_llm") and not real_llm:
        return _skip_result(probe, run_id, reason="requires --real-llm")

    mode = probe.get("mode", "headless")
    command = probe["command"]
    timeout = probe.get("timeout_secs", 120)

    tmpdir = None
    cwd = project_dir or os.getcwd()
    if probe.get("requires_tmpdir"):
        tmpdir = tempfile.mkdtemp(prefix="theo-probe-")
        cwd = tmpdir
        subprocess.run(["git", "init"], cwd=cwd, capture_output=True)

    try:
        if mode == "raw":
            result = _run_raw(command, cwd=cwd, timeout=timeout)
        else:
            result = _run_headless(command, cwd=cwd, timeout=timeout)
    except subprocess.TimeoutExpired:
        result = {"exit_code": 1, "stdout": "", "stderr": "TIMEOUT", "success": False}
    except Exception as e:
        result = {"exit_code": 1, "stdout": "", "stderr": str(e), "success": False}

    duration_ms = int((time.monotonic() - start) * 1000)

    # Evaluate success check
    passed = _eval_success(probe.get("success_check", "exit_code == 0"), result)

    return {
        "schema_version": "theo.benchmark-run.v1",
        "run_id": run_id,
        "model_id": os.environ.get("THEO_MODEL", "unknown"),
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "theo_sha": _get_git_sha(),
        "task_id": probe["id"],
        "task_category": probe.get("category", "e2e-probe"),
        "benchmark_suite": "e2e-probe",
        "pass": passed,
        "duration_ms": duration_ms,
        "tokens": {"input": 0, "output": 0, "total": 0},
        "cost_usd": 0.0,
        "metadata": {
            "probe_source": probe.get("_source", "unknown"),
            "real_llm": real_llm,
        },
    }


def _run_raw(command: str, *, cwd: str, timeout: int) -> dict:
    """Run theo with raw args (not headless)."""
    args = [THEO_BIN] + command.split()
    proc = subprocess.run(
        args, cwd=cwd, capture_output=True, text=True, timeout=timeout
    )
    return {
        "exit_code": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
        "success": proc.returncode == 0,
    }


def _run_headless(command: str, *, cwd: str, timeout: int) -> dict:
    """Run theo --headless and parse JSON result."""
    args = [THEO_BIN, "--headless"] + command.split()
    proc = subprocess.run(
        args, cwd=cwd, capture_output=True, text=True, timeout=timeout
    )
    result = {
        "exit_code": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
        "success": proc.returncode == 0,
    }
    # Try to parse JSON from stdout
    try:
        parsed = json.loads(proc.stdout)
        result.update(parsed)
    except (json.JSONDecodeError, ValueError):
        pass
    return result


def _eval_success(check: str, result: dict) -> bool:
    """Evaluate a success check expression against the result."""
    try:
        exit_code = result.get("exit_code", 1)
        stdout = result.get("stdout", "")
        stderr = result.get("stderr", "")
        return bool(eval(check, {"__builtins__": {}}, {
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
            "result": result,
            "len": len,
            "True": True,
            "False": False,
        }))
    except Exception:
        return False


def _skip_result(probe: dict, run_id: str, reason: str) -> dict:
    return {
        "schema_version": "theo.benchmark-run.v1",
        "run_id": run_id,
        "model_id": "skip",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "theo_sha": _get_git_sha(),
        "task_id": probe["id"],
        "task_category": probe.get("category", "e2e-probe"),
        "benchmark_suite": "e2e-probe",
        "pass": False,
        "duration_ms": 0,
        "tokens": {"input": 0, "output": 0, "total": 0},
        "cost_usd": 0.0,
        "metadata": {"skipped": True, "skip_reason": reason},
    }


def _get_git_sha() -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"], text=True
        ).strip()
    except Exception:
        return "unknown"


def main():
    parser = argparse.ArgumentParser(description="E2E Probe Runner for Theo Code")
    parser.add_argument("--real-llm", action="store_true", help="Use real LLM (default: skip LLM-requiring probes)")
    parser.add_argument("--suite", type=str, help="Filter probes by suite name (cli, tool, provider)")
    parser.add_argument("--filter", type=str, help="Filter probes by ID substring")
    parser.add_argument("--project", type=str, help="Project directory for probes requiring a codebase")
    parser.add_argument("--report", type=str, help="Custom report output path")
    args = parser.parse_args()

    probes = load_probes(suite=args.suite)
    if args.filter:
        probes = [p for p in probes if args.filter in p["id"]]

    if not probes:
        print("No probes matched the filter criteria.", file=sys.stderr)
        sys.exit(1)

    print(f"Running {len(probes)} probes (real_llm={args.real_llm})...")
    results = []
    passed = 0
    skipped = 0

    for probe in probes:
        result = run_probe(probe, real_llm=args.real_llm, project_dir=args.project)
        results.append(result)

        if result.get("metadata", {}).get("skipped"):
            skipped += 1
            status = "SKIP"
        elif result["pass"]:
            passed += 1
            status = "PASS"
        else:
            status = "FAIL"
        print(f"  [{status}] {probe['id']}")

    total = len(results)
    executed = total - skipped
    pass_rate = passed / executed if executed > 0 else 0.0

    report = {
        "schema": "theo.e2e-probe.v1",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "probes_total": total,
        "probes_executed": executed,
        "probes_passed": passed,
        "probes_skipped": skipped,
        "pass_rate": round(pass_rate, 4),
        "real_llm": args.real_llm,
        "results": results,
    }

    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    report_path = args.report or str(
        REPORTS_DIR / f"e2e-probe-{int(time.time())}.json"
    )
    with open(report_path, "w") as f:
        json.dump(report, f, indent=2)

    print(f"\nResults: {passed}/{executed} passed ({pass_rate:.0%}), {skipped} skipped")
    print(f"Report: {report_path}")

    sys.exit(0 if pass_rate == 1.0 or executed == 0 else 1)


if __name__ == "__main__":
    main()
