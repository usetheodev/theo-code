#!/usr/bin/env bash
# check-unsafe.sh
#
# T2.9 — `unsafe` block SAFETY-comment gate.
#
# Every `unsafe { … }` / `unsafe fn …` / `unsafe impl …` in production code
# MUST be immediately preceded by a line that starts with `// SAFETY:`.
# This is the minimum hygiene bar per `.claude/rules/rust-conventions.md`:
# the invariant that makes the unsafe block sound must be documented at
# the call site.
#
# Allowlist at `.claude/rules/unsafe-allowlist.txt` — same file[:line]
# format as the other audit gates. Kept empty by default; the expectation
# is to write a SAFETY comment rather than allowlist.
#
# Usage:
#   scripts/check-unsafe.sh            # strict, exits 1 on violation
#   scripts/check-unsafe.sh --report   # report-only, exit 0
#   scripts/check-unsafe.sh --json
#
# Exit:
#   0  every unsafe has a SAFETY comment OR entry is allowlisted
#   1  unsafe without SAFETY comment encountered
#   2  invocation error

set -euo pipefail

MODE="strict"
for arg in "$@"; do
    case "$arg" in
        --report) MODE="report" ;;
        --json)   MODE="json" ;;
        --help|-h) sed -n '2,24p' "$0"; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

ALLOWLIST_FILE=".claude/rules/unsafe-allowlist.txt"
TODAY="$(date -u +%Y-%m-%d)"

declare -A ALLOW
declare -A ALLOW_SUNSET
declare -A ALLOW_REASON
ALLOW_PATTERN_GLOBS=()
ALLOW_PATTERN_REGEXES=()
ALLOW_PATTERN_SUNSETS=()
ALLOW_PATTERN_REASONS=()

if [[ -f "$ALLOWLIST_FILE" ]]; then
    while IFS='|' read -r loc sunset reason; do
        [[ -z "${loc// }" || "${loc:0:1}" == "#" ]] && continue
        ALLOW["$loc"]=1
        ALLOW_SUNSET["$loc"]="$sunset"
        ALLOW_REASON["$loc"]="$reason"
    done < "$ALLOWLIST_FILE"
fi

# ── ADR-021 recognized patterns (T2.2 of code-hygiene-5x5-plan) ────────
PATTERN_LOADER="${REPO_ROOT}/scripts/check-recognized-patterns.sh"
if [[ -f "$PATTERN_LOADER" ]]; then
    # shellcheck source=check-recognized-patterns.sh
    REPO_ROOT="$REPO_ROOT" source "$PATTERN_LOADER"
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        part1="${line%%@@*}"
        rest1="${line#*@@}"
        part2="${rest1%%@@*}"
        rest2="${rest1#*@@}"
        part3="${rest2%%@@*}"
        part4="${rest2#*@@}"
        ALLOW_PATTERN_GLOBS+=("$part1")
        ALLOW_PATTERN_REGEXES+=("$part2")
        ALLOW_PATTERN_SUNSETS+=("$part3")
        ALLOW_PATTERN_REASONS+=("$part4")
    done < <(emit_recognized_patterns unsafe)
fi

# Locate every `unsafe` site in production code.
collect() {
    if command -v rg >/dev/null 2>&1; then
        rg -n --no-heading \
            --glob 'crates/**/src/**/*.rs' \
            --glob 'apps/**/src/**/*.rs' \
            --glob '!**/tests/**' \
            --glob '!**/benches/**' \
            --glob '!**/target/**' \
            -e 'unsafe[[:space:]]*\{' \
            -e 'unsafe[[:space:]]+fn' \
            -e 'unsafe[[:space:]]+impl' 2>/dev/null || true
    else
        grep -rn -E --include='*.rs' --exclude-dir=tests --exclude-dir=target --exclude-dir=benches \
            'unsafe[[:space:]]*\{|unsafe[[:space:]]+fn|unsafe[[:space:]]+impl' crates/ apps/ 2>/dev/null || true
    fi
}

