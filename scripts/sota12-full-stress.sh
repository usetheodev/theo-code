#!/usr/bin/env bash
#
# SOTA-12 Full Stress E2E — exercita o sistema inteiro com OAuth Codex real.
#
# Cobertura por fase, validando cada gap funcional:
#   - Phase A: pre-flight (token, npx, build)
#   - Phase B: workspace setup (USER.md, agents, guardrails YAML, mcp.toml)
#   - Phase C: MCP discovery via CLI com server REAL (gaps #1, #6, #8)
#   - Phase D: agent run com OAuth → delegate_task_single → guardrail
#              (gaps #2, #5, #7)
#   - Phase E: persistência sub-agent runs (foundation)
#   - Phase F: dashboard endpoints + SSE live stream (gap #5)
#   - Phase G: theo subagent resume (rejeita terminal status)
#   - Phase H: theo mcp invalidate (gap #9)
#
# NÃO testado aqui (limitações deste script — cobertos em outras suítes):
#   - gap #3 Resume idempotency: coberto via testes Rust
#       cargo test -p theo-agent-runtime --lib subagent::resume::tests::idempotency
#       cargo test -p theo-agent-runtime --lib run_engine::tests::dispatch_replays
#       cargo test -p theo-agent-runtime --test resume_e2e
#   - gap #10 Resume worktree restore: coberto via testes Rust
#       cargo test -p theo-agent-runtime --lib subagent::tests::worktree_override
#       cargo test -p theo-agent-runtime --lib subagent::resume::tests::worktree
#       cargo test -p theo-isolation worktree_handle_existing
#       cargo test -p theo-agent-runtime --test resume_e2e
#   - gap #4 tier_chosen telemetry: AutomaticModelRouter não wired em prod
#
# Uso: OAUTH_E2E=1 bash scripts/sota12-full-stress.sh

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$REPO_ROOT/target/release/theo"
AUTH_JSON="${XDG_CONFIG_HOME:-$HOME/.config}/theo/auth.json"
WORK=$(mktemp -d -t sota12-stress-XXXXXX)
MCP_DATA="$WORK/mcp-data"
DASH_PORT=5183
DASH_PID=""

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0

cleanup() {
  [ -n "$DASH_PID" ] && kill "$DASH_PID" 2>/dev/null
  find "$WORK" -mindepth 1 -delete 2>/dev/null
  rmdir "$WORK" 2>/dev/null
}
trap cleanup EXIT

