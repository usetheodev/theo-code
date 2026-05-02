#!/usr/bin/env bash
#
# Orchestration entry point for the benchmark suite — Phase 47.
#
# Runs the full sequence smoke → tbench-core → swebench-lite → tbench-pro
# with OTLP capture + cost tracking + comparison.md aggregation.
#
# Usage (run on the droplet):
#   cd /opt/theo-code
#   THEO_MODEL=gpt-5.4 bash scripts/bench/run-all.sh [report-dir]
#
# Env:
#   SKIP_TBENCH_CORE=1   skip Phase 48
#   SKIP_SWEBENCH=1      skip Phase 49
#   SKIP_TBENCH_PRO=1    skip Phase 50
#   THEO_MODEL           model id (default: gpt-5.4)
#   N_CONCURRENT         tasks in parallel (default: 4)
#   K_ATTEMPTS           pass@k (default: 1)

set -uo pipefail

REPO_ROOT="${REPO_ROOT:-/opt/theo-code}"
BENCH_DIR="$REPO_ROOT/apps/theo-benchmark"
DATE="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
REPORT_DIR="${1:-$BENCH_DIR/reports/$DATE}"
mkdir -p "$REPORT_DIR"

THEO_MODEL="${THEO_MODEL:-gpt-5.4}"
N_CONCURRENT="${N_CONCURRENT:-4}"
K_ATTEMPTS="${K_ATTEMPTS:-1}"

abort() {
  echo "[run-all] ABORT: $*" >&2
  exit 1
}

# ── 1. Pre-flight ────────────────────────────────────────────────────
echo "[run-all] $(date -u +%FT%TZ) starting in $REPORT_DIR"
echo "[run-all] model=$THEO_MODEL n_concurrent=$N_CONCURRENT k=$K_ATTEMPTS"

command -v docker >/dev/null || abort "docker not found"
command -v theo >/dev/null || abort "theo binary not in PATH"
[ -f /root/.config/theo/auth.json ] || [ -n "${OPENAI_API_KEY:-}" ] || \
  abort "no auth.json + no OPENAI_API_KEY — set credential first"

# Manifest — pin everything for reproducibility
THEO_SHA="$(cd "$REPO_ROOT" && git rev-parse --short HEAD)"
cat > "$REPORT_DIR/manifest.json" <<EOF
{
  "date": "$DATE",
  "theo_sha": "$THEO_SHA",
  "model": "$THEO_MODEL",
  "n_concurrent": $N_CONCURRENT,
  "k_attempts": $K_ATTEMPTS,
  "host": "$(hostname)",
  "kernel": "$(uname -r)",
  "cores": $(nproc),
  "ram_gb": $(awk '/MemTotal/ {printf "%.1f", $2/1024/1024}' /proc/meminfo)
}
EOF
echo "[run-all] manifest written"

# ── 2. Sobe collector OTel ───────────────────────────────────────────
echo "[run-all] starting OTel collector..."
docker compose -f "$BENCH_DIR/otlp/docker-compose.yml" up -d
trap "echo '[run-all] tearing down collector'; docker compose -f $BENCH_DIR/otlp/docker-compose.yml down" EXIT
sleep 5

export OTLP_ENDPOINT="http://172.17.0.1:4317"  # bridge-network host alias
export OTLP_PROTOCOL="grpc"
export OTLP_SERVICE_NAME="theo-bench-$DATE"
export THEO_MODEL
export THEO_SKIP_ONBOARDING=1

# ── 3. Smoke gate (Phase 46) ─────────────────────────────────────────
echo "[run-all] smoke 5-task gate..."
if ! THEO_BIN=/usr/local/bin/theo python3 "$BENCH_DIR/runner/smoke.py" \
       --filter 01,02,03,04,05 \
       --report "$REPORT_DIR/smoke.json" 2>&1 | tee -a "$REPORT_DIR/smoke.log"; then
  abort "smoke gate failed — see $REPORT_DIR/smoke.log"
fi
echo "[run-all] smoke OK"

# ── 4. Phase 48 — Terminal-Bench Core ────────────────────────────────
if [ "${SKIP_TBENCH_CORE:-0}" = "1" ]; then
  echo "[run-all] SKIP tbench-core"
else
  echo "[run-all] running Terminal-Bench Core..."
  bash "$REPO_ROOT/scripts/bench/run-tbench-core.sh" "$REPORT_DIR" \
    2>&1 | tee "$REPORT_DIR/tbench-core.log" || \
    echo "[run-all] tbench-core had failures — continuing"
fi

# ── 5. Phase 49 — SWE-bench Lite ─────────────────────────────────────
if [ "${SKIP_SWEBENCH:-0}" = "1" ]; then
  echo "[run-all] SKIP swebench-lite"
else
  echo "[run-all] running SWE-bench Lite..."
  bash "$REPO_ROOT/scripts/bench/run-swebench-lite.sh" "$REPORT_DIR" \
    2>&1 | tee "$REPORT_DIR/swebench-lite.log" || \
    echo "[run-all] swebench-lite had failures — continuing"
fi

# ── 6. Phase 50 — Terminal-Bench Pro ─────────────────────────────────
if [ "${SKIP_TBENCH_PRO:-0}" = "1" ]; then
  echo "[run-all] SKIP tbench-pro"
else
  echo "[run-all] running Terminal-Bench Pro..."
  bash "$REPO_ROOT/scripts/bench/run-tbench-pro.sh" "$REPORT_DIR" \
    2>&1 | tee "$REPORT_DIR/tbench-pro.log" || \
    echo "[run-all] tbench-pro had failures — continuing"
fi

# ── 7. Aggregate ─────────────────────────────────────────────────────
echo "[run-all] aggregating reports..."
python3 "$BENCH_DIR/analysis/aggregate.py" \
  --report-dir "$REPORT_DIR" \
  --manifest "$REPORT_DIR/manifest.json" \
  --output "$REPORT_DIR/comparison.md"

echo ""
echo "[run-all] DONE. See:"
echo "  $REPORT_DIR/comparison.md"
echo "  Jaeger UI: http://$(hostname -I | awk '{print $1}'):16686"
