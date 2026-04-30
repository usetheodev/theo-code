#!/usr/bin/env bash
# check-inline-io-tests.sh
#
# T5.2 — Detect `#[test]` / `#[tokio::test]` blocks inside `crates/*/src/`
# that touch real filesystem, network, or sqlx I/O. These should live in
# `crates/<crate>/tests/` so the default `cargo test` run separates fast
# deterministic unit tests from integration-style I/O tests.
#
# The heuristic: a file is flagged when BOTH conditions hold —
#   1. It lives under `crates/*/src/` and contains `#[test]` or
#      `#[tokio::test]` inside a `#[cfg(test)]` block.
#   2. The same file references any I/O marker from the curated list below.
#
# False positives are tolerated; the next step is human triage into the
# allowlist or a real migration. Each line of output identifies the file
# and the I/O marker that fired, so triage can be fast.
#
# Usage:
#   scripts/check-inline-io-tests.sh            # strict, exits 1 on any violation
#   scripts/check-inline-io-tests.sh --report   # no exit code, print summary
#   scripts/check-inline-io-tests.sh --json
#
# Allowlist: .claude/rules/io-test-allowlist.txt — one path per line (sunset-less).
#
# Exit codes:
#   0  clean OR allowlisted
#   1  violations detected
#   2  invocation error

set -euo pipefail

MODE="strict"
for arg in "$@"; do
    case "$arg" in
        --report) MODE="report" ;;
        --json)   MODE="json" ;;
        --help|-h) sed -n '2,30p' "$0"; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

ALLOWLIST_FILE=".claude/rules/io-test-allowlist.txt"
declare -A ALLOW
if [[ -f "$ALLOWLIST_FILE" ]]; then
    while IFS= read -r line; do
        [[ -z "${line// }" || "${line:0:1}" == "#" ]] && continue
        ALLOW["$line"]=1
    done < "$ALLOWLIST_FILE"
fi

# ADR-017 v2 / ADR-021 — load `[[io_test_pattern]]` scope_path entries
# from `.claude/rules/recognized-patterns.toml`. A test-bearing file
# whose path has any of these prefixes is auto-allowed (codified
# pattern, no explicit allowlist entry needed).
PATTERN_PREFIXES=()
if command -v python3 >/dev/null 2>&1 \
   && [[ -f "$REPO_ROOT/.claude/rules/recognized-patterns.toml" ]]; then
    while IFS= read -r prefix; do
        [[ -n "$prefix" ]] && PATTERN_PREFIXES+=("$prefix")
    done < <(python3 - "$REPO_ROOT/.claude/rules/recognized-patterns.toml" <<'PY'
import sys
try:
    import tomllib
except ImportError:
    import tomli as tomllib
with open(sys.argv[1], "rb") as f:
    data = tomllib.load(f)
for entry in data.get("io_test_pattern", []):
    sp = entry.get("scope_path")
    # Only emit explicit-prefix patterns (markers-based ones are
    # already hardcoded in IO_MARKERS / the tempfile auto-allow).
    if sp:
        print(sp)
PY
)
fi

# I/O markers: Rust patterns that generally imply real syscalls.
IO_MARKERS=(
    'std::fs::'
    'tokio::fs::'
    'sqlx::'
    'reqwest::(?!Error)'
    'TcpStream'
    'TcpListener'
    'UnixStream'
    'UnixListener'
    'std::process::Command'
    'tokio::process::'
    'tokio::net::'
    'std::os::unix::net::'
)

# Step 1: collect files under crates/*/src/ and apps/*/src/ that declare
# #[test] or #[tokio::test] inside a #[cfg(test)] block.
if command -v rg >/dev/null 2>&1; then
    test_bearing_files="$(
        rg -l --glob 'crates/**/src/**/*.rs' --glob 'apps/**/src/**/*.rs' \
            -e '#\[test\]' -e '#\[tokio::test\]' 2>/dev/null \
            | sort -u
    )"
else
    test_bearing_files="$(
        grep -rl -E '#\[(tokio::)?test\]' --include='*.rs' \
            crates/*/src/ apps/*/src/ 2>/dev/null \
            | sort -u
    )"
fi

