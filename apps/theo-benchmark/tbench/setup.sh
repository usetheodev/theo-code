#!/usr/bin/env bash
# Setup script for Terminal-Bench / Harbor.
# Installs the Theo Code binary inside a Debian-based container.
#
# This script is copied into the task container and executed once before
# the agent is invoked. It must leave a working `theo` binary in PATH.

set -euo pipefail

THEO_VERSION="${THEO_VERSION:-latest}"
# Default: HTTP server on the Docker host bridge (172.17.0.1:8080) where
# scripts/bench/run-all.sh starts `python3 -m http.server` against the
# /opt/theo-target/release/ directory. Override with THEO_BIN_URL env.
THEO_BIN_URL="${THEO_BIN_URL:-http://172.17.0.1:8080/theo}"

echo "[theo-setup] Installing Theo Code agent..."

# Ensure curl exists (some minimal images skip it).
if ! command -v curl >/dev/null 2>&1; then
    echo "[theo-setup] curl missing; installing"
    apt-get update -qq && apt-get install -y -qq curl ca-certificates >/dev/null
fi

# Setup auth.json — download from the bench HTTP server (fastest path
# for OAuth Codex). THEO_AUTH_URL defaults to the same host as the binary.
mkdir -p /root/.config/theo
THEO_AUTH_URL="${THEO_AUTH_URL:-http://172.17.0.1:8080/auth.json}"
if [ -n "$THEO_AUTH_URL" ]; then
    if curl -fsSL --max-time 10 "$THEO_AUTH_URL" -o /root/.config/theo/auth.json 2>/dev/null; then
        chmod 600 /root/.config/theo/auth.json
        echo "[theo-setup] auth.json installed from $THEO_AUTH_URL"
    fi
fi

# Method 1: pre-built binary from URL (fastest, default)
if [ -n "$THEO_BIN_URL" ]; then
    echo "[theo-setup] Downloading from $THEO_BIN_URL"
    if curl -fsSL --max-time 30 "$THEO_BIN_URL" -o /usr/local/bin/theo; then
        chmod +x /usr/local/bin/theo
        # Verify it runs (catches glibc/musl mismatch)
        if /usr/local/bin/theo --version >/dev/null 2>&1; then
            echo "[theo-setup] Installed from URL: $(theo --version)"
            exit 0
        else
            echo "[theo-setup] Binary downloaded but FAILED to run — likely glibc mismatch"
            ldd /usr/local/bin/theo 2>&1 | head -5 || true
            rm -f /usr/local/bin/theo
        fi
    else
        echo "[theo-setup] HTTP download failed from $THEO_BIN_URL"
    fi
fi

# Method 2: copy from mounted volume (for local dev)
if [ -f /mnt/theo-bin/theo ]; then
    cp /mnt/theo-bin/theo /usr/local/bin/theo
    chmod +x /usr/local/bin/theo
    echo "[theo-setup] Installed from /mnt/theo-bin"
    theo --version || true
    exit 0
fi

# Method 3: build from source (slowest, last resort)
if command -v cargo &>/dev/null; then
    echo "[theo-setup] Building from source..."
    if [ -d /mnt/theo-src ]; then
        cd /mnt/theo-src
    elif [ -d /opt/theo-code ]; then
        cd /opt/theo-code
    else
        echo "[theo-setup] ERROR: no source directory found"
        exit 1
    fi
    cargo build -p theo --release
    cp target/release/theo /usr/local/bin/theo
    echo "[theo-setup] Built and installed"
    theo --version || true
    exit 0
fi

echo "[theo-setup] ERROR: no installation method available"
echo "[theo-setup] Set THEO_BIN_URL or mount binary at /mnt/theo-bin/theo"
exit 1
