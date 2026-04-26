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
test_bearing_files="$(
    rg -l --glob 'crates/**/src/**/*.rs' --glob 'apps/**/src/**/*.rs' \
        -e '#\[test\]' -e '#\[tokio::test\]' 2>/dev/null \
        | sort -u
)"

violations=()
for f in $test_bearing_files; do
    # Skip files that obviously are tests only (path contains /tests/).
    case "$f" in
        */tests/*) continue ;;
    esac

    # Only keep files that ALSO have `#[cfg(test)]`.
    grep -q '#\[cfg(test)\]' "$f" 2>/dev/null || continue

    # For each I/O marker, check if the file references it.
    for marker in "${IO_MARKERS[@]}"; do
        # Use rg so the PCRE marker patterns work.
        if rg -q -- "$marker" "$f" 2>/dev/null; then
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