violations=()
expired=()
allow_hits=0
total=0

# For every hit, check whether the preceding 5 lines contain `SAFETY:`.
while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    path="${line%%:*}"
    rest="${line#*:}"
    line_no="${rest%%:*}"
    total=$((total + 1))

    key="$path:$line_no"

    # Allowlist check
    local_match=""
    if [[ -n "${ALLOW[$key]-}" ]]; then
        local_match="$key"
    elif [[ -n "${ALLOW[$path]-}" ]]; then
        local_match="$path"
    fi
    if [[ -n "$local_match" ]]; then
        allow_hits=$((allow_hits + 1))
        sunset="${ALLOW_SUNSET[$local_match]}"
        if [[ -n "$sunset" && "$TODAY" > "$sunset" ]]; then
            expired+=("$local_match — sunset $sunset (${ALLOW_REASON[$local_match]})")
        fi
        continue
    fi

    # ADR-021 recognized-pattern check: if this unsafe line matches one
    # of the codified `unsafe_pattern` regexes for this file's path, it is
    # accepted without a SAFETY comment (the pattern + ADR-021 entry IS
    # the documented invariant).
    line_content="$(sed -n "${line_no}p" "$path" 2>/dev/null)"
    matched_pattern=0
    p_idx=0
    while (( p_idx < ${#ALLOW_PATTERN_GLOBS[@]} )); do
        glob="${ALLOW_PATTERN_GLOBS[$p_idx]}"
        regex="${ALLOW_PATTERN_REGEXES[$p_idx]}"
        p_idx=$((p_idx + 1))
        case "$path" in
            $glob)
                if [[ "$line_content" =~ $regex ]]; then
                    matched_pattern=1
                    allow_hits=$((allow_hits + 1))
                    break
                fi
                ;;
        esac
    done
    (( matched_pattern )) && continue

    # Look back up to 8 lines for a SAFETY comment.
    start=$((line_no > 8 ? line_no - 8 : 1))
    block="$(sed -n "${start},$((line_no - 1))p" "$path" 2>/dev/null)"
    if grep -qE '^[[:space:]]*//[[:space:]]*SAFETY:' <<< "$block"; then
        continue
    fi

    violations+=("$path:$line_no (no SAFETY: comment within 8 lines above)")
done < <(collect)

if [[ "$MODE" == "json" ]]; then
    printf '{\n  "unsafe_sites": %d,\n  "violations": %d,\n  "allowlisted": %d,\n  "expired": %d,\n  "items": [' \
        "$total" "${#violations[@]}" "$allow_hits" "${#expired[@]}"
    first=1
    for v in "${violations[@]}"; do
        (( first )) || printf ','; first=0
        printf '\n    "%s"' "${v//\"/\\\"}"
    done
    printf '\n  ]\n}\n'
else
    printf 'unsafe-SAFETY gate\n'
    printf '  unsafe sites scanned:  %d\n' "$total"
    printf '  violations (no SAFETY): %d\n' "${#violations[@]}"
    printf '  allowlisted:            %d\n' "$allow_hits"
    printf '  expired:                %d\n\n' "${#expired[@]}"
    if (( ${#violations[@]} > 0 )); then
        printf 'Unsafe blocks without a `// SAFETY:` comment:\n'
        for v in "${violations[@]:0:50}"; do printf '  - %s\n' "$v"; done
        printf '\n  -> Add a `// SAFETY: <invariant>` line immediately before the unsafe block.\n'
    fi
    if (( ${#violations[@]} == 0 && ${#expired[@]} == 0 )); then
        printf 'OK — every production `unsafe` has a SAFETY comment.\n'
    fi
fi

if [[ "$MODE" == "strict" ]] && (( ${#violations[@]} > 0 || ${#expired[@]} > 0 )); then
    exit 1
fi
exit 0