check() {
  local label="$1"
  local cond="$2"
  local detail="${3:-}"
  if [ "$cond" = "true" ]; then
    echo "  ✓ $label"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "  ✗ $label"
    [ -n "$detail" ] && echo "    detail: $detail"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

skip() {
  echo "  ⊘ $1 (skipped: $2)"
  SKIP_COUNT=$((SKIP_COUNT + 1))
}

###############################################################################
# Phase A — pre-flight
###############################################################################

echo "═══ Phase A — pre-flight ═══"

if [ "${OAUTH_E2E:-0}" != "1" ]; then
  echo "Set OAUTH_E2E=1 to run this stress test against the real OAuth Codex."
  exit 0
fi

if [ ! -f "$AUTH_JSON" ]; then
  echo "FAIL: $AUTH_JSON not found. Run \`theo login\`."
  exit 1
fi
EXPIRES_AT=$(python3 -c "import json; print(json.load(open('$AUTH_JSON'))['openai']['expires_at'])" 2>/dev/null || echo "0")
NOW=$(date +%s)
if [ "$EXPIRES_AT" -le "$NOW" ]; then
  echo "FAIL: OAuth token expired. Run \`theo login\`."
  exit 1
fi
check "OAuth Codex token valid (expires_at=$EXPIRES_AT)" true

if ! command -v npx >/dev/null 2>&1; then
  echo "WARN: npx not in PATH — Phase C MCP integration will be partial"
  HAS_NPX=false
else
  check "npx available for real MCP server" true
  HAS_NPX=true
fi

if [ ! -x "$CLI" ]; then
  echo "Building release binary..."
  (cd "$REPO_ROOT" && cargo build --release -p theo --bin theo) || exit 1
fi
check "theo CLI built" true

###############################################################################
# Phase B — workspace setup
###############################################################################

echo ""
echo "═══ Phase B — workspace setup ═══"

cd "$WORK"
git init -q
git -c user.email=t@t.com -c user.name=t commit --allow-empty -q -m "init"
mkdir -p .theo/agents .theo/memory "$MCP_DATA"

# Sample data the MCP filesystem server will expose
echo "alpha contents" > "$MCP_DATA/alpha.txt"
echo "beta contents"  > "$MCP_DATA/beta.txt"
mkdir -p "$MCP_DATA/subdir"
echo "nested" > "$MCP_DATA/subdir/gamma.txt"

# USER.md: signal to the model that onboarding is done
cat > .theo/memory/USER.md <<'EOF'
---
role: sre
---
# User
SRE running E2E validation. Skip onboarding. Tool calls only — no chitchat.
EOF

# Custom agent spec
cat > .theo/agents/audit-bot.md <<'EOF'
---
name: audit-bot
description: "E2E stress validator: read-only audit"
denied_tools: [edit, write, bash]
max_iterations: 3
timeout: 60
---
You audit the project. Use `glob` to list files, then call `done` with a summary.
EOF

# Phase 23: declarative guardrails YAML
cat > .theo/handoff_guardrails.toml <<'EOF'
# Allow audit-bot for any objective (deliberately permissive so the
# spawn proceeds during this smoke).
[[guardrail]]
id = "stress.allow-audit-bot"
matcher.target_agent = "audit-bot"
decision.kind = "warn"
decision.message = "audit-bot spawned during stress test"
EOF

# Phase 21: MCP server config
if [ "$HAS_NPX" = "true" ]; then
  cat > .theo/mcp.toml <<EOF
[[server]]
name = "fs"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "$MCP_DATA"]
EOF
  check ".theo/mcp.toml written for filesystem server" true
fi

# Phase 27 (gap #4): routing slots so AutomaticModelRouter actually fires.
# IMPORTANT: only models supported by ChatGPT-account OAuth (see probe
# in scripts/sota12-full-stress.sh comments). Verified live:
#   ✅ gpt-5.4, gpt-5.4-mini, gpt-5.3-codex, gpt-5.2
#   ❌ gpt-5.2-codex, gpt-5.1-codex-max, gpt-5.1-codex-mini (API-key only)
cat > .theo/config.toml <<'EOF'
[routing]
enabled = true
strategy = "rules"

[routing.slots.cheap]
model = "gpt-5.4-mini"
provider = "chatgpt-codex"

[routing.slots.default]
model = "gpt-5.4"
provider = "chatgpt-codex"

[routing.slots.strong]
model = "gpt-5.3-codex"
provider = "chatgpt-codex"
EOF
check ".theo/config.toml written for AutomaticModelRouter" true

check "workspace tree created at $WORK" true

# Phase 2: approve the project agent
"$CLI" agents approve --all --repo "$WORK" >/dev/null 2>&1
APPROVED=$([ -f "$WORK/.theo/.agents-approved" ] && echo true || echo false)
check "project agent approved (S3 manifest)" "$APPROVED"

###############################################################################
# Phase C — MCP discovery via CLI (gaps #1, #6, #8)
###############################################################################

echo ""
echo "═══ Phase C — MCP CLI + real server (gaps #1, #6, #8) ═══"

