#!/usr/bin/env bash
#
# Phase 35-39 (mcp-http-and-discover-flake) — HTTP/Streamable transport
# E2E smoke against a reachable HTTP MCP server. Optional, gated by
# THEO_MCP_HTTP_TEST_URL.
#
# Usage:
#   THEO_MCP_HTTP_TEST_URL=https://your-server bash scripts/mcp-http-smoke.sh
#
# When the URL env var is absent, the script prints "skipped" and exits 0
# so CI doesn't break on lack of an HTTP MCP server in the test fleet.
#
# What this script proves:
#   - parse_registry_toml accepts [[server]] with transport = "http"
#   - theo mcp discover routes the HTTP server through McpAnyClient
#   - HttpTransport sends a POST + parses application/json (or SSE)
#   - The discovered tools land in the in-process cache

set -uo pipefail

if [ -z "${THEO_MCP_HTTP_TEST_URL:-}" ]; then
  echo "[mcp-http-smoke] skipped (set THEO_MCP_HTTP_TEST_URL=https://... to run)"
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$REPO_ROOT/target/release/theo"
WORK=$(mktemp -d -t mcp-http-smoke-XXXXXX)
trap "rm -rf '$WORK'" EXIT
cd "$WORK"

if [ ! -x "$CLI" ]; then
  echo "[mcp-http-smoke] Building release binary..."
  (cd "$REPO_ROOT" && cargo build --release -p theo --bin theo)
fi

git init -q
git -c user.email=t@t.com -c user.name=t commit --allow-empty -q -m "init"

mkdir -p .theo
cat > .theo/mcp.toml <<EOF
[[server]]
transport = "http"
name = "remote"
url = "${THEO_MCP_HTTP_TEST_URL}"
timeout_ms = 10000
EOF

# Optional Authorization header for the remote server.
if [ -n "${THEO_MCP_HTTP_TEST_BEARER:-}" ]; then
  cat >> .theo/mcp.toml <<EOF
headers = { Authorization = "Bearer ${THEO_MCP_HTTP_TEST_BEARER}" }
EOF
fi

echo "[mcp-http-smoke] running theo mcp discover against ${THEO_MCP_HTTP_TEST_URL}..."
DISCOVER_OUT=$("$CLI" mcp discover --repo "$WORK" --timeout-secs 15 2>&1)
echo "$DISCOVER_OUT"

if echo "$DISCOVER_OUT" | grep -qE "^✓ remote"; then
  echo "[mcp-http-smoke] ✓ HTTP transport discovered tools from remote MCP server"
  exit 0
else
  echo "[mcp-http-smoke] ✗ HTTP discover did NOT report success for 'remote'"
  exit 1
fi
