#!/usr/bin/env bash
# check-sizes.sh
#
# T4.6 — File and function size gate.
#
# Enforces limits declared in .claude/rules/code-size.yaml:
#
#   crates  — file max 800 LOC, function max 60 LOC
#   UI TS   — file max 400 LOC, function max 60 LOC
#
# Allowlist at .claude/rules/size-allowlist.yaml — each entry carries a
# sunset date after which the gate will fail even if the file is listed.
# New violations (files not in allowlist) fail immediately.
#
# Usage:
#   scripts/check-sizes.sh               # strict, exit != 0 on violation
#   scripts/check-sizes.sh --report      # never fail, print summary
#   scripts/check-sizes.sh --json
#
# Exit codes:
#   0  clean OR all violations allowlisted with valid sunset
#   1  new violation OR expired allowlist entry

set -euo pipefail

MODE="strict"
for arg in "$@"; do
    case "$arg" in
        --report) MODE="report" ;;
        --json)   MODE="json" ;;
        --help|-h) sed -n '2,22p' "$0"; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

# Limits
CRATE_FILE_MAX=800
UI_FILE_MAX=400

# Today (YYYY-MM-DD) for sunset comparison.
TODAY="$(date -u +%Y-%m-%d)"

# Allowlist file format (simple line-delimited, no parser deps):
#   <path>|<limit>|<sunset-YYYY-MM-DD>|<reason>
ALLOWLIST_FILE=".claude/rules/size-allowlist.txt"

declare -A ALLOW_PATH_LIMIT
declare -A ALLOW_PATH_SUNSET
declare -A ALLOW_PATH_REASON
if [[ -f "$ALLOWLIST_FILE" ]]; then
    while IFS='|' read -r path limit sunset reason; do
        # skip comments and blanks
        [[ -z "${path// }" || "${path:0:1}" == "#" ]] && continue
        ALLOW_PATH_LIMIT["$path"]="$limit"
        ALLOW_PATH_SUNSET["$path"]="$sunset"
        ALLOW_PATH_REASON["$path"]="$reason"
    done < "$ALLOWLIST_FILE"
fi

violations_new=()        # files that violate AND are not allowlisted
violations_expired=()    # files that are allowlisted with a past sunset
files_over=0

check_file() {
    local f="$1" max="$2" kind="$3"
    local loc
    loc="$(wc -l < "$f")"
    if (( loc <= max )); then
        return
    fi
    files_over=$((files_over + 1))
    local allowed_limit="${ALLOW_PATH_LIMIT[$f]-}"
    if [[ -n "$allowed_limit" ]]; then
        # Validate sunset
        local sunset="${ALLOW_PATH_SUNSET[$f]}"
        if [[ "$TODAY" > "$sunset" ]]; then
            violations_expired+=("$f ($loc lines, allowlisted limit $allowed_limit, sunset $sunset expired)")
        fi
        # If within allowlist limit too — fine.
        if (( loc > allowed_limit )); then
            violations_new+=("$f ($loc lines — exceeds even allowlist limit $allowed_limit)")
        fi
        return
    fi
    violations_new+=("$f ($loc lines > $kind limit $max)")
}

# Scan Rust crates/apps (production code, excluding tests/, benches/, examples/).
while IFS= read -r -d '' rs_file; do
    case "$rs_file" in
        */tests/*|*/benches/*|*/examples/*) continue ;;
    esac
    check_file "$rs_file" "$CRATE_FILE_MAX" "Rust"
done < <(find crates apps -type d \( -name tests -o -name benches -o -name examples -o -name target -o -name node_modules \) -prune -false -o -type f -name '*.rs' -print0 2>/dev/null)

# Scan TS/TSX for the UI (apps/theo-ui/src).
if [[ -d apps/theo-ui/src ]]; then
    while IFS= read -r -d '' ts_file; do
        # Skip generated files and type-only barrel exports.
        case "$ts_file" in
            *.d.ts|*/__generated__/*) continue ;;
        esac
        check_file "$ts_file" "$UI_FILE_MAX" "TS"
    done < <(find apps/theo-ui/src -type f \( -name '*.ts' -o -name '*.tsx' \) -print0 2>/dev/null)
fi

# Report ---------------------------------------------------------------------
total_new="${#violations_new[@]}"
total_expired="${#violations_expired[@]}"

if [[ "$MODE" == "json" ]]; then
    printf '{\n  "files_over_limit": %d,\n' "$files_over"
    printf '  "new_violations": ['
    first=1
    for v in "${violations_new[@]}"; do
        (( first )) || printf ','; first=0
        printf '\n    %s' "\"${v//\"/\\\"}\""
    done
    printf '\n  ],\n  "expired_allowlist": ['
    first=1
    for v in "${violations_expired[@]}"; do
        (( first )) || printf ','; first=0
        printf '\n    %s' "\"${v//\"/\\\"}\""
    done
    printf '\n  ]\n}\n'
else
    printf 'size gate\n'
    printf '  crate file limit: %d LOC   UI file limit: %d LOC\n' "$CRATE_FILE_MAX" "$UI_FILE_MAX"
    printf '  files over limit: %d\n' "$files_over"
    printf '  NEW violations:   %d\n' "$total_new"
    printf '  EXPIRED allowed:  %d\n\n' "$total_expired"
    if (( total_new > 0 )); then
        printf 'New violations (not in allowlist):\n'
        for v in "${violations_new[@]}"; do printf '  - %s\n' "$v"; done
        printf '\n'
    fi
    if (( total_expired > 0 )); then
        printf 'Allowlisted files whose sunset has expired:\n'
        for v in "${violations_expired[@]}"; do printf '  - %s\n' "$v"; done
        printf '\n'
    fi
    if (( total_new == 0 && total_expired == 0 )); then
        printf 'OK — every oversize file is allowlisted with a future sunset.\n'
    fi
fi

if [[ "$MODE" == "strict" ]] && (( total_new > 0 || total_expired > 0 )); then
    exit 1
fi
exit 0