if [ "$HAS_NPX" = "true" ]; then
  # Pre-warm the npx cache so the discover call doesn't hit the
  # 5s per-server timeout on first download. We pipe `Y` to accept
  # any prompts and discard output.
  echo "Pre-warming npx cache (one-time, may take 30s)..."
  timeout 60 npx -y @modelcontextprotocol/server-filesystem --help \
    > /dev/null 2>&1 || true

  echo "Running \`theo mcp discover --timeout-secs 30\`..."
  # Phase 34 (mcp-http-and-discover-flake): explicit 30s timeout
  # absorbs npx cold-start latency (download + node bootstrap).
  DISCOVER_OUT=$("$CLI" mcp discover --repo "$WORK" --timeout-secs 30 2>&1)
  DISCOVERED=$(echo "$DISCOVER_OUT" | grep -c "✓ fs" 2>/dev/null)
  DISCOVERED=${DISCOVERED:-0}
  if [ "$DISCOVERED" -ge 1 ]; then
    check "theo mcp discover succeeded for fs server" true
  else
    # Soft-fail: the test still proceeds. npx flakiness is environmental.
    check "theo mcp discover succeeded for fs server" false "$DISCOVER_OUT"
  fi

  # `theo mcp list` (in a NEW process — cache is per-process, so this
  # will be empty. We assert the empty-state guidance instead).
  LIST_OUT=$("$CLI" mcp list --repo "$WORK" 2>&1)
  if echo "$LIST_OUT" | grep -qE "No MCP servers cached|SERVER"; then
    check "theo mcp list reports cache state" true
  else
    check "theo mcp list reports cache state" false "$LIST_OUT"
  fi
else
  skip "MCP discovery + list" "npx absent"
fi

###############################################################################
# Phase D — primary agent run (gaps #2, #5, #7)
###############################################################################

echo ""
echo "═══ Phase D — agent run (real OAuth → guardrail → spawn) ═══"

echo "Executing agent (THEO_FORCE_TOOL_CHOICE=function:delegate_task_single)..."
RESULT_JSON=$(THEO_SKIP_ONBOARDING=1 THEO_FORCE_TOOL_CHOICE=function:delegate_task_single \
  "$CLI" agent --headless --repo "$WORK" --max-iter 5 \
  'Use delegate_task_single with agent="audit-bot" objective="audit project files"' \
  2>/dev/null | tail -1)

if [ -z "$RESULT_JSON" ]; then
  check "agent produced JSON result" false "empty output"
else
  check "agent produced JSON result" true
fi

# Trajectory inspection: which event types were emitted?
TRAJ_FILE=$(find "$WORK/.theo/trajectories" -name "*.jsonl" 2>/dev/null | head -1)
if [ -z "$TRAJ_FILE" ]; then
  check "trajectory file created" false ""
else
  check "trajectory file created at .theo/trajectories/" true

  HANDOFF=$(grep -c "HandoffEvaluated" "$TRAJ_FILE" 2>/dev/null || echo "0")
  STARTED=$(grep -c "SubagentStarted" "$TRAJ_FILE" 2>/dev/null || echo "0")
  COMPLETED=$(grep -c "SubagentCompleted" "$TRAJ_FILE" 2>/dev/null || echo "0")

  [ "$HANDOFF" -ge 1 ] && check "HandoffEvaluated event emitted (n=$HANDOFF)" true \
    || check "HandoffEvaluated event emitted" false "expected ≥1, got $HANDOFF"
  [ "$STARTED" -ge 1 ] && check "SubagentStarted event emitted (n=$STARTED)" true \
    || check "SubagentStarted event emitted" false "expected ≥1, got $STARTED"
  [ "$COMPLETED" -ge 1 ] && check "SubagentCompleted event emitted (n=$COMPLETED)" true \
    || check "SubagentCompleted event emitted" false "expected ≥1, got $COMPLETED"

  # Phase 23 evidence: declarative guardrail id appears in
  # `guardrails_evaluated` array of HandoffEvaluated payloads.
  GUARDRAIL_HIT=$(grep "HandoffEvaluated" "$TRAJ_FILE" 2>/dev/null | grep -c "stress.allow-audit-bot" || echo "0")
  [ "$GUARDRAIL_HIT" -ge 1 ] && check "declarative TOML guardrail loaded + evaluated" true \
    || check "declarative TOML guardrail loaded + evaluated" false "stress.allow-audit-bot not in any HandoffEvaluated payload"
fi

###############################################################################
# Phase E — persistence
###############################################################################

