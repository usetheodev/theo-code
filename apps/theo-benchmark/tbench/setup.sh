#!/usr/bin/env bash
# Setup script for Terminal-Bench.
# Installs the Theo Code binary inside a Debian-based container.
#
# IMPORTANT: tb sources this script (`source install-agent.sh`), so we
# MUST NOT call `exit` (it kills the parent tmux shell). All exits are
# `return` from the wrapper function or fall-through to end of file.

# Wrap everything in a function so we can use `return` instead of `exit`.
__theo_setup() {
    set +e  # don't abort on individual errors — script must complete cleanly

    THEO_VERSION="${THEO_VERSION:-latest}"
    # Default: HTTP server on the Docker host bridge (172.17.0.1:8080)
    # where the bench runner exposes /opt/theo-bin/.
    THEO_BIN_URL="${THEO_BIN_URL:-http://172.17.0.1:8080/theo}"

    echo "[theo-setup] Installing Theo Code agent..."

    # Ensure curl exists (some minimal images skip it).
    if ! command -v curl >/dev/null 2>&1; then
        echo "[theo-setup] curl missing; installing"
        apt-get update -qq >/dev/null 2>&1
        apt-get install -y -qq curl ca-certificates >/dev/null 2>&1
    fi

    # Setup auth.json — download from the bench HTTP server (fastest path
    # for OAuth Codex). THEO_AUTH_URL defaults to the same host as the binary.
    mkdir -p /root/.config/theo
    THEO_AUTH_URL="${THEO_AUTH_URL:-http://172.17.0.1:8080/auth.json}"
    if [ -n "$THEO_AUTH_URL" ]; then
        if curl -fsSL --max-time 10 "$THEO_AUTH_URL" \
               -o /root/.config/theo/auth.json 2>/dev/null; then
            chmod 600 /root/.config/theo/auth.json
            echo "[theo-setup] auth.json installed from $THEO_AUTH_URL"
        fi
    fi

    # Method 1: pre-built binary from URL (fastest, default)
    if [ -n "$THEO_BIN_URL" ]; then
        echo "[theo-setup] Downloading from $THEO_BIN_URL"
        if curl -fsSL --max-time 60 "$THEO_BIN_URL" \
               -o /usr/local/bin/theo 2>/dev/null; then
            chmod +x /usr/local/bin/theo
            # Verify it runs (catches glibc mismatch)
            if /usr/local/bin/theo --version >/dev/null 2>&1; then
                echo "[theo-setup] Installed from URL: $(/usr/local/bin/theo --version)"
                return 0
            else
                echo "[theo-setup] Binary downloaded but FAILED to run — glibc mismatch"
                ldd /usr/local/bin/theo 2>&1 | head -5
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
        return 0
    fi

    echo "[theo-setup] ERROR: no installation method succeeded"
    echo "[theo-setup] Set THEO_BIN_URL or mount binary at /mnt/theo-bin/theo"
    return 1
}

# Invoke. Use ||true so the wrapper's non-zero return doesn't propagate
# `set -e` to the caller's shell (tmux will hang on tmux wait -S done
# if the shell exits prematurely).
__theo_setup || true
unset -f __theo_setup
