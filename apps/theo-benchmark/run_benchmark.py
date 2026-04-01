#!/usr/bin/env python3
"""
GRAPHCTX Benchmark: Context Engineering Validation

Compares LLM performance WITH vs WITHOUT GRAPHCTX context on coding tasks.
Uses Qwen Coder 35B via vLLM OpenAI-compatible API.

Metrics:
  - Task success rate
  - Tokens used (input + output)
  - Number of interactions needed
  - Context coverage (did the LLM have what it needed?)
"""

import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional

import requests

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

VLLM_BASE_URL = os.environ.get("VLLM_URL", "http://localhost:8000")
MODEL_NAME = os.environ.get("MODEL_NAME", "cpatonn/Qwen3-Coder-30B-A3B-Instruct-AWQ-4bit")
THEO_CODE_BIN = os.environ.get("THEO_CODE_BIN", "./target/release/theo-code")
REPO_PATH = os.environ.get("REPO_PATH", ".")
MAX_INTERACTIONS = 10
TOKEN_BUDGET = 16384

# ---------------------------------------------------------------------------
# Data types
# ---------------------------------------------------------------------------

@dataclass
class Task:
    id: str
    description: str
    target_file: str  # The file the LLM should identify/modify
    expected_symbols: list[str]  # Symbols it should find
    difficulty: str  # easy, medium, hard

@dataclass
class InteractionResult:
    interaction_num: int
    input_tokens: int
    output_tokens: int
    found_target: bool
    response_text: str

@dataclass
class BenchmarkResult:
    task_id: str
    mode: str  # "with_graphctx" or "without_graphctx"
    success: bool
    interactions: int
    total_input_tokens: int
    total_output_tokens: int
    total_tokens: int
    found_target_at_interaction: Optional[int]
    elapsed_seconds: float
    details: list[dict] = field(default_factory=list)

# ---------------------------------------------------------------------------
# Tasks — real coding tasks on the theo-code repo
# ---------------------------------------------------------------------------

TASKS = [
    Task(
        id="permission_eval",
        description="Find the code that evaluates permission rules using glob pattern matching. Where is the evaluate() function and how does it work?",
        target_file="crates/core/src/permission.rs",
        expected_symbols=["evaluate", "PermissionRule", "glob_match"],
        difficulty="easy",
    ),
    Task(
        id="bm25_search",
        description="Find the BM25 search implementation. How does the tokenizer work and how are communities scored?",
        target_file="crates/context/src/search.rs",
        expected_symbols=["Bm25Index", "tokenise", "MultiSignalScorer", "bm25", "scorer", "tokenizer", "ScoredCommunity", "Bm25Config"],
        difficulty="medium",
    ),
    Task(
        id="impact_analysis",
        description="Find the impact analysis code that uses BFS to determine which communities are affected when a file is edited. How does it propagate through the graph?",
        target_file="crates/governance/src/impact.rs",
        expected_symbols=["analyze_impact", "bfs_reachable", "ImpactReport"],
        difficulty="medium",
    ),
    Task(
        id="leiden_cluster",
        description="Find the Leiden community detection algorithm. How does the refinement phase guarantee connected communities? What's the difference from Louvain?",
        target_file="crates/graph/src/cluster.rs",
        expected_symbols=["leiden_communities", "refine_partition", "detect_file_communities", "leiden", "modularity", "connected"],
        difficulty="hard",
    ),
    Task(
        id="context_assembly",
        description="Find the context assembly code that packs code into a token budget using a greedy knapsack approach. How does it decide what to include?",
        target_file="crates/context/src/assembly.rs",
        expected_symbols=["assemble_greedy", "assemble_with_code", "ContextPayload", "knapsack", "budget", "density"],
        difficulty="medium",
    ),
    # --- NEW: Diverse task types ---
    Task(
        id="cochange_decay",
        description="How does the temporal decay work for co-change edges in the code graph? What is the half-life and what formula is used?",
        target_file="crates/graph/src/cochange.rs",
        expected_symbols=["temporal_decay", "DEFAULT_LAMBDA", "update_cochanges", "exp", "decay", "half-life", "lambda"],
        difficulty="easy",
    ),
    Task(
        id="escape_hatch",
        description="How does the escape hatch detect context misses? When a file is not in the current context, how does it suggest which communities to expand?",
        target_file="crates/context/src/escape.rs",
        expected_symbols=["ContextMiss", "ContextMembership", "detect_miss", "suggested_expansion", "contains"],
        difficulty="medium",
    ),
    Task(
        id="turboquant",
        description="How does the TurboQuant 2-bit vector quantization work? How does it compute approximate inner products with quantized vectors?",
        target_file="crates/context/src/turboquant.rs",
        expected_symbols=["TurboQuantizer", "QuantizedVector", "quantize", "inner_product", "cosine_similarity", "2-bit", "rotation"],
        difficulty="hard",
    ),
    Task(
        id="bridge_graph",
        description="How does the bridge module convert extracted file data into a CodeGraph? What node types and edge types does it create?",
        target_file="crates/graph/src/bridge.rs",
        expected_symbols=["build_graph", "FileData", "SymbolData", "BridgeStats", "file_node_id", "Contains", "Calls"],
        difficulty="medium",
    ),
    Task(
        id="git_integration",
        description="How does the git module parse git log output to extract co-change information? How does it skip noisy commits?",
        target_file="crates/graph/src/git.rs",
        expected_symbols=["populate_cochanges_from_git", "CoChangeStats", "GitError", "max_files_per_commit", "parse", "commit"],
        difficulty="medium",
    ),
]

