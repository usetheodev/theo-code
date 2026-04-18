# Theo Benchmark

Evaluation harness for the **Theo Code** AI coding agent. Measures autonomous task
completion across multiple tiers of difficulty — from single-file fixes to real
GitHub issues with hundreds of files.

```
                         ┌──────────────────┐
                         │   theo --headless │  ← Rust binary (the agent)
                         └────────┬─────────┘
                                  │
                           _headless.py        ← Thin wrapper: invoke, parse, retry
                                  │
          ┌───────────┬───────────┼───────────┬──────────┐
          │           │           │           │          │
      smoke.py    adapter.py  harness.py  tbench/   feature_agent.py
      (Phase 0)   (Phase 2a)  (Phase 2b)  (Phase 1)  (Multi-task)
```

**Principle:** Python does orchestration only. The Rust binary owns the full agent
lifecycle — tools, LLM calls, state machine, context engineering. Zero Python
reimplementation of agent logic.

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Quick Start](#quick-start)
4. [Benchmarks](#benchmarks)
   - [Phase 0: Smoke Tests](#phase-0-smoke-tests-20-scenarios)
   - [Phase 1: Terminal-Bench](#phase-1-terminal-bench)
   - [Phase 2: SWE-bench](#phase-2-swe-bench)
   - [GRAPHCTX Validation](#graphctx-validation)
   - [Feature Agent](#feature-agent)
5. [Writing Scenarios](#writing-smoke-scenarios)
6. [Report Format](#report-format)
7. [Reproducibility Guide](#reproducibility-guide)
8. [Cost Tracking](#cost-tracking)
9. [Running Tests](#running-tests)
10. [Project Structure](#project-structure)
11. [Hardware Requirements](#hardware-requirements)

---

## Prerequisites

| Requirement | Version | Required For |
|---|---|---|
| Python | >= 3.10 | All |
| Rust toolchain | stable | Building `theo` binary |
| Git | any | All (clones repos) |
| Docker | >= 24.0 | SWE-bench official grader, Terminal-Bench |
| API key | OpenAI or Anthropic | LLM inference (unless self-hosting) |

## Installation

### 1. Build the agent binary

```bash
# From the repo root
cargo build -p theo --release

# Verify
./target/release/theo --version
```

### 2. Install Python dependencies

```bash
cd apps/theo-benchmark

# Core dependencies
pip install -e .

# Development (adds pytest)
pip install -e '.[dev]'

# SWE-bench official grader (adds swebench + docker SDK)
pip install -e '.[swe-grader]'
```

### 3. Configure the LLM provider

The agent reads its LLM config from environment variables or `.theo/config.toml`.
You need at least one provider configured:

```bash
# Option A: OpenAI API
export OPENAI_API_KEY="sk-..."

# Option B: Anthropic API
export ANTHROPIC_API_KEY="sk-ant-..."

# Option C: Self-hosted vLLM (e.g., Qwen, DeepSeek)
# Configure in .theo/config.toml or via THEO_MODEL + provider setup
```

### 4. Verify everything works

```bash
# Unit tests (no binary or API needed)
python -m pytest tests/ -v

# Smoke test (needs binary + API)
python runner/smoke.py --filter 01
```

---

## Quick Start

```bash
# Run all 20 smoke scenarios
python runner/smoke.py

# Run 5 SWE-bench Lite instances (generates patches)
python swe/adapter.py --limit 5

# Run 5 SWE-bench + official grading (requires Docker)
python swe/adapter.py --limit 5 --grade
```

---

## Benchmarks

### Phase 0: Smoke Tests (20 scenarios)

Quick validation of core agent capabilities in isolated environments.
Each scenario creates a temporary git repo with predefined files, runs
`theo --headless`, and checks the result with a bash assertion.

**Categories:**

| Category | Scenarios | What it tests |
|---|---|---|
| `read` | 01, 15 | File comprehension, cross-file search |
| `search` | 02 | Grep/pattern matching |
| `fix-bug` | 03, 04, 10, 11, 13, 16, 17, 19, 20 | Typos, logic bugs, cross-file bugs, off-by-one, inheritance |
| `create` | 05 | Add new code |
| `rename` | 06 | Refactoring |
| `analyze` | 07 | Codebase analysis |
| `multi-file` | 08, 18 | Edits across 2-3 files |
| `plan` | 09 | Plan-mode structured output |
| `bash` | 12 | Shell command execution |
| `imports` | 14 | Dependency management |

**Run:**

```bash
# All scenarios
python runner/smoke.py

# Single scenario
python runner/smoke.py --filter 16

# Keep temp directories for debugging
python runner/smoke.py --keep-tmp

# Custom binary path
python runner/smoke.py --bin /path/to/theo

# Custom output path
python runner/smoke.py --report my-results.json
```

**Output:** `reports/smoke-<timestamp>.json`

**Environment variable:** `THEO_BIN` overrides the binary path.

---

### Phase 1: Terminal-Bench

Real terminal tasks inside Docker containers via the
[Harbor](https://harborframework.com/) framework. Each task has an instruction,
a Docker environment, and a verification script.

**Run:**

```bash
# Install Harbor CLI
pip install terminal-bench

# Run evaluation
tb run --agent-import-path tbench.agent:TheoAgent \
       --dataset-name terminal-bench-core --dataset-version 0.1.1

# With parallelism
tb run --agent-import-path tbench.agent:TheoAgent \
       --dataset-name terminal-bench-core --dataset-version 0.1.1 \
       --n-concurrent 8
```

**Requirements:** Docker running, Harbor CLI installed.

---

### Phase 2: SWE-bench

Industry-standard benchmark: real GitHub issues from open-source projects
(Django, Flask, Requests, scikit-learn, etc.). Two tools are available:

#### `swe/adapter.py` — Patch Generation + Official Grading (recommended)

Generates patches via `theo --headless` and optionally evaluates them with
the official Princeton SWE-bench grader.

```bash
# Quick smoke (5 instances, patch generation only)
python swe/adapter.py --limit 5

# Full Lite (300 instances)
python swe/adapter.py --dataset lite

# Full Verified (500 instances)
python swe/adapter.py --dataset verified

# With official Docker grader (REQUIRED for publishable results)
python swe/adapter.py --dataset lite --grade

# Filter by repository
python swe/adapter.py --dataset lite --filter django --limit 20

# Deterministic (temperature=0)
python swe/adapter.py --dataset lite --temperature 0.0

# Non-oracle mode (don't show failing test names to agent)
python swe/adapter.py --dataset lite --no-oracle

# Resume after interruption
python swe/adapter.py --dataset lite --resume

# Custom binary
python swe/adapter.py --bin /path/to/theo
```

**Output:**
- `reports/swe-lite-<timestamp>.json` — Full report with metrics
- `reports/swe-lite-<timestamp>.jsonl` — SWE-bench submission format

**Oracle vs Non-Oracle:**

| Mode | Flag | What agent sees | Use for |
|---|---|---|---|
| Oracle | (default) | Issue + `FAIL_TO_PASS` test names | Development, iteration |
| Non-Oracle | `--no-oracle` | Issue only | Fair comparison with other agents |

If you publish results, disclose which mode was used.

#### `swe_bench_harness.py` — Patch Generation + Local Test Execution

Alternative harness that clones repos, runs the agent, applies the gold test
patch, and executes tests locally (without Docker). Useful for fast iteration
but **not authoritative** — use `adapter.py --grade` for publishable results.

```bash
python swe_bench_harness.py --limit 10
python swe_bench_harness.py --filter django --timeout 900
python swe_bench_harness.py --resume
```

---

### GRAPHCTX Validation

Compares LLM performance WITH vs WITHOUT GRAPHCTX context engineering.
Not a standalone benchmark — validates that GRAPHCTX improves task completion.

```bash
# Requires a running vLLM server
VLLM_URL=http://localhost:8000 python run_benchmark.py
```

**Output:** `results.json`

---

### Feature Agent

Executes complex multi-step features by decomposing them into subtasks.
Uses the task engine with circuit breaker protection.

```bash
python feature_agent.py \
  --repo /path/to/project \
  --feature "Add rate limiting middleware with configurable per-IP limits"
```

**Output:** `feature_results.json`

---

## Writing Smoke Scenarios

Scenarios are TOML files in `scenarios/smoke/`. Each file defines one test:

```toml
id = "21-my-scenario"
category = "fix-bug"
description = "Human-readable description for reports"
prompt = "The exact instruction sent to theo --headless"
mode = "agent"           # agent, plan, or ask
timeout_secs = 120

# Bash script that verifies the result.
# Exit 0 = pass, non-zero = fail.
# $THEO_SUMMARY contains the agent's summary text.
success_check = '''
python3 -c "
from src.main import my_function
assert my_function(42) == 84
print('OK')
"
'''

# Files created in the temp repo before the agent runs.
[[setup_files]]
path = "src/main.py"
content = """
def my_function(x):
    return x + x  # BUG: should be x * 2... wait, that's the same thing
"""
```

**Guidelines:**
- `success_check` must be deterministic — no network calls, no randomness
- Use `$THEO_SUMMARY` for answer-based checks (grep the agent's summary)
- Use Python assertions for code-based checks (import and call the code)
- Each scenario is isolated in its own temp directory with a fresh git repo

---

## Report Format

### Smoke (`theo.smoke.v1`)

```json
{
  "schema": "theo.smoke.v1",
  "scenarios_total": 20,
  "scenarios_passed": 18,
  "pass_rate": 0.9,
  "totals": {
    "input_tokens": 50000,
    "output_tokens": 5000,
    "iterations": 120,
    "tool_calls": 200,
    "llm_calls": 95,
    "retries": 2,
    "duration_ms": 300000
  },
  "by_category": { "fix-bug": { "total": 9, "passed": 8 }, ... },
  "results": [ ... per-scenario details ... ]
}
```

### SWE-bench (`theo.swe.v2`)

```json
{
  "schema": "theo.swe.v2",
  "dataset": "lite",
  "total": 300,
  "with_patch": 250,
  "errors": 5,
  "temperature": 0.0,
  "oracle_mode": true,
  "total_cost_usd": 15.30,
  "grader": {
    "resolved": 96,
    "applied": 240,
    "error": 10
  },
  "results": [
    {
      "instance_id": "django__django-12345",
      "model_patch": "diff --git a/...",
      "has_patch": true,
      "duration_secs": 180.5,
      "headless": {
        "success": true,
        "iterations": 12,
        "tokens": { "input": 15000, "output": 2000, "total": 17000 },
        "cost_usd": 0.05,
        "model": "gpt-4o"
      }
    }
  ]
}
```

---

## Reproducibility Guide

For results you want to publish or compare:

| Step | How | Why |
|---|---|---|
| Fix temperature | `--temperature 0.0` | Deterministic sampling |
| Pin binary version | Record `theo --version` + git commit | Binary changes affect results |
| Pin model | Record exact model name from report | Different model = different results |
| Use official grader | `--grade` flag | Custom grading is not comparable |
| Multiple runs | Run 3+ times per configuration | Single runs have high variance |
| Disclose oracle mode | Note if `FAIL_TO_PASS` was included | Oracle mode inflates scores |
| Record cost | Included automatically in reports | Enables cost-efficiency comparison |

**Example of a reproducible run:**

```bash
# Record the binary
theo --version
git rev-parse HEAD

# Run with all controls
python swe/adapter.py \
  --dataset lite \
  --temperature 0.0 \
  --grade \
  --report reports/swe-lite-v1-run1.json

# Run again for confidence
python swe/adapter.py \
  --dataset lite \
  --temperature 0.0 \
  --grade \
  --report reports/swe-lite-v1-run2.json

python swe/adapter.py \
  --dataset lite \
  --temperature 0.0 \
  --grade \
  --report reports/swe-lite-v1-run3.json
```

The `_headless.py` module also provides `run_headless_multi()` which runs the
same task N times and computes:
- Mean and standard deviation for iterations, duration, tokens
- Wilson score 95% confidence interval for success rate
- Total cost across all runs

---

## Cost Tracking

Every run automatically estimates cost based on model and token usage.
Pricing is defined in `_headless.py:MODEL_PRICING`.

| Model | Input ($/1M tok) | Output ($/1M tok) |
|---|---|---|
| gpt-4o | $2.50 | $10.00 |
| gpt-4.1-mini | $0.40 | $1.60 |
| claude-sonnet-4-5 | $3.00 | $15.00 |
| claude-opus-4 | $15.00 | $75.00 |
| qwen / deepseek / llama | $0.00 | $0.00 (self-hosted) |

Self-hosted models report $0 API cost — compute cost is tracked separately.

Reports include `cost_usd` per task and `total_cost_usd` for the entire run.

---

## Running Tests

```bash
# All 70 unit tests (fast, no binary or API needed)
python -m pytest tests/ -v

# Specific test file
python -m pytest tests/test_headless.py -v

# With output
python -m pytest tests/ -v -s
```

**What's tested:**

| Module | Tests | Covers |
|---|---|---|
| `test_headless.py` | 25 | JSON parsing, cost estimation, Wilson CI, std, error handling |
| `test_swe_harness.py` | 12 | pytest/Django output parsing, diff extraction, task filtering |
| `test_decompose.py` | 12 | Intent classification, template generation, dependency ordering |
| `test_task_engine.py` | 21 | Circuit breaker, checkpoint rollback, context stack, task execution |

---

## Project Structure

```
apps/theo-benchmark/
│
│   # Core infrastructure
├── _headless.py            # Invoke theo --headless, parse JSON, retry, cost, multi-run
├── pyproject.toml          # Dependencies and pytest config
├── README.md               # This file
│
│   # Benchmarks
├── runner/
│   ├── smoke.py            # Phase 0: Smoke test runner
│   └── evolve.py           # Prompt mutation/optimization loop
├── swe/
│   ├── adapter.py          # Phase 2: SWE-bench (patches + official grader)
│   ├── local_runner.py     # Local SWE-bench runner
│   └── local_instances.json
├── tbench/
│   ├── agent.py            # Phase 1: Harbor/Terminal-Bench adapter
│   ├── setup.sh            # Container binary installation
│   └── __init__.py
├── swe_bench_harness.py    # Phase 2 alt: SWE-bench with local test execution
├── run_benchmark.py        # GRAPHCTX value validation
├── loop_benchmark.sh       # Build -> benchmark -> analyze loop
│
│   # Task orchestration
├── decompose.py            # Hybrid decomposer (Graph + Templates + LLM fallback)
├── task_engine.py          # Multi-task engine with circuit breaker
├── feature_agent.py        # Complex multi-step feature execution
│
│   # Scenarios and results
├── scenarios/
│   └── smoke/              # 20 TOML scenario definitions
│       ├── 01-read-answer.toml
│       ├── ...
│       └── 20-class-inheritance-bug.toml
├── reports/                # JSON result files (gitignored bulk)
├── results.json            # GRAPHCTX benchmark results
├── VALIDATION_LOG.md       # Manual validation notes
│
│   # Tests
└── tests/
    ├── test_headless.py
    ├── test_swe_harness.py
    ├── test_decompose.py
    └── test_task_engine.py
```

---

## Hardware Requirements

### Minimum (smoke tests + SWE-bench via API)

| Resource | Requirement |
|---|---|
| CPU | 4 cores |
| RAM | 4 GB |
| Disk | 50 GB (for cloned repos) |
| GPU | None |
| Network | Required (API calls) |
| Docker | Optional (for `--grade`) |

### For Official SWE-bench Grading

Same as above, plus:
- **Docker** installed and running
- **60+ GB disk** (Docker images for each repo version)
- **Stable network** (pulls images on first run)

### For Self-Hosted LLM (vLLM)

| Model | GPU VRAM | RAM |
|---|---|---|
| Qwen3-30B (AWQ 4-bit) | 1x 24 GB (RTX 4090) | 16 GB |
| Qwen3-30B (FP8) | 1x 40 GB (A100) | 32 GB |
| DeepSeek-V3 | 2-4x 80 GB (H100) | 64 GB |

The benchmark machine does NOT need a GPU if using a remote vLLM server or
cloud API. Only the inference server needs GPU.
