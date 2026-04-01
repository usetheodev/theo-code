#!/usr/bin/env python3
"""
Feature Agent — Executes complex multi-step features using Task Engine + Agent Loop.

Solves the O(N²) task explosion problem by:
1. Breaking features into tasks
2. Executing each task with circuit breaker protection
3. Rolling back failed tasks via undo stack (no git)
4. Maintaining context stack across task boundaries

Usage:
    python3 benchmark/feature_agent.py --repo <path> --feature "<description>" [--vllm-url <url>]
"""

import argparse
import json
import os
import sys
import time

# Import from our modules
sys.path.insert(0, os.path.dirname(__file__))
from agent_loop import run_agent, VLLM_URL, THEO_CODE_BIN
from task_engine import TaskEngine, TaskStatus
from decompose import decompose, TaskType

import requests


def execute_task_with_agent(description: str, context: str, repo_path: str,
                             vllm_url: str, checkpoint_mgr=None) -> tuple[bool, str, str]:
    """Execute a single task using the agent loop. Returns (success, result, error)."""

    # Combine task engine context with the agent's own flow
    full_issue = f"{context}\n\nTASK: {description}"

    result = run_agent(repo_path, full_issue, verbose=True)

    # Record edits in checkpoint manager if available
    if checkpoint_mgr and result.edits_made:
        for edit in result.edits_made:
            checkpoint_mgr.record_edit(
                edit.get("file", ""),
                edit.get("old", ""),
                edit.get("new", "")
            )

    if result.success:
        return True, result.summary, ""
    else:
        return False, "", result.summary or "Task did not complete"


def main():
    parser = argparse.ArgumentParser(description="Feature Agent — complex multi-step features")
    parser.add_argument("--repo", required=True, help="Path to the repository")
    parser.add_argument("--feature", required=True, help="Feature description")
    parser.add_argument("--vllm-url", default=VLLM_URL, help="vLLM API URL")
    parser.add_argument("--model", default=os.environ.get("MODEL_NAME", "cpatonn/Qwen3-Coder-30B-A3B-Instruct-AWQ-4bit"))
    args = parser.parse_args()

    vllm_url = args.vllm_url
    repo_path = args.repo
    model_name = args.model

    # Verify API
    try:
        resp = requests.get(f"{vllm_url}/v1/models", timeout=5)
        models = resp.json()
        print(f"Model: {models['data'][0]['id']}")
    except Exception as e:
        print(f"ERROR: Cannot reach vLLM at {vllm_url}: {e}")
        sys.exit(1)

    print(f"\n{'='*60}")
    print(f"FEATURE AGENT — Complex Multi-Step Execution")
    print(f"{'='*60}")
    print(f"Repo:    {repo_path}")
    print(f"Feature: {args.feature[:80]}...")
    print()

    # Step 1: Hybrid decomposition (Graph + Templates + LLM fallback)
    theo_bin = os.environ.get("THEO_CODE_BIN", THEO_CODE_BIN)
    print("[1] Decomposing feature (Graph + Templates + LLM fallback)...")
    task_specs, intent, analysis, source = decompose(
        args.feature, repo_path, theo_bin, vllm_url, model_name
    )
    print(f"    Intent:  {intent.value}")
    print(f"    Source:  {source} ({'no LLM needed' if 'template' in source else 'LLM used'})")
    print(f"    Risk:    {analysis.risk_level}")
    print(f"    Files:   {len(analysis.affected_files)} affected")
    print(f"    Tasks:   {len(task_specs)}")
    for t in task_specs:
        deps = f" (after: {', '.join(t.depends_on)})" if t.depends_on else ""
        files = f" → {', '.join(t.target_files[:2])}" if t.target_files else ""
        print(f"    [{t.id}] [{t.risk}] {t.description[:55]}...{deps}{files}")
    tasks = [{"id": t.id, "description": t.description} for t in task_specs]
    print()

    # Step 2: Initialize Task Engine
    print("[2] Initializing Task Engine (circuit breaker + undo stack)...")
    engine = TaskEngine(repo_path, os.environ.get("THEO_CODE_BIN", THEO_CODE_BIN))

    # Step 3: Execute tasks
    print("[3] Executing tasks with explosion prevention...\n")

    def agent_fn(description, context):
        return execute_task_with_agent(
            description, context, repo_path, vllm_url,
            engine.checkpoint_mgr
        )

    result = engine.execute_spec(
        [{"id": t["id"], "description": t["description"]} for t in tasks],
        agent_fn
    )

    # Step 4: Summary
    print(f"\n{'='*60}")
    print(f"FEATURE EXECUTION COMPLETE")
    print(f"{'='*60}")
    print(f"Result: {result['summary']}")
    print(f"Circuit breaker tripped: {result['circuit_breaker_tripped']}")
    print(f"Blocked tasks: {result.get('blocked', result.get('failed', 0))}")
    print()

    for tid, r in result["tasks"].items():
        status_icon = {"done": "✅", "blocked": "🔴", "skipped": "⏭", "failed": "❌"}.get(r["status"], "?")
        print(f"  {status_icon} {tid}: {r['status']}")
        if r["error"]:
            print(f"      Error: {r['error'][:100]}")
        if r["result"]:
            print(f"      Result: {r['result'][:100]}")

    # Save results
    output_path = os.path.join(os.path.dirname(__file__), "feature_results.json")
    with open(output_path, "w") as f:
        json.dump(result, f, indent=2, default=str)
    print(f"\nResults saved to: {output_path}")


if __name__ == "__main__":
    main()
