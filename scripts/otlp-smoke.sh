#!/usr/bin/env bash
#
# Phase 45 (otlp-exporter-plan) — E2E smoke test for the OTLP exporter.
#
# Runs theo-cli with --features otel + OTLP_ENDPOINT pointing to a real
# OTel Collector container (debug exporter). Validates that span
# attributes (gen_ai.agent.name, subagent.spawn) appear in the
# collector's stdout. Determinístico, sem rede externa.
#
# Gates:
#   - Docker required. Absent → script prints "skipped" and exits 0.
#   - OAUTH_E2E=1 required. Otherwise script skips agent run.
#   - Valid OAuth token. Otherwise prints "skipped".
#
# Usage:
#   OAUTH_E2E=1 bash scripts/otlp-smoke.sh

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$REPO_ROOT/target/release/theo"
COLLECTOR_LOG=$(mktemp -t otel-collector-log-XXXXXX)
WORK=""
DOCKER_NAME="theo-otlp-smoke-$$"

cleanup() {
  if [ -n "${DOCKER_NAME:-}" ]; then
    docker logs "$DOCKER_NAME" > "$COLLECTOR_LOG" 2>&1 || true
    docker rm -f "$DOCKER_NAME" >/dev/null 2>&1 || true
  fi
  if [ -n "${WORK:-}" ] && [ -d "$WORK" ]; then
    rm -rf "$WORK"
  fi
}
trap cleanup EXIT

# 1. Docker gate
if ! command -v docker >/dev/null 2>&1; then
  echo "[otlp-smoke] skipped (docker not installed)"
  exit 0
fi

# 2. OAUTH gate
if [ "${OAUTH_E2E:-0}" != "1" ]; then
  echo "[otlp-smoke] skipped (set OAUTH_E2E=1 to run)"
  exit 0
fi

AUTH_JSON="${XDG_CONFIG_HOME:-$HOME/.config}/theo/auth.json"
if [ ! -f "$AUTH_JSON" ]; then
  echo "[otlp-smoke] skipped (no auth.json — run \`theo login\`)"
  exit 0
fi
EXPIRES_AT=$(python3 -c "import json,sys; print(json.load(open('$AUTH_JSON'))['openai']['expires_at'])" 2>/dev/null || echo "0")
NOW=$(date +%s)
if [ "$EXPIRES_AT" -le "$NOW" ]; then
  echo "[otlp-smoke] skipped (OAuth token expired — run \`theo login\`)"
  exit 0
fi
echo "[otlp-smoke] ✓ Pre-flight checks passed"

# 3. Build CLI with otel feature if needed
if [ ! -x "$CLI" ] || ! "$CLI" --version 2>/dev/null | grep -q "theo"; then
  echo "[otlp-smoke] Building release binary with --features otel..."
  (cd "$REPO_ROOT" && cargo build --release --features otel -p theo --bin theo)
fi

# 4. Start OTel Collector in Docker
echo "[otlp-smoke] Starting OTel Collector container..."
docker run --rm -d --name "$DOCKER_NAME" \
  -p 4317:4317 -p 4318:4318 \
  -v "$REPO_ROOT/scripts/otlp/collector-config.yaml:/etc/otelcol-contrib/config.yaml:ro" \
  otel/opentelemetry-collector-contrib:0.110.0 \
  --config=/etc/otelcol-contrib/config.yaml \
  >/dev/null
sleep 3

# Verify collector is accepting connections.
if ! docker ps --filter "name=$DOCKER_NAME" --format '{{.Status}}' | grep -q "Up"; then
  echo "[otlp-smoke] ✗ collector failed to start"
  docker logs "$DOCKER_NAME" 2>&1 | tail -20
  exit 1
fi
echo "[otlp-smoke] ✓ Collector running"

# 5. Setup workspace + agent fixture
WORK=$(mktemp -d -t otlp-smoke-XXXXXX)
cd "$WORK"
git init -q
git -c user.email=t@t.com -c user.name=t commit --allow-empty -q -m init
mkdir -p .theo/agents .theo/memory
cat > .theo/agents/audit-bot.md <<'EOF'
---
name: audit-bot
description: "Audit only — uses glob then done"
denied_tools: [edit, write, bash]
max_iterations: 2
timeout: 30
---
You audit. Use `glob` once then `done`.
EOF
cat > .theo/memory/USER.md <<'EOF'
---
role: sre
---
# User
SRE running OTLP smoke. Skip onboarding.
EOF
"$CLI" agents approve --all --repo "$WORK" >/dev/null

# 6. Run agent with OTLP wired up
echo "[otlp-smoke] Executing agent with OTLP_ENDPOINT=http://localhost:4317..."
OTLP_ENDPOINT=http://localhost:4317 \
OTLP_SERVICE_NAME=theo-otlp-smoke \
OTLP_PROTOCOL=grpc \
THEO_FORCE_TOOL_CHOICE=function:delegate_task_single \
THEO_SKIP_ONBOARDING=1 \
"$CLI" agent --headless --repo "$WORK" --max-iter 4 \
  'Use delegate_task_single with agent="audit-bot" objective="audit"' \
  >/dev/null 2>&1 || true

# 7. Allow exporter to flush + collector to write logs
sleep 5
docker logs "$DOCKER_NAME" > "$COLLECTOR_LOG" 2>&1

# 8. Validate spans appeared
echo ""
echo "[otlp-smoke] Validating collector received spans..."
PASS=true
for ATTR in "audit-bot" "subagent.spawn" "theo-otlp-smoke"; do
  if grep -q "$ATTR" "$COLLECTOR_LOG"; then
    echo "  ✓ collector received '$ATTR'"
  else
    echo "  ✗ collector did NOT receive '$ATTR'"
    PASS=false
  fi
done

if [ "$PASS" = "true" ]; then
  echo ""
  echo "[otlp-smoke] ✓✓✓ ALL ASSERTIONS PASSED"
  exit 0
else
  echo ""
  echo "[otlp-smoke] ✗ Some assertions failed. Collector log tail:"
  tail -50 "$COLLECTOR_LOG"
  exit 1
fi
