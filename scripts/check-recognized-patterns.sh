#!/usr/bin/env bash
# check-recognized-patterns.sh
#
# Shared loader for `.claude/rules/recognized-patterns.toml` (ADR-021).
# Emits each pattern in the legacy `regex:<glob>@@<regex>@@<sunset>@@<reason>`
# format that the existing check-{unwrap,unsafe,panic,secrets}.sh gate
# scripts already understand.
#
# Sourced by each gate script; the patterns are appended to the existing
# allowlist content-regex arrays.
#
# Usage (inside a gate script that has `ALLOW_GLOBS_CR`/`ALLOW_REGEXES_CR`
#         /`ALLOW_SUNSETS_CR`/`ALLOW_REASONS_CR` arrays):
#
#     while IFS= read -r line; do
#         part1="${line%%@@*}"
#         rest1="${line#*@@}"
#         part2="${rest1%%@@*}"
#         rest2="${rest1#*@@}"
#         part3="${rest2%%@@*}"
#         part4="${rest2#*@@}"
#         ALLOW_GLOBS_CR+=("$part1")
#         ALLOW_REGEXES_CR+=("$part2")
#         ALLOW_SUNSETS_CR+=("$part3")
#         ALLOW_REASONS_CR+=("$part4")
#     done < <(emit_recognized_patterns "<gate>")
#
# Where <gate> is one of: unwrap, unsafe, panic, secret.
#
# The TOML is parsed via Python (tomllib in 3.11+, fallback to tomli for
# older). One Python invocation per gate script; cost is negligible.

set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
PATTERNS_FILE="${REPO_ROOT}/.claude/rules/recognized-patterns.toml"

# Sunset for ADR-021 codified patterns: 2026-10-31 (consistent with the
# existing renewed allowlist sunset). This is a single canonical date for
# all codified patterns; individual deprecation is handled by removing
# the pattern from the TOML, not by per-pattern sunsets.
ADR_021_SUNSET="2026-10-31"

emit_recognized_patterns() {
    local gate="$1"
    if [[ ! -f "$PATTERNS_FILE" ]]; then
        return 0
    fi
    # Map gate-name to TOML key
    local toml_key
    case "$gate" in
        unwrap) toml_key="unwrap_pattern" ;;
        unsafe) toml_key="unsafe_pattern" ;;
        panic)  toml_key="panic_pattern"  ;;
        secret) toml_key="secret_pattern" ;;
        *) echo "unknown gate: $gate" >&2; return 1 ;;
    esac
    python3 - "$PATTERNS_FILE" "$toml_key" "$ADR_021_SUNSET" <<'PY'
import sys
import tomllib
from pathlib import Path

path, key, sunset = sys.argv[1], sys.argv[2], sys.argv[3]
try:
    with open(path, "rb") as f:
        data = tomllib.load(f)
except Exception as e:
    print(f"# recognized-patterns.toml load failed: {e}", file=sys.stderr)
    sys.exit(0)

entries = data.get(key, [])
for e in entries:
    name = e.get("name", "(unnamed)")
    regex = e.get("regex", "")
    scope = e.get("scope", e.get("scope_path", ""))
    invariant = e.get("invariant", "")
    if not scope:
        continue
    # Secret patterns may use scope_path (substring match) instead of
    # regex+glob. Emit a placeholder regex that always matches; the
    # secrets gate script will further filter by scope_path semantics.
    if not regex:
        regex = ".*"
    # Emit in `<glob>@@<regex>@@<sunset>@@<reason>` format. The receiving
    # script will parse with the same `${var%%@@*}` chain it already uses.
    print(f"{scope}@@{regex}@@{sunset}@@ADR-021#{name} — {invariant}")
PY
}

# When sourced, the function `emit_recognized_patterns` becomes available.
# When invoked directly (e.g. for debugging), echo the patterns for a
# selected gate.
if [[ "${BASH_SOURCE[0]:-}" == "${0:-}" ]]; then
    gate="${1:-unwrap}"
    emit_recognized_patterns "$gate"
fi