# ---------------------------------------------------------------------------
# LLM interaction
# ---------------------------------------------------------------------------

def call_llm(messages: list[dict], max_tokens: int = 2048) -> dict:
    """Call vLLM OpenAI-compatible API."""
    url = f"{VLLM_BASE_URL}/v1/chat/completions"
    payload = {
        "model": MODEL_NAME,
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": 0.1,
    }

    try:
        resp = requests.post(url, json=payload, timeout=120)
        resp.raise_for_status()
        data = resp.json()
        return {
            "content": data["choices"][0]["message"]["content"],
            "input_tokens": data["usage"]["prompt_tokens"],
            "output_tokens": data["usage"]["completion_tokens"],
        }
    except Exception as e:
        return {"content": f"ERROR: {e}", "input_tokens": 0, "output_tokens": 0}

def check_success(response: str, task: Task) -> bool:
    """Check if the LLM found the target file and expected symbols."""
    text = response.lower()
    # Must mention the target file
    file_found = task.target_file.lower().replace("/", " ").split()[-1].replace(".rs", "") in text
    # Must mention at least 2 of the expected symbols
    symbols_found = sum(1 for s in task.expected_symbols if s.lower() in text)
    return file_found and symbols_found >= 2

# ---------------------------------------------------------------------------
# GRAPHCTX context generation
# ---------------------------------------------------------------------------

def get_graphctx_context(task: Task) -> str:
    """Run theo-code to get context for a task."""
    try:
        result = subprocess.run(
            [THEO_CODE_BIN, "context", REPO_PATH, task.description],
            capture_output=True, text=True, timeout=120,
        )
        # Extract just the context items (skip stats header)
        output = result.stdout
        # Find the context section
        lines = output.split("\n")
        context_lines = []
        in_context = False
        for line in lines:
            if line.startswith("--- Item"):
                in_context = True
            if in_context:
                context_lines.append(line)
            if line.startswith("--- Timing"):
                break
        return "\n".join(context_lines)
    except Exception as e:
        return f"Error generating context: {e}"

# ---------------------------------------------------------------------------
# Benchmark execution
# ---------------------------------------------------------------------------

def run_task_with_context(task: Task) -> BenchmarkResult:
    """Run a task WITH GRAPHCTX context."""
    start = time.time()

    # Get GRAPHCTX context
    context = get_graphctx_context(task)

    system_prompt = f"""You are a code assistant analyzing the theo-code Rust project.
You have been given pre-assembled context from the GRAPHCTX system.
Use this context to answer the user's question. Do NOT ask to read files —
the relevant code is already provided below.

=== GRAPHCTX CONTEXT ===
{context}
=== END CONTEXT ==="""

    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": task.description},
    ]

    total_in = 0
    total_out = 0
    details = []
    found_at = None

    for i in range(1, MAX_INTERACTIONS + 1):
        result = call_llm(messages, max_tokens=2048)
        total_in += result["input_tokens"]
        total_out += result["output_tokens"]

        success = check_success(result["content"], task)
        details.append({
            "interaction": i,
            "input_tokens": result["input_tokens"],
            "output_tokens": result["output_tokens"],
            "found_target": success,
        })

        if success:
            found_at = i
            break

        # Follow up
        messages.append({"role": "assistant", "content": result["content"]})
        messages.append({"role": "user", "content": "Can you be more specific? Show me the exact file path and function names."})

    elapsed = time.time() - start

    return BenchmarkResult(
        task_id=task.id,
        mode="with_graphctx",
        success=found_at is not None,
        interactions=found_at or MAX_INTERACTIONS,
        total_input_tokens=total_in,
        total_output_tokens=total_out,
        total_tokens=total_in + total_out,
        found_target_at_interaction=found_at,
        elapsed_seconds=elapsed,
        details=details,
    )