echo ""
echo "═══ Phase E — sub-agent persistence ═══"

RUNS_DIR="$WORK/.theo/subagent/runs"
RUN_FILES=$(find "$RUNS_DIR" -name "*.json" 2>/dev/null | wc -l)
[ "$RUN_FILES" -ge 1 ] && check "sub-agent run files persisted (n=$RUN_FILES)" true \
  || check "sub-agent run files persisted" false "expected ≥1, found $RUN_FILES in $RUNS_DIR"

LIST_OUT=$("$CLI" subagent list --repo "$WORK" 2>/dev/null)
if echo "$LIST_OUT" | grep -q "audit-bot"; then
  check "theo subagent list shows audit-bot" true
else
  check "theo subagent list shows audit-bot" false "$LIST_OUT"
fi

# Pick first persisted run id
FIRST_RUN_ID=$(find "$RUNS_DIR" -name "subagent-audit-bot-*.json" 2>/dev/null | head -1 | xargs -I {} basename {} .json)
if [ -n "$FIRST_RUN_ID" ]; then
  STATUS_OUT=$("$CLI" subagent status "$FIRST_RUN_ID" --repo "$WORK" 2>/dev/null)
  if echo "$STATUS_OUT" | grep -q "Agent: audit-bot"; then
    check "theo subagent status retrieves details" true
  else
    check "theo subagent status retrieves details" false "$STATUS_OUT"
  fi
fi

###############################################################################
# Phase F — dashboard endpoints + SSE live (gap #5)
###############################################################################

echo ""
echo "═══ Phase F — dashboard + SSE live stream (gap #5) ═══"

"$CLI" dashboard --repo "$WORK" --port "$DASH_PORT" >/dev/null 2>&1 &
DASH_PID=$!
sleep 2

# /api/agents — list
AGENTS_JSON=$(curl -s "http://127.0.0.1:$DASH_PORT/api/agents" 2>/dev/null || echo "[]")
if echo "$AGENTS_JSON" | grep -q "audit-bot"; then
  check "/api/agents serves audit-bot" true
else
  check "/api/agents serves audit-bot" false "body: $AGENTS_JSON"
fi

# /api/agents/audit-bot — detail
DETAIL_JSON=$(curl -s "http://127.0.0.1:$DASH_PORT/api/agents/audit-bot" 2>/dev/null || echo "{}")
RUN_COUNT_API=$(echo "$DETAIL_JSON" | python3 -c "import json,sys; print(json.load(sys.stdin)['stats']['run_count'])" 2>/dev/null || echo "0")
[ "$RUN_COUNT_API" -ge 1 ] && check "/api/agents/audit-bot returns run_count=$RUN_COUNT_API" true \
  || check "/api/agents/audit-bot returns run_count" false "$DETAIL_JSON"

# /api/agents/audit-bot/runs — runs list
RUNS_JSON=$(curl -s "http://127.0.0.1:$DASH_PORT/api/agents/audit-bot/runs" 2>/dev/null || echo "[]")
RUNS_API=$(echo "$RUNS_JSON" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))" 2>/dev/null || echo "0")
[ "$RUNS_API" -ge 1 ] && check "/api/agents/audit-bot/runs lists $RUNS_API runs" true \
  || check "/api/agents/audit-bot/runs lists runs" false "$RUNS_JSON"

# /api/agents/events — SSE live (Phase 28 file-tail)
SSE_OUT=$(timeout 4 curl -sN "http://127.0.0.1:$DASH_PORT/api/agents/events" 2>&1)
if echo "$SSE_OUT" | grep -qE "subagent_run_added|subagent_started|subagent_completed|handoff_evaluated"; then
  check "SSE /api/agents/events emits subagent events" true
else
  check "SSE /api/agents/events emits subagent events" false "first 200 chars: ${SSE_OUT:0:200}"
fi

###############################################################################
# Phase G — resume CLI rejection (Phase 16 friendly UX)
###############################################################################

echo ""
echo "═══ Phase G — resume CLI rejection ═══"

