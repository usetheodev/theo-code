#!/usr/bin/env bash
# Setup script for Terminal-Bench / Harbor.
# Installs the Theo Code binary inside a Debian-based container.
#
# This script is copied into the task container and executed once before
# the agent is invoked. It must leave a working `theo` binary in PATH.

set -euo pipefail

THEO_VERSION="${THEO_VERSION:-latest}"
THEO_BIN_URL="${THEO_BIN_URL:-}"

echo "[theo-setup] Installing Theo Code agent..."

# Method 1: pre-built binary from URL (fastest)
if [ -n "$THEO_BIN_URL" ]; then
    echo "[theo-setup] Downloading from $THEO_BIN_URL"
    curl -fsSL "$THEO_BIN_URL" -o /usr/local/bin/theo
    chmod +x /usr/local/bin/theo
    echo "[theo-setup] Installed from URL"
    theo --version || true
    exit 0
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
