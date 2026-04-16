# Theo Benchmark

Evaluation harness for the [Theo Code](../../) AI coding agent. Measures autonomous task completion across three tiers of complexity.

## Architecture

```
theo --headless    <-- Rust binary (the agent under test)
       |
  _headless.py     <-- Thin Python wrapper (invoke + parse JSON)
       |
  +-----------+-----------+-----------+
  |           |           |           |
smoke.py   adapter.py  harness.py  tbench/
(Phase 0)  (Phase 2)   (Phase 2)   (Phase 1)
```

**Every benchmark invokes the Rust binary.** No Python reimplementation of the agent loop exists. Python handles only orchestration, dataset loading, and result aggregation.

## Setup

```bash
# Build the agent binary
cargo build -p theo --release

# Install Python dependencies
cd apps/theo-benchmark
pip install -e .              # core deps
pip install -e '.[dev]'       # + pytest
pip install -e '.[swe-grader]'  # + official SWE-bench Docker grader

# Verify
python -m pytest tests/ -v
```

## Benchmarks

### Phase 0: Smoke Tests (15 scenarios)

Quick validation that basic agent capabilities work: read files, fix typos, add functions, multi-file edits.

```bash
python runner/smoke.py
python runner/smoke.py --filter 03        # single scenario
python runner/smoke.py --keep-tmp         # inspect workdirs
```

**Output:** `reports/smoke-<timestamp>.json`

**What it measures:** Pass rate on micro-tasks (2-20 line files). Verifies plumbing, not intelligence.

### Phase 1: Terminal-Bench (via Harbor)

Real terminal tasks in Docker containers. Requires [Harbor](https://harborframework.com/) CLI.

```bash
tb run --agent-import-path tbench.agent:TheoAgent \
       --dataset-name terminal-bench-core --dataset-version 0.1.1
```

### Phase 2: SWE-bench Lite / Verified

Real GitHub issues from open-source projects. The industry standard for code agent evaluation.

```bash
# Patch generation only (fast, no Docker)
python swe/adapter.py --dataset lite --limit 10

# With official Princeton grader (requires Docker)
python swe/adapter.py --dataset lite --grade

# Deterministic run (temperature=0, fixed)
python swe/adapter.py --dataset lite --temperature 0.0

# Non-oracle mode (no test names in prompt)
python swe/adapter.py --dataset lite --no-oracle

# Multiple runs for statistical significance
python swe/adapter.py --dataset lite --limit 20 --temperature 0.0

# Resume interrupted run
python swe/adapter.py --dataset lite --resume
```

**Output:** `reports/swe-<dataset>-<timestamp>.json` + `.jsonl` (SWE-bench submission format)

**Official grading** requires: `pip install swebench` + Docker running. Without `--grade`, only patch generation rate is measured (not resolution rate).

### GRAPHCTX Validation (run_benchmark.py)

Compares agent performance WITH vs WITHOUT GRAPHCTX context. Not a standalone benchmark — validates the value of context engineering.

```bash
VLLM_URL=http://localhost:8000 python run_benchmark.py
```

## Reproducibility

For publishable results:

1. **Fix temperature:** `--temperature 0.0` (deterministic sampling)
2. **Run multiple times:** Minimum 3 runs per configuration. Report mean +/- std.
3. **Use official grader:** `--grade` flag for SWE-bench (Docker required)
4. **Pin binary version:** Record `theo --version` and git commit hash
5. **Disclose oracle mode:** Results with `FAIL_TO_PASS` test hints must be reported as "oracle mode"
6. **Record cost:** Reports include `cost_usd` per task (based on model pricing)

### Report schema

```json
{
  "schema": "theo.swe.v2",
  "dataset": "lite",
  "temperature": 0.0,
  "oracle_mode": true,
  "total_cost_usd": 12.50,
  "results": [...]
}
```

## Cost Tracking

Token-based cost estimation is computed automatically from model pricing tables in `_headless.py`. Self-hosted models (Qwen, LLaMA, DeepSeek) are tracked as $0.00 API cost (compute cost is separate).

## Project Structure

```
apps/theo-benchmark/
  _headless.py          # Core: invoke theo --headless, parse results, cost, multi-run
  decompose.py          # Task decomposition (Graph + Templates + LLM fallback)
  task_engine.py        # Multi-task orchestration with circuit breaker
  feature_agent.py      # Complex multi-step feature execution
  swe_bench_harness.py  # SWE-bench eval with test running
  run_benchmark.py      # GRAPHCTX value validation
  loop_benchmark.sh     # Build -> benchmark -> analyze loop
  runner/
    smoke.py            # Smoke test runner
    evolve.py           # Prompt mutation/optimization loop
  swe/
    adapter.py          # SWE-bench adapter (patches + official grader)
    local_runner.py     # Local SWE-bench instances
  tbench/
    agent.py            # Harbor/Terminal-Bench agent adapter
    setup.sh            # Container setup script
  scenarios/smoke/      # 15 TOML scenario definitions
  reports/              # JSON result files
  tests/                # Unit tests (pytest)
  pyproject.toml        # Dependencies and test config
```

## Running Tests

```bash
python -m pytest tests/ -v
```

70 unit tests covering: JSON parsing, cost estimation, statistics (Wilson CI), test output parsing, intent classification, circuit breaker, checkpoint rollback, context stack.
