#!/usr/bin/env bash
# Phase 49 — SWE-bench Lite via swe/adapter.py.
# Patch generation requires only theo + datasets. Grading optional (Docker).

set -uo pipefail

REPORT_DIR="${1:?usage: $0 <report-dir>}"
BENCH_OUT="$REPORT_DIR/swebench-lite"
mkdir -p "$BENCH_OUT"

REPO_ROOT="${REPO_ROOT:-/opt/theo-code}"
BENCH_DIR="$REPO_ROOT/apps/theo-benchmark"

VENV=/opt/theo-bench-venv
source "$VENV/bin/activate"
export PYTHONPATH="$BENCH_DIR:${PYTHONPATH:-}"

LIMIT="${SWEBENCH_LIMIT:-300}"

echo "[swebench-lite] generating patches (limit=$LIMIT)..."
python3 "$BENCH_DIR/swe/adapter.py" \
  --dataset lite \
  --limit "$LIMIT" \
  --report "$BENCH_OUT/patches.json" 2>&1 | tee "$BENCH_OUT/patches.log"

# Optional grading
if python3 -c "import swebench" 2>/dev/null && command -v docker >/dev/null; then
  echo "[swebench-lite] running official grader..."
  python3 "$BENCH_DIR/swe/adapter.py" \
    --dataset lite \
    --grade \
    --predictions "$BENCH_OUT/patches.json" \
    --report "$BENCH_OUT/graded.json" 2>&1 | tee -a "$BENCH_OUT/grader.log" || \
    echo "[swebench-lite] grader had failures — patch results still recorded"
else
  echo "[swebench-lite] swebench package or docker absent — skipping grader"
fi

# Convert to per-task records (one .json per instance) for aggregator
python3 "$BENCH_DIR/analysis/swe_post.py" \
  --report "$BENCH_OUT/patches.json" \
  --graded "$BENCH_OUT/graded.json" \
  --output-dir "$BENCH_OUT" 2>&1 | tail -10