if [ -n "$FIRST_RUN_ID" ]; then
  # The persisted run is in a terminal status (Failed because BudgetExceeded
  # at sub-agent level). Resume must print friendly guidance and exit 0.
  RESUME_OUT=$("$CLI" subagent resume "$FIRST_RUN_ID" --repo "$WORK" 2>&1)
  RESUME_EXIT=$?
  if [ $RESUME_EXIT -eq 0 ] && echo "$RESUME_OUT" | grep -qE "terminal|abandoned"; then
    check "theo subagent resume prints terminal-status guidance + Ok exit" true
  else
    check "theo subagent resume prints terminal-status guidance + Ok exit" false \
      "exit=$RESUME_EXIT out=$RESUME_OUT"
  fi
fi

###############################################################################
# Phase H — MCP CLI invalidate + clear-all (gap #9)
###############################################################################

echo ""
echo "═══ Phase H — MCP cache invalidate (gap #9) ═══"

INVALIDATE_OUT=$("$CLI" mcp invalidate fs --repo "$WORK" 2>&1)
if echo "$INVALIDATE_OUT" | grep -qE "Invalidated|not in cache"; then
  check "theo mcp invalidate runs cleanly" true
else
  check "theo mcp invalidate runs cleanly" false "$INVALIDATE_OUT"
fi

CLEAR_OUT=$("$CLI" mcp clear-all --repo "$WORK" 2>&1)
if echo "$CLEAR_OUT" | grep -q "Cleared"; then
  check "theo mcp clear-all runs cleanly" true
else
  check "theo mcp clear-all runs cleanly" false "$CLEAR_OUT"
fi

###############################################################################
# Phase I — AutomaticModelRouter wired (gap #4)
###############################################################################

echo ""
echo "═══ Phase I — routing telemetry (gap #4) ═══"

# Trajectory captures `routing_reason` per LlmCallStart event. With the
# router wired, it should NOT be "no_router" — it should be one of the
# tiered values (e.g. "rules:auto", "task_type:Analysis").
if [ -n "$TRAJ_FILE" ]; then
  NO_ROUTER_COUNT=$(grep "LlmCallStart" "$TRAJ_FILE" 2>/dev/null | grep -c '"routing_reason":"no_router"' || true)
  NO_ROUTER_COUNT=${NO_ROUTER_COUNT:-0}
  ROUTED_COUNT=$(grep "LlmCallStart" "$TRAJ_FILE" 2>/dev/null | grep -cv '"routing_reason":"no_router"' || true)
  ROUTED_COUNT=${ROUTED_COUNT:-0}
  if [ "$NO_ROUTER_COUNT" -eq 0 ] && [ "$ROUTED_COUNT" -ge 1 ]; then
    check "AutomaticModelRouter active (no 'no_router' fallbacks)" true
  else
    check "AutomaticModelRouter active (no 'no_router' fallbacks)" false \
      "no_router=$NO_ROUTER_COUNT routed=$ROUTED_COUNT"
  fi
else
  skip "routing telemetry assertion" "no trajectory file"
fi

###############################################################################
# Summary
###############################################################################

kill "$DASH_PID" 2>/dev/null
DASH_PID=""

echo ""
echo "═══ Summary ═══"
echo "  PASS:  $PASS_COUNT"
echo "  FAIL:  $FAIL_COUNT"
echo "  SKIP:  $SKIP_COUNT"
echo ""
echo "Out-of-scope here (covered by Rust test suite):"
echo "  ⊕ #3 Resume idempotency tool replay  — see cargo test resume::tests::idempotency"
echo "  ⊕ #10 Resume worktree restore        — see cargo test resume::tests::worktree + tests/resume_e2e.rs"

if [ "$FAIL_COUNT" -eq 0 ]; then
  echo ""
  echo "✓✓✓ STRESS TEST PASSED ($PASS_COUNT assertions, $SKIP_COUNT skipped)"
  exit 0
else
  echo ""
  echo "✗ STRESS TEST FAILED ($FAIL_COUNT assertions failed)"
  exit 1
fi
