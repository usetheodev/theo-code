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

    # Phase 54 escape hatch: tests source setup.sh to exercise the prompt
    # variant block without paying the 60s+ binary install retry cost.
    if [ "${THEO_SKIP_BIN_INSTALL:-}" = "1" ]; then
        echo "[theo-setup] THEO_SKIP_BIN_INSTALL=1 — skipping binary install"
        return 0
    fi

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
    # Bug #7 fix: retry with exponential backoff. 3 trials (qemu-alpine-ssh,
    # qemu-startup, cron-broken-network) failed because the container had
    # transient network setup delay and curl 1-shot timed out. Retry covers it.
    if [ -n "$THEO_BIN_URL" ]; then
        echo "[theo-setup] Downloading from $THEO_BIN_URL (with retries)"
        local attempt=0
        local backoff=2
        while [ $attempt -lt 5 ]; do
            attempt=$((attempt + 1))
            if curl -fsSL --max-time 60 --connect-timeout 10 \
                   "$THEO_BIN_URL" -o /usr/local/bin/theo 2>/tmp/theo-curl-err.log; then
                chmod +x /usr/local/bin/theo
                if /usr/local/bin/theo --version >/dev/null 2>&1; then
                    echo "[theo-setup] Installed from URL on attempt $attempt: $(/usr/local/bin/theo --version)"
                    return 0
                else
                    echo "[theo-setup] Binary downloaded (attempt $attempt) but FAILED to run — glibc mismatch:"
                    ldd /usr/local/bin/theo 2>&1 | head -5
                    rm -f /usr/local/bin/theo
                    return 1   # glibc mismatch is permanent — don't retry
                fi
            else
                echo "[theo-setup] curl attempt $attempt/5 failed:"
                cat /tmp/theo-curl-err.log 2>/dev/null | head -3
                sleep "$backoff"
                backoff=$((backoff * 2))
            fi
        done
        echo "[theo-setup] all 5 HTTP attempts failed — falling through"
    fi

    # Method 2: copy from mounted volume (for local dev)
    if [ -f /mnt/theo-bin/theo ]; then
        cp /mnt/theo-bin/theo /usr/local/bin/theo
        chmod +x /usr/local/bin/theo
        echo "[theo-setup] Installed from /mnt/theo-bin"
        # fall through to Phase 54 prompt download
    else
        echo "[theo-setup] ERROR: no installation method succeeded"
        echo "[theo-setup] Set THEO_BIN_URL or mount binary at /mnt/theo-bin/theo"
        return 1
    fi
}

# Phase 54 (prompt-ab-testing-plan): download A/B prompt variant if requested
# and export THEO_SYSTEM_PROMPT_FILE so theo --headless picks it up. Variant
# names map to URLs on the same HTTP server that hosts the binary + auth.json.
__theo_prompt_variant_setup() {
    if [ -z "${THEO_PROMPT_VARIANT:-}" ]; then
        return 0
    fi
    local prompt_host="${THEO_PROMPT_HOST:-http://172.17.0.1:8080}"
    local variant_url="${prompt_host}/prompts/${THEO_PROMPT_VARIANT}.md"
    # Container path defaults to /installed-agent; tests can override.
    local prompt_path="${THEO_PROMPT_PATH:-/installed-agent/prompt.md}"
    mkdir -p "$(dirname "$prompt_path")"
    # 3 retries (network known good — binary install already completed)
    local attempt=0
    local backoff=1
    while [ $attempt -lt 3 ]; do
        attempt=$((attempt + 1))
        if curl -fsSL --max-time 5 --connect-timeout 2 \
               "$variant_url" -o "$prompt_path" 2>/tmp/theo-prompt-curl.log; then
            export THEO_SYSTEM_PROMPT_FILE="$prompt_path"
            # Persist into the agent shell environment so subsequent commands
            # (theo --headless invoked by tb) inherit it. tb's TmuxSession
            # spawns child processes from the parent shell, which already
            # `source`d this script — so `export` here suffices.
            echo "[theo-setup] prompt variant '$THEO_PROMPT_VARIANT' loaded from $variant_url"
            return 0
        else
            echo "[theo-setup] prompt fetch attempt $attempt/3 failed:"
            cat /tmp/theo-prompt-curl.log 2>/dev/null | head -3
            sleep "$backoff"
            backoff=$((backoff * 2))
        fi
    done
    echo "[theo-setup] WARN: variant '$THEO_PROMPT_VARIANT' unavailable; falling back to default prompt"
    unset THEO_SYSTEM_PROMPT_FILE
    return 0
}

# Invoke. Use ||true so the wrapper's non-zero return doesn't propagate
# `set -e` to the caller's shell (tmux will hang on tmux wait -S done
# if the shell exits prematurely).
__theo_setup || true
__theo_prompt_variant_setup || true
unset -f __theo_setup
unset -f __theo_prompt_variant_setup
