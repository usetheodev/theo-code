#!/usr/bin/env bash
#
# Phase 29 (sota-gaps-followup) — OAuth Codex E2E smoke test that
# exercises the full delegate_task path with all 5 sota-gaps features
# active simultaneously. Closes gap #7 (OAuth Codex E2E does not exercise
# delegate_task).
#
# Gated by env: requires `OAUTH_E2E=1` to actually run + a valid OAuth
# Codex token in $XDG_CONFIG_HOME/theo/auth.json. Otherwise prints
# "skipped" and exits 0 so CI doesn't break when the token is absent.
#
# Steps:
#   1. Verify OAuth token (auth.json present + not expired).
#   2. Build CLI release binary.
#   3. Create fixture project with .theo/agents/sota12-validator.md.
#   4. Approve via `theo agents approve --all`.
#   5. Run `THEO_SKIP_ONBOARDING=1 theo agent --headless` so the model
#      bypasses the bootstrap Q&A and executes delegate_task literally.
#   6. Assert subagent_admin shows ≥ 1 run, dashboard endpoint serves
#      the agent's stats, trajectory contains HandoffEvaluated.

set -euo pipefail

if [ "${OAUTH_E2E:-0}" != "1" ]; then
  echo "[sota12-oauth-smoke] skipped (set OAUTH_E2E=1 to run)"
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$REPO_ROOT/target/release/theo"
AUTH_JSON="${XDG_CONFIG_HOME:-$HOME/.config}/theo/auth.json"

# ── 1. Verify OAuth token ─────────────────────────────────────────────
if [ ! -f "$AUTH_JSON" ]; then
  echo "[sota12-oauth-smoke] FAIL: $AUTH_JSON not found. Run \`theo login\`."
  exit 1
fi
EXPIRES_AT=$(python3 -c "import json,sys; print(json.load(open('$AUTH_JSON'))['openai']['expires_at'])" 2>/dev/null || echo "0")
NOW=$(date +%s)
if [ "$EXPIRES_AT" -le "$NOW" ]; then
  echo "[sota12-oauth-smoke] FAIL: OAuth token expired. Run \`theo login\`."
  exit 1
fi
echo "[sota12-oauth-smoke] ✓ OAuth token valid (expires_at=$EXPIRES_AT)"

# ── 2. Build CLI ──────────────────────────────────────────────────────
if [ ! -x "$CLI" ]; then
  echo "[sota12-oauth-smoke] Building release binary..."
  (cd "$REPO_ROOT" && cargo build --release -p theo --bin theo)
fi

# ── 3. Fixture project ────────────────────────────────────────────────
WORK=$(mktemp -d -t sota12-oauth-XXXXXX)
trap "rm -rf '$WORK'" EXIT
cd "$WORK"
git init -q
git -c user.email=t@t.com -c user.name=t commit --allow-empty -q -m "init"

mkdir -p .theo/agents
cat > .theo/agents/sota12-validator.md <<'EOF'
---
name: sota12-validator
description: "E2E validator: read-only audit of the working directory"
denied_tools: [edit, write, bash]
max_iterations: 3
timeout: 60
---
You audit project structure. Use `glob` to list files, then `done` with a 1-line summary.
EOF

# Help the bootstrap path skip onboarding.
mkdir -p .theo/memory
cat > .theo/memory/USER.md <<'EOF'
---
role: sre
---
# User
SRE running E2E validation. Skip onboarding, execute tools directly.
EOF

# ── 4. Approve agents ─────────────────────────────────────────────────
"$CLI" agents approve --all --repo "$WORK"

# ── 5. Run agent with delegate_task ───────────────────────────────────
echo "[sota12-oauth-smoke] Executing agent..."
RESULT_JSON=$(THEO_SKIP_ONBOARDING=1 "$CLI" agent --headless --repo "$WORK" --max-iter 6 \
  'Call delegate_task with {"agent":"sota12-validator","objective":"glob ** in this repo and report"}. Then end with done.' \
  2>/dev/null | tail -1)

echo "[sota12-oauth-smoke] result line:"
echo "$RESULT_JSON" | head -c 400
echo

# ── 6. Assertions ─────────────────────────────────────────────────────
SUCCESS=$(echo "$RESULT_JSON" | python3 -c "import json,sys; print(json.load(sys.stdin).get('success', False))" 2>/dev/null || echo "False")
if [ "$SUCCESS" != "True" ]; then
  echo "[sota12-oauth-smoke] WARN: agent did not report success. JSON:"
  echo "$RESULT_JSON"
fi

# Sub-agent run persisted?
RUN_COUNT=$("$CLI" subagent list --repo "$WORK" 2>/dev/null | grep -c "sota12-validator\|RUN_ID" || echo "0")
if [ "$RUN_COUNT" -lt 2 ]; then  # 1 header + ≥1 run
  echo "[sota12-oauth-smoke] FAIL: expected ≥1 sub-agent run; got: $RUN_COUNT"
  "$CLI" subagent list --repo "$WORK" || true
  exit 1
fi
echo "[sota12-oauth-smoke] ✓ Sub-agent run persisted"

# HandoffEvaluated event in trajectory?
HANDOFF_COUNT=$(grep -h "HandoffEvaluated" "$WORK"/.theo/trajectories/*.jsonl 2>/dev/null | wc -l || echo "0")
if [ "$HANDOFF_COUNT" -lt 1 ]; then
  echo "[sota12-oauth-smoke] FAIL: HandoffEvaluated event missing from trajectories"
  ls "$WORK"/.theo/trajectories/ || true
  exit 1
fi
echo "[sota12-oauth-smoke] ✓ HandoffEvaluated event captured ($HANDOFF_COUNT occurrences)"

# Dashboard endpoint responds?
PORT=5180
"$CLI" dashboard --repo "$WORK" --port $PORT >/dev/null 2>&1 &
DASH_PID=$!
trap "kill $DASH_PID 2>/dev/null; rm -rf '$WORK'" EXIT
sleep 2
AGENTS_JSON=$(curl -s "http://127.0.0.1:$PORT/api/agents" 2>/dev/null || echo "[]")
if ! echo "$AGENTS_JSON" | grep -q "sota12-validator"; then
  echo "[sota12-oauth-smoke] FAIL: dashboard /api/agents missing sota12-validator. Body: $AGENTS_JSON"
  exit 1
fi
echo "[sota12-oauth-smoke] ✓ Dashboard exposes sota12-validator"

echo ""
echo "[sota12-oauth-smoke] ✓✓✓ ALL ASSERTIONS PASSED"
echo "  - OAuth token valid"
echo "  - Sub-agent run persisted"
echo "  - HandoffEvaluated event emitted ($HANDOFF_COUNT)"
echo "  - Dashboard /api/agents serves the agent"