def run_task_without_context(task: Task) -> BenchmarkResult:
    """Run a task WITHOUT any context — LLM must figure it out from scratch."""
    start = time.time()

    system_prompt = """You are a code assistant analyzing a Rust project called theo-code.
The project has these crates: core, graph, context, governance, parser, tools.
You do NOT have access to the source code. Answer based on your understanding
of the project structure and common Rust patterns."""

    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": task.description},
    ]

    total_in = 0
    total_out = 0
    details = []
    found_at = None

    for i in range(1, MAX_INTERACTIONS + 1):
        result = call_llm(messages, max_tokens=2048)
        total_in += result["input_tokens"]
        total_out += result["output_tokens"]

        success = check_success(result["content"], task)
        details.append({
            "interaction": i,
            "input_tokens": result["input_tokens"],
            "output_tokens": result["output_tokens"],
            "found_target": success,
        })

        if success:
            found_at = i
            break

        messages.append({"role": "assistant", "content": result["content"]})
        messages.append({"role": "user", "content": "Can you be more specific? Show me the exact file path and function names."})

    elapsed = time.time() - start

    return BenchmarkResult(
        task_id=task.id,
        mode="without_graphctx",
        success=found_at is not None,
        interactions=found_at or MAX_INTERACTIONS,
        total_input_tokens=total_in,
        total_output_tokens=total_out,
        total_tokens=total_in + total_out,
        found_target_at_interaction=found_at,
        elapsed_seconds=elapsed,
        details=details,
    )

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    print("=" * 60)
    print("GRAPHCTX Benchmark — Context Engineering Validation")
    print("=" * 60)
    print(f"Model: {MODEL_NAME}")
    print(f"API:   {VLLM_BASE_URL}")
    print(f"Repo:  {REPO_PATH}")
    print(f"Tasks: {len(TASKS)}")
    print()

    # Check API is reachable
    try:
        resp = requests.get(f"{VLLM_BASE_URL}/v1/models", timeout=10)
        models = resp.json()
        print(f"API OK. Models: {[m['id'] for m in models['data']]}")
    except Exception as e:
        print(f"ERROR: Cannot reach vLLM at {VLLM_BASE_URL}: {e}")
        print("Set VLLM_URL environment variable to the correct URL.")
        sys.exit(1)

    # Build theo-code if needed
    if not Path(THEO_CODE_BIN).exists():
        print(f"\nBuilding {THEO_CODE_BIN}...")
        subprocess.run(["cargo", "build", "--release"], check=True)

    results = []

    for task in TASKS:
        print(f"\n{'=' * 60}")
        print(f"Task: {task.id} ({task.difficulty})")
        print(f"  {task.description[:80]}...")

        # Run WITHOUT context
        print(f"\n  [WITHOUT GRAPHCTX]")
        r_without = run_task_without_context(task)
        print(f"    Success: {r_without.success}")
        print(f"    Interactions: {r_without.interactions}")
        print(f"    Tokens: {r_without.total_tokens}")
        results.append(r_without)

        # Run WITH context
        print(f"\n  [WITH GRAPHCTX]")
        r_with = run_task_with_context(task)
        print(f"    Success: {r_with.success}")
        print(f"    Interactions: {r_with.interactions}")
        print(f"    Tokens: {r_with.total_tokens}")
        results.append(r_with)

        # Comparison
        if r_with.success and r_without.success:
            token_reduction = 1 - (r_with.total_tokens / max(r_without.total_tokens, 1))
            interaction_reduction = 1 - (r_with.interactions / max(r_without.interactions, 1))
            print(f"\n  Improvement: {token_reduction:.0%} fewer tokens, {interaction_reduction:.0%} fewer interactions")
        elif r_with.success and not r_without.success:
            print(f"\n  GRAPHCTX enabled success where baseline failed!")

    # Summary
    print(f"\n{'=' * 60}")
    print("SUMMARY")
    print(f"{'=' * 60}")

    with_results = [r for r in results if r.mode == "with_graphctx"]
    without_results = [r for r in results if r.mode == "without_graphctx"]

    with_success = sum(1 for r in with_results if r.success)
    without_success = sum(1 for r in without_results if r.success)

    with_tokens = sum(r.total_tokens for r in with_results)
    without_tokens = sum(r.total_tokens for r in without_results)

    with_interactions = sum(r.interactions for r in with_results)
    without_interactions = sum(r.interactions for r in without_results)

    print(f"\n{'Metric':<25} {'Without':>12} {'With GRAPHCTX':>15} {'Improvement':>12}")
    print("-" * 65)
    print(f"{'Success rate':<25} {without_success}/{len(TASKS):>10} {with_success}/{len(TASKS):>13} {'':>12}")
    print(f"{'Total tokens':<25} {without_tokens:>12,} {with_tokens:>15,} {(1-with_tokens/max(without_tokens,1)):>11.0%}")
    print(f"{'Avg interactions':<25} {without_interactions/len(TASKS):>12.1f} {with_interactions/len(TASKS):>15.1f} {(1-with_interactions/max(without_interactions,1)):>11.0%}")

    # Save results
    output_path = "benchmark/results.json"
    os.makedirs("benchmark", exist_ok=True)
    with open(output_path, "w") as f:
        json.dump([asdict(r) for r in results], f, indent=2)
    print(f"\nDetailed results saved to: {output_path}")

if __name__ == "__main__":
    main()
