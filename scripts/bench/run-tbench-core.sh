#!/usr/bin/env bash
# Phase 48 — Terminal-Bench Core via tb harness.

set -uo pipefail

REPORT_DIR="${1:?usage: $0 <report-dir>}"
BENCH_OUT="$REPORT_DIR/tbench-core"
mkdir -p "$BENCH_OUT/raw"

REPO_ROOT="${REPO_ROOT:-/opt/theo-code}"
BENCH_DIR="$REPO_ROOT/apps/theo-benchmark"

VENV=/opt/theo-bench-venv
TB="$VENV/bin/tb"

[ -x "$TB" ] || { echo "tb not found at $TB"; exit 1; }
command -v docker >/dev/null || { echo "docker required"; exit 1; }
command -v theo >/dev/null || { echo "theo binary required"; exit 1; }

# Activate venv so the agent-import-path resolution sees tbench.agent
source "$VENV/bin/activate"
export PYTHONPATH="$BENCH_DIR:${PYTHONPATH:-}"

N_CONCURRENT="${N_CONCURRENT:-4}"
K_ATTEMPTS="${K_ATTEMPTS:-1}"

echo "[tbench-core] starting — n_concurrent=$N_CONCURRENT k=$K_ATTEMPTS"

# tb run; the heredoc-style env passes OTLP, model, etc. into the
# container via the TheoAgent._env property.
"$TB" run \
  --dataset-name terminal-bench-core --dataset-version head \
  --agent-import-path tbench.agent:TheoAgent \
  --n-concurrent "$N_CONCURRENT" \
  -k "$K_ATTEMPTS" \
  --output-path "$BENCH_OUT/raw" \
  2>&1 | tee "$BENCH_OUT/run.log"

# Post-process tb output → per-task analyses
python3 "$BENCH_DIR/analysis/tbench_post.py" \
  --raw-dir "$BENCH_OUT/raw" \
  --output-dir "$BENCH_OUT" \
  --bench-name "tbench-core" 2>&1 | tail -10
