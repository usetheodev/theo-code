#!/usr/bin/env python3
"""
Prompt Evolution Loop — autoresearch-style self-improvement for Theo Code.

Pattern: EVAL → ANALYZE → MUTATE → RE-EVAL → COMPARE → ACCEPT/REVERT

Each iteration:
1. Run the smoke benchmark with the current system prompt
2. Analyze failures and inefficiencies
3. Generate a prompt mutation hypothesis
4. Apply the mutation
5. Re-run the benchmark
6. Compare scores — accept if better, revert if worse
7. Log everything for learning

Usage:
    python3 apps/theo-benchmark/runner/evolve.py --iterations 5
    python3 apps/theo-benchmark/runner/evolve.py --iterations 3 --target swe --swe-filter requests
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
PROMPT_PATH = ROOT / "crates" / "theo-agent-runtime" / "src" / "config.rs"
REPORTS_DIR = ROOT / "apps" / "theo-benchmark" / "reports"
EVOLVE_LOG = REPORTS_DIR / "evolve-log.jsonl"
THEO_BIN = ROOT / "target" / "release" / "theo"


def run_smoke() -> dict:
    """Run smoke suite, return parsed report."""
    proc = subprocess.run(
        [sys.executable, str(ROOT / "apps/theo-benchmark/runner/smoke.py")],
        capture_output=True, text=True, timeout=1800,
    )
    # Find the latest report
    reports = sorted(REPORTS_DIR.glob("smoke-*.json"))
    if not reports:
        return {"pass_rate": 0, "totals": {}}
    return json.loads(reports[-1].read_text())


def run_swe(filter_str: str, limit: int = 3) -> dict:
    """Run SWE-bench subset, return parsed report."""
    proc = subprocess.run(
        [sys.executable, str(ROOT / "apps/theo-benchmark/swe/adapter.py"),
         "--dataset", "lite", "--filter", filter_str,
         "--limit", str(limit), "--timeout", "300"],
        capture_output=True, text=True, timeout=1200,
    )
    reports = sorted(REPORTS_DIR.glob("swe-lite-*.json"))
    if not reports:
        return {"total": 0, "with_patch": 0}
    return json.loads(reports[-1].read_text())


def analyze_smoke(report: dict) -> dict:
    """Extract key metrics and failure patterns from smoke report."""
    results = report.get("results", [])
    total = len(results)
    passed = sum(1 for r in results if r.get("check_passed"))
    total_iter = sum((r.get("headless") or {}).get("iterations", 0) for r in results)
    total_tokens = sum(
        (r.get("headless") or {}).get("tokens", {}).get("total", 0)
        for r in results
    )
    failures = [r["id"] for r in results if not r.get("check_passed")]
    high_iter = [(r["id"], (r.get("headless") or {}).get("iterations", 0))
                 for r in results if (r.get("headless") or {}).get("iterations", 0) > 12]

    return {
        "pass_rate": passed / total if total else 0,
        "total_iterations": total_iter,
        "total_tokens": total_tokens,
        "avg_iterations": total_iter / total if total else 0,
        "failures": failures,
        "high_iteration_scenarios": high_iter,
    }


def analyze_swe(report: dict) -> dict:
    """Extract metrics from SWE report."""
    results = report.get("results", [])
    total = len(results)
    patched = sum(1 for r in results if r.get("has_patch"))
    total_iter = sum((r.get("headless") or {}).get("iterations", 0) for r in results)
    at_limit = sum(1 for r in results
                   if (r.get("headless") or {}).get("iterations", 0) >= 29)
    return {
        "total": total,
        "patched": patched,
        "patch_rate": patched / total if total else 0,
        "avg_iterations": total_iter / total if total else 0,
        "at_iteration_limit": at_limit,
    }


def compute_score(smoke_analysis: dict, swe_analysis: dict | None = None) -> float:
    """Compute a composite score for comparison. Higher is better."""
    # Smoke: pass_rate (0-1) × 40 + efficiency bonus (fewer iterations = better)
    smoke_score = smoke_analysis["pass_rate"] * 40
    if smoke_analysis["avg_iterations"] > 0:
        # Bonus for efficiency: 10 points if avg < 5 iter, 0 if avg > 20
        efficiency = max(0, min(10, 10 * (1 - (smoke_analysis["avg_iterations"] - 5) / 15)))
        smoke_score += efficiency

    # SWE: patch_rate × 40 + iteration efficiency × 10
    swe_score = 0
    if swe_analysis and swe_analysis["total"] > 0:
        swe_score = swe_analysis["patch_rate"] * 40
        if swe_analysis["avg_iterations"] > 0:
            swe_eff = max(0, min(10, 10 * (1 - (swe_analysis["avg_iterations"] - 10) / 20)))
            swe_score += swe_eff

    return smoke_score + swe_score


def generate_mutation(smoke_analysis: dict, swe_analysis: dict | None, iteration: int) -> dict:
    """Generate a prompt mutation hypothesis based on failure analysis."""
    mutations = []

    # Pattern 1: too many iterations
    if smoke_analysis["avg_iterations"] > 10:
        mutations.append({
            "type": "efficiency",
            "target": "system_prompt",
            "description": "Add stronger iteration efficiency directive",
            "find": "Minimize iterations.",
            "replace": "Minimize iterations. Target 3-5 iterations for simple tasks, 8-12 for complex. Every extra iteration wastes budget.",
        })

    # Pattern 2: specific failures
    for fail_id in smoke_analysis.get("failures", []):
        if "plan" in fail_id:
            mutations.append({
                "type": "plan_reliability",
                "target": "plan_prompt",
                "description": f"Plan mode scenario {fail_id} failed — reinforce write+done requirement",
                "find": "you MUST now call the `write` tool",
                "replace": "you MUST now call the `write` tool to save your plan. This is NOT optional. Call write THEN done.",
            })

    # Pattern 3: SWE iteration limit
    if swe_analysis and swe_analysis.get("at_iteration_limit", 0) > 0:
        ratio = swe_analysis["at_iteration_limit"] / max(swe_analysis["total"], 1)
        if ratio > 0.3:
            mutations.append({
                "type": "swe_convergence",
                "target": "swe_prompt",
                "description": f"{ratio:.0%} of SWE instances hit iteration limit — make agent converge faster",
                "action": "swe_prompt_faster",
            })

    # Mutation bank: ordered by expected impact. Each must have find/replace.
    mutation_bank = [
        {
            "type": "batch_usage",
            "description": "Encourage batch tool calls to reduce iterations",
            "find": "Use `batch` to read multiple files in one call.",
            "replace": "ALWAYS use `batch` when reading 2+ files or doing 2+ searches. This halves your iteration count. Example: batch(calls: [{tool: \"read\", args: {filePath: \"a.rs\"}}, {tool: \"grep\", args: {pattern: \"TODO\"}}]).",
        },
        {
            "type": "verify_done_combo",
            "description": "Reinforce verify+done in same turn",
            "find": "VERIFY+DONE — after making changes, verify the result AND call `done` in the SAME response.",
            "replace": "VERIFY+DONE — after making changes, call read on the edited file AND `done` in the SAME response. NEVER use a separate iteration just to verify. Combine: read(verify) + done(summary) in one turn.",
        },
        {
            "type": "think_skip",
            "description": "Skip think for simple tasks to save iterations",
            "find": "Skip for trivial tasks (typo fix, single-line change).",
            "replace": "Skip for ANY task where the user tells you exactly what to change (typo, rename, one-line fix). Just read the file and edit it.",
        },
        {
            "type": "codebase_context_skip",
            "description": "Don't require codebase_context for small repos",
            "find": "call `codebase_context` first to understand the project structure before editing.",
            "replace": "call `codebase_context` for repos with 50+ files. For small repos (< 20 files), use grep/glob instead — faster.",
        },
        {
            "type": "done_without_test",
            "description": "Don't run tests when not required",
            "find": "Only call `done` when the project compiles and tests pass.",
            "replace": "Call `done` when your change is correct. If the project has no test suite or build system, call `done` after verifying the edit visually.",
        },
    ]

    # Pick the mutation for this iteration (round-robin through bank)
    if not mutations:
        idx = iteration % len(mutation_bank)
        mutations.append(mutation_bank[idx])

    return mutations[0] if mutations else {"type": "none", "description": "no mutation needed"}


def apply_mutation(mutation: dict) -> bool:
    """Apply a prompt mutation to config.rs. Returns True if applied."""
    if mutation.get("type") == "none":
        return False

    if "find" in mutation and "replace" in mutation:
        text = PROMPT_PATH.read_text()
        if mutation["find"] in text:
            new_text = text.replace(mutation["find"], mutation["replace"], 1)
            PROMPT_PATH.write_text(new_text)
            # Rebuild
            result = subprocess.run(
                ["cargo", "build", "-p", "theo", "--release"],
                capture_output=True, text=True, timeout=300,
                cwd=ROOT,
            )
            return result.returncode == 0
    return False


def revert_mutation(mutation: dict) -> bool:
    """Revert a previously applied mutation."""
    if "find" in mutation and "replace" in mutation:
        text = PROMPT_PATH.read_text()
        if mutation["replace"] in text:
            new_text = text.replace(mutation["replace"], mutation["find"], 1)
            PROMPT_PATH.write_text(new_text)
            subprocess.run(
                ["cargo", "build", "-p", "theo", "--release"],
                capture_output=True, text=True, timeout=300,
                cwd=ROOT,
            )
            return True
    return False


def log_iteration(iteration: int, data: dict) -> None:
    """Append iteration data to evolve log."""
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    data["iteration"] = iteration
    data["timestamp"] = int(time.time())
    with EVOLVE_LOG.open("a") as f:
        f.write(json.dumps(data) + "\n")


def print_comparison(before: float, after: float, mutation: dict) -> None:
    delta = after - before
    symbol = "↑" if delta > 0 else ("↓" if delta < 0 else "=")
    decision = "ACCEPT" if delta > 0 else ("REVERT" if delta < 0 else "SKIP")
    print(f"\n  [{decision}] {mutation.get('type', '?')}: {mutation.get('description', '?')}")
    print(f"         score: {before:.1f} → {after:.1f} ({symbol}{abs(delta):.1f})")


def main() -> int:
    ap = argparse.ArgumentParser(description="Prompt evolution loop")
    ap.add_argument("--iterations", type=int, default=3, help="evolution iterations")
    ap.add_argument("--target", choices=["smoke", "swe", "both"], default="smoke")
    ap.add_argument("--swe-filter", default="requests", help="SWE repo filter")
    ap.add_argument("--swe-limit", type=int, default=3)
    args = ap.parse_args()

    if not THEO_BIN.exists():
        print("ERROR: build theo first", file=sys.stderr)
        return 2

    print(f"=== EVOLVE LOOP: {args.iterations} iterations, target={args.target} ===\n")

    # Baseline
    print("  [baseline] Running evaluation...", flush=True)
    smoke_report = run_smoke() if args.target in ("smoke", "both") else {"results": []}
    smoke_analysis = analyze_smoke(smoke_report)

    swe_report = None
    swe_analysis = None
    if args.target in ("swe", "both"):
        swe_report = run_swe(args.swe_filter, args.swe_limit)
        swe_analysis = analyze_swe(swe_report)

    baseline_score = compute_score(smoke_analysis, swe_analysis)
    best_score = baseline_score

    print(f"  [baseline] score={baseline_score:.1f} "
          f"smoke={smoke_analysis['pass_rate']*100:.0f}% "
          f"avg_iter={smoke_analysis['avg_iterations']:.0f}")
    if swe_analysis:
        print(f"             swe={swe_analysis['patch_rate']*100:.0f}% "
              f"avg_iter={swe_analysis['avg_iterations']:.0f}")

    log_iteration(0, {
        "phase": "baseline",
        "score": baseline_score,
        "smoke": smoke_analysis,
        "swe": swe_analysis,
    })

    accepted = 0
    reverted = 0

    for i in range(1, args.iterations + 1):
        print(f"\n  [iter {i}/{args.iterations}] Generating mutation...", flush=True)

        mutation = generate_mutation(smoke_analysis, swe_analysis, i)
        print(f"  [iter {i}] {mutation['type']}: {mutation.get('description', '?')}")

        applied = apply_mutation(mutation)
        if not applied:
            print(f"  [iter {i}] Could not apply mutation — skipping")
            log_iteration(i, {"phase": "skip", "mutation": mutation})
            continue

        print(f"  [iter {i}] Running evaluation...", flush=True)

        new_smoke = run_smoke() if args.target in ("smoke", "both") else {"results": []}
        new_smoke_analysis = analyze_smoke(new_smoke)
        new_swe_analysis = None
        if args.target in ("swe", "both"):
            new_swe = run_swe(args.swe_filter, args.swe_limit)
            new_swe_analysis = analyze_swe(new_swe)

        new_score = compute_score(new_smoke_analysis, new_swe_analysis)

        print_comparison(best_score, new_score, mutation)

        if new_score > best_score:
            best_score = new_score
            smoke_analysis = new_smoke_analysis
            swe_analysis = new_swe_analysis
            accepted += 1
            log_iteration(i, {
                "phase": "accept",
                "mutation": mutation,
                "score": new_score,
                "delta": new_score - best_score,
                "smoke": new_smoke_analysis,
                "swe": new_swe_analysis,
            })
        else:
            revert_mutation(mutation)
            reverted += 1
            log_iteration(i, {
                "phase": "revert",
                "mutation": mutation,
                "score": new_score,
                "delta": new_score - best_score,
            })

    print(f"\n{'='*60}")
    print(f"  EVOLUTION COMPLETE")
    print(f"  baseline: {baseline_score:.1f} → best: {best_score:.1f} "
          f"(Δ{best_score - baseline_score:+.1f})")
    print(f"  accepted: {accepted}, reverted: {reverted}")
    print(f"  log: {EVOLVE_LOG}")
    print(f"{'='*60}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
