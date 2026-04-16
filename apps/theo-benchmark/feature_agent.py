#!/usr/bin/env python3
"""
Feature Agent — Executes complex multi-step features using Task Engine + Headless Agent.

Solves the O(N²) task explosion problem by:
1. Breaking features into tasks (Graph + Templates + LLM fallback)
2. Executing each task via `theo --headless` (Rust binary)
3. Rolling back failed tasks via undo stack (no git)
4. Maintaining context stack across task boundaries

Usage:
    python3 feature_agent.py --repo <path> --feature "<description>"
"""

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
from _headless import make_agent_fn, _resolve_bin
from task_engine import TaskEngine, TaskStatus
from decompose import decompose, TaskType


def main():
    parser = argparse.ArgumentParser(description="Feature Agent — complex multi-step features")
    parser.add_argument("--repo", required=True, help="Path to the repository")
    parser.add_argument("--feature", required=True, help="Feature description")
    parser.add_argument("--max-iter", type=int, default=30, help="Max iterations per task")
    parser.add_argument("--timeout", type=int, default=600, help="Timeout per task in seconds")
    args = parser.parse_args()

    repo_path = args.repo
    theo_bin = _resolve_bin()

    if not theo_bin.exists():
        print(f"ERROR: theo binary not found at {theo_bin}")
        print("Build with: cargo build -p theo --release")
        sys.exit(2)

    print(f"\n{'='*60}")
    print(f"FEATURE AGENT — Complex Multi-Step Execution")
    print(f"{'='*60}")
    print(f"Repo:    {repo_path}")
    print(f"Binary:  {theo_bin}")
    print(f"Feature: {args.feature[:80]}...")
    print()

    # Step 1: Hybrid decomposition (Graph + Templates + LLM fallback)
    theo_code_bin = os.environ.get("THEO_CODE_BIN", str(theo_bin))
    vllm_url = os.environ.get("VLLM_URL", "http://localhost:8000")
    model_name = os.environ.get("MODEL_NAME", "")

    print("[1] Decomposing feature (Graph + Templates + LLM fallback)...")
    task_specs, intent, analysis, source = decompose(
        args.feature, repo_path, theo_code_bin, vllm_url, model_name
    )
    print(f"    Intent:  {intent.value}")
    print(f"    Source:  {source} ({'no LLM needed' if 'template' in source else 'LLM used'})")
    print(f"    Risk:    {analysis.risk_level}")
    print(f"    Files:   {len(analysis.affected_files)} affected")
    print(f"    Tasks:   {len(task_specs)}")
    for t in task_specs:
        deps = f" (after: {', '.join(t.depends_on)})" if t.depends_on else ""
        files = f" -> {', '.join(t.target_files[:2])}" if t.target_files else ""
        print(f"    [{t.id}] [{t.risk}] {t.description[:55]}...{deps}{files}")
    tasks = [{"id": t.id, "description": t.description} for t in task_specs]
    print()

    # Step 2: Initialize Task Engine
    print("[2] Initializing Task Engine (circuit breaker + undo stack)...")
    engine = TaskEngine(repo_path, theo_code_bin)

    # Step 3: Execute tasks via theo --headless
    print("[3] Executing tasks via theo --headless...\n")

    agent_fn = make_agent_fn(
        repo_path,
        max_iter=args.max_iter,
        timeout=args.timeout,
        theo_bin=theo_bin,
    )

    result = engine.execute_spec(tasks, agent_fn)

    # Step 4: Summary
    print(f"\n{'='*60}")
    print(f"FEATURE EXECUTION COMPLETE")
    print(f"{'='*60}")
    print(f"Result: {result['summary']}")
    print(f"Circuit breaker tripped: {result['circuit_breaker_tripped']}")
    print(f"Failed tasks: {result.get('failed', 0)}")
    print()

    for tid, r in result["tasks"].items():
        icons = {"done": "OK", "blocked": "BLOCKED", "skipped": "SKIP", "failed": "FAIL"}
        status_str = icons.get(r["status"], "?")
        print(f"  [{status_str:>7}] {tid}: {r['status']}")
        if r["error"]:
            print(f"           Error: {r['error'][:100]}")
        if r["result"]:
            print(f"           Result: {r['result'][:100]}")

    # Save results
    output_path = os.path.join(os.path.dirname(__file__), "feature_results.json")
    with open(output_path, "w") as f:
        json.dump(result, f, indent=2, default=str)
    print(f"\nResults saved to: {output_path}")


if __name__ == "__main__":
    main()