violations=()
allow_pattern_hits=0
for f in $test_bearing_files; do
    # Skip files that obviously are tests only (path contains /tests/).
    case "$f" in
        */tests/*) continue ;;
    esac

    # Only keep files that ALSO have `#[cfg(test)]`.
    grep -q '#\[cfg(test)\]' "$f" 2>/dev/null || continue

    # ADR-017 v2 — inline_io_test pattern: a file is auto-allowed if it
    # uses tempfile::TempDir / tempdir / NamedTempFile / Builder, or an
    # in-project TestDir wrapper. These markers prove the test isolates
    # I/O via a per-test scratch dir (RAII cleanup, no shared state).
    tempfile_pattern='tempfile::TempDir|tempfile::tempdir|tempfile::NamedTempFile|tempfile::Builder|use tempfile::|TestDir::'
    if command -v rg >/dev/null 2>&1; then
        tempfile_match=$(rg -q -e 'tempfile::TempDir' \
                  -e 'tempfile::tempdir' \
                  -e 'tempfile::NamedTempFile' \
                  -e 'tempfile::Builder' \
                  -e 'use tempfile::' \
                  -e 'TestDir::' \
                  "$f" 2>/dev/null && echo 1 || echo 0)
    else
        tempfile_match=$(grep -qE "$tempfile_pattern" "$f" 2>/dev/null && echo 1 || echo 0)
    fi
    if (( tempfile_match )); then
        allow_pattern_hits=$((allow_pattern_hits + 1))
        continue
    fi

    # ADR-017 v2 / ADR-021 — scope_path codified patterns from
    # recognized-patterns.toml. These cover legitimate I/O categories
    # that don't fit `tempfile_isolated_fs` (subprocess JSON-RPC,
    # sandbox executors, OAuth callback servers, read-only fs probes,
    # etc.). One pattern per category, sourced from the TOML so the
    # ADR remains the source of truth.
    matched_prefix=0
    for prefix in "${PATTERN_PREFIXES[@]}"; do
        if [[ "$f" == "$prefix"* ]]; then
            matched_prefix=1
            break
        fi
    done
    if (( matched_prefix )); then
        allow_pattern_hits=$((allow_pattern_hits + 1))
        continue
    fi

    # For each I/O marker, check if the file references it.
    for marker in "${IO_MARKERS[@]}"; do
        # Use rg so the PCRE marker patterns work. Falls back to grep.
        marker_found=0
        if command -v rg >/dev/null 2>&1; then
            rg -q -- "$marker" "$f" 2>/dev/null && marker_found=1
        else
            grep -qE -- "$marker" "$f" 2>/dev/null && marker_found=1
        fi
        if (( marker_found )); then
            key="$f::$marker"
            if [[ -n "${ALLOW[$f]-}${ALLOW[$key]-}" ]]; then
                continue
            fi
            violations+=("$f  →  $marker")
            break   # one marker per file is enough
        fi
    done
done

total="${#violations[@]}"

if [[ "$MODE" == "json" ]]; then
    printf '{\n  "violations": %d,\n  "items": [' "$total"
    first=1
    for v in "${violations[@]}"; do
        (( first )) || printf ','; first=0
        esc="${v//\"/\\\"}"
        printf '\n    "%s"' "$esc"
    done
    printf '\n  ]\n}\n'
else
    printf 'inline I/O test gate\n'
    printf '  files flagged: %d\n' "$total"
    printf '  pattern-allowed (ADR-017 v2): %d\n' "$allow_pattern_hits"
    allow_count=0
    # Workaround for `set -u` on empty associative arrays.
    [[ -v ALLOW ]] && allow_count="${#ALLOW[@]}"
    printf '  allowlist:     %d entries\n\n' "$allow_count"
    if (( total > 0 )); then
        printf 'Candidate misclassified I/O tests (first 50):\n'
        for v in "${violations[@]:0:50}"; do printf '  - %s\n' "$v"; done
        printf "\n  -> Move these #[test] blocks into the crate's tests/ directory,\n"
        printf "     or add the file path to %s\n     if the test really must stay inline.\n" "$ALLOWLIST_FILE"
    else
        printf 'OK — no unlisted inline I/O tests detected.\n'
    fi
fi

if [[ "$MODE" == "strict" ]] && (( total > 0 )); then
    exit 1
fi
exit 0
