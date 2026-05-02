#!/usr/bin/env bash
# Phase 50 — Terminal-Bench Pro (200 public tasks).

set -uo pipefail

REPORT_DIR="${1:?usage: $0 <report-dir>}"
BENCH_OUT="$REPORT_DIR/tbench-pro"
mkdir -p "$BENCH_OUT/raw"

REPO_ROOT="${REPO_ROOT:-/opt/theo-code}"
BENCH_DIR="$REPO_ROOT/apps/theo-benchmark"

VENV=/opt/theo-bench-venv
TB="$VENV/bin/tb"

[ -x "$TB" ] || { echo "tb not found at $TB"; exit 1; }
command -v docker >/dev/null || { echo "docker required"; exit 1; }
command -v theo >/dev/null || { echo "theo binary required"; exit 1; }

source "$VENV/bin/activate"
export PYTHONPATH="$BENCH_DIR:${PYTHONPATH:-}"

N_CONCURRENT="${N_CONCURRENT:-4}"
K_ATTEMPTS="${K_ATTEMPTS:-1}"

# Clone terminal-bench-pro if not in registry
PRO_REPO_DIR=/opt/terminal-bench-pro
if [ ! -d "$PRO_REPO_DIR" ]; then
  echo "[tbench-pro] cloning Alibaba terminal-bench-pro..."
  git clone --depth 1 https://github.com/alibaba/terminal-bench-pro "$PRO_REPO_DIR"
fi

echo "[tbench-pro] starting — n_concurrent=$N_CONCURRENT k=$K_ATTEMPTS"

"$TB" run \
  --dataset-path "$PRO_REPO_DIR" \
  --agent-import-path tbench.agent:TheoAgent \
  --n-concurrent "$N_CONCURRENT" \
  -k "$K_ATTEMPTS" \
  --output-path "$BENCH_OUT/raw" \
  2>&1 | tee "$BENCH_OUT/run.log"

python3 "$BENCH_DIR/analysis/tbench_post.py" \
  --raw-dir "$BENCH_OUT/raw" \
  --output-dir "$BENCH_OUT" \
  --bench-name "tbench-pro" 2>&1 | tail -10
