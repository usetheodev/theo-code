#!/usr/bin/env bash
# Phase 57 (prompt-ab-testing-plan) — A/B prompt orchestrator wrapper.
#
# Runs the same N tasks across multiple prompt variants for paired
# statistical comparison. Designed to run on the bench droplet.
#
# Inputs (env, with defaults):
#   VARIANTS    sota,sota-lean,sota-no-bench
#   N_TASKS     20
#   N_CONCURRENT 4
#   DATASET     terminal-bench-core==0.1.1
#   REPORT_DIR  /opt/theo-code/.theo/bench-data/<date>/reports/ab
#
# Output: <REPORT_DIR>/{manifest.json, <variant>/raw, comparison.md}

set -uo pipefail

REPO_ROOT="${REPO_ROOT:-/opt/theo-code}"
BENCH_DIR="$REPO_ROOT/apps/theo-benchmark"
DATE="$(date -u +%Y-%m-%d)"

REPORT_DIR="${REPORT_DIR:-$REPO_ROOT/.theo/bench-data/$DATE/reports/ab}"
VARIANTS="${VARIANTS:-sota,sota-lean,sota-no-bench}"
N_TASKS="${N_TASKS:-20}"
N_CONCURRENT="${N_CONCURRENT:-4}"
DATASET="${DATASET:-terminal-bench-core==0.1.1}"

VENV=/opt/theo-bench-venv
TB="$VENV/bin/tb"
[ -x "$TB" ] || { echo "[ab] tb not found at $TB"; exit 1; }
command -v docker >/dev/null || { echo "[ab] docker required"; exit 1; }
command -v theo >/dev/null || { echo "[ab] theo binary required"; exit 1; }

# Activate venv so the agent-import-path resolution sees tbench.agent
# shellcheck disable=SC1091
source "$VENV/bin/activate"
export PYTHONPATH="$BENCH_DIR:${PYTHONPATH:-}"

# Bench-mode toggles (mirror run-all.sh). Without these the agent triggers
# the onboarding flow (asks the user "what's your role?") and exits at
# iter 1 instead of doing the task.
export THEO_SKIP_ONBOARDING="${THEO_SKIP_ONBOARDING:-1}"
export THEO_BENCHMARK_MODE="${THEO_BENCHMARK_MODE:-1}"  # bug #3 safety relax
export THEO_MODEL="${THEO_MODEL:-gpt-5.4}"

mkdir -p "$REPORT_DIR"
echo "[ab] variants=$VARIANTS  n_tasks=$N_TASKS  n_concurrent=$N_CONCURRENT"
echo "[ab] dataset=$DATASET  out=$REPORT_DIR"

python3 "$BENCH_DIR/runner/ab_test.py" \
  --variants "$VARIANTS" \
  --n-tasks "$N_TASKS" \
  --n-concurrent "$N_CONCURRENT" \
  --dataset "$DATASET" \
  --tb-bin "$TB" \
  --output-dir "$REPORT_DIR" \
  2>&1 | tee "$REPORT_DIR/ab-test.log"
RC=$?

# Post-process raw → analyzed for each variant (so ab_compare can read it)
IFS=',' read -ra ARR <<<"$VARIANTS"
for v in "${ARR[@]}"; do
  raw="$REPORT_DIR/$v/raw"
  ana="$REPORT_DIR/$v/analyzed"
  if [ -d "$raw" ]; then
    echo "[ab] post-processing variant '$v'..."
    python3 "$BENCH_DIR/analysis/tbench_post.py" \
      --raw-dir "$raw" \
      --output-dir "$ana" \
      --bench-name "ab-$v" 2>&1 | tail -3
  fi
done

# Local statistical analysis happens via:
#   python3 apps/theo-benchmark/runner/ab_compare.py --ab-dir <REPORT_DIR>
# It can run on the droplet too — but the comparison report is small enough
# to pull and view locally.
echo "[ab] orchestration RC=$RC. Run ab_compare.py to produce comparison.md"
exit "$RC"
