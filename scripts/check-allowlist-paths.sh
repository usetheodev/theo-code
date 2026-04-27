#!/usr/bin/env bash
# check-allowlist-paths.sh
#
# Structural audit for the size + complexity allowlist files.
#
# CONTENT audit (already in place): each entry has a sunset date
#   and a non-zero ceiling (`scripts/check-sizes.sh` /
#   `scripts/check-complexity.sh`).
# STRUCTURAL audit (this script): each entry's path actually
#   resolves to a file on disk OR a crate name registered in
#   `Cargo.toml`.
#
# Stale entries silently disable the gate for that file/crate —
# I caught and fixed several of them in iter-7 (run_engine.rs →
# run_engine/mod.rs etc.) but only AFTER they had been silently
# dead for weeks. This script prevents the regression.
#
# Same lesson as the iter-25 (CLI invokability) and iter-26
# (tool input_examples vs schema) gates: a CONTENT audit
# (entry exists in file?) doesn't substitute for a STRUCTURAL
# audit (does the entry's referent actually exist?).
#
# Usage:
#   scripts/check-allowlist-paths.sh           # strict
#   scripts/check-allowlist-paths.sh --json    # CI consumption
#
# Exit codes:
#   0  every entry resolves to an existing path / crate
#   1  one or more entries are stale
#   2  invocation error

set -uo pipefail

OUTPUT="text"
for arg in "$@"; do
    case "$arg" in
        --json)    OUTPUT="json" ;;
        --help|-h) sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

declare -A CRATES
# Build a lookup of crate names from workspace Cargo.toml.
while IFS= read -r path; do
    if [[ -f "$path/Cargo.toml" ]]; then
        name="$(grep -E '^[[:space:]]*name[[:space:]]*=' "$path/Cargo.toml" | head -1 \
            | sed -E 's/[[:space:]]*name[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')"
        [[ -n "$name" ]] && CRATES[$name]="$path"
    fi
done < <(find crates apps -maxdepth 2 -name Cargo.toml -exec dirname {} \;)

declare -a STALE_PATHS=()
declare -a STALE_CRATES=()
total=0

# ---------------------------------------------------------------------------
# size-allowlist.txt — entry format: <path>|<limit>|<sunset>|<reason>
# ---------------------------------------------------------------------------

if [[ -f .claude/rules/size-allowlist.txt ]]; then
    while IFS='|' read -r path _limit _sunset _reason; do
        [[ -z "${path// }" || "${path:0:1}" == "#" ]] && continue
        total=$((total + 1))
        if [[ ! -e "$path" ]]; then
            STALE_PATHS+=( "size-allowlist|$path" )
        fi
    done < .claude/rules/size-allowlist.txt
fi

# ---------------------------------------------------------------------------
# complexity-allowlist.txt — entry format: <crate>|<count>|<reason>
# ---------------------------------------------------------------------------

if [[ -f .claude/rules/complexity-allowlist.txt ]]; then
    while IFS='|' read -r crate _count _reason; do
        [[ -z "${crate// }" || "${crate:0:1}" == "#" ]] && continue
        total=$((total + 1))
        if [[ -z "${CRATES[$crate]:-}" ]]; then
            STALE_CRATES+=( "complexity-allowlist|$crate" )
        fi
    done < .claude/rules/complexity-allowlist.txt
fi

# ---------------------------------------------------------------------------
# Render
# ---------------------------------------------------------------------------

stale_count=$(( ${#STALE_PATHS[@]} + ${#STALE_CRATES[@]} ))

if [[ "$OUTPUT" == "json" ]]; then
    printf '{\n  "total_entries": %d,\n' "$total"
    printf '  "stale": [\n'
    first=1
    for s in "${STALE_PATHS[@]}" "${STALE_CRATES[@]}"; do
        IFS='|' read -r src ref <<< "$s"
        [[ $first -eq 0 ]] && printf ',\n'
        first=0
        printf '    {"source": "%s", "ref": "%s"}' "$src" "$ref"
    done
    printf '\n  ],\n'
    printf '  "stale_count": %d\n}\n' "$stale_count"
else
    printf 'Allowlist path/crate audit (structural)\n'
    printf '%s\n' "------------------------------------------------------------"
    printf '  total entries scanned: %d\n' "$total"
    printf '  stale path entries:    %d\n' "${#STALE_PATHS[@]}"
    printf '  stale crate entries:   %d\n' "${#STALE_CRATES[@]}"
    if [[ ${#STALE_PATHS[@]} -gt 0 ]]; then
        printf '\n  Stale path entries (file no longer exists):\n'
        for s in "${STALE_PATHS[@]}"; do
            IFS='|' read -r src ref <<< "$s"
            printf '    [%s] %s\n' "$src" "$ref"
        done
    fi
    if [[ ${#STALE_CRATES[@]} -gt 0 ]]; then
        printf '\n  Stale crate entries (no crate by that name in workspace):\n'
        for s in "${STALE_CRATES[@]}"; do
            IFS='|' read -r src ref <<< "$s"
            printf '    [%s] %s\n' "$src" "$ref"
        done
    fi
    printf '%s\n' "------------------------------------------------------------"
    if [[ $stale_count -gt 0 ]]; then
        printf '✗ %d stale allowlist entr(y/ies) detected.\n' "$stale_count"
        printf '  Either restore the missing path/crate OR update the\n'
        printf '  entry in .claude/rules/<file>-allowlist.txt to its\n'
        printf '  current canonical name (the file may have been\n'
        printf '  renamed via module-dir refactor: foo.rs → foo/mod.rs).\n'
    else
        printf '✓ Every allowlist entry resolves to an existing\n'
        printf '  path/crate. Allowlists are not silently disabled.\n'
    fi
fi

exit $(( stale_count > 0 ? 1 : 0 ))
