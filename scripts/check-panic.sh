#!/usr/bin/env bash
# check-panic.sh
#
# T2.6 — Production `panic!`, `todo!`, `unimplemented!` gate.
#
# Scans `crates/*/src/` and `apps/*/src/` for the three macros outside
# `#[cfg(test)]` modules and `tests/` directories. Per
# `.claude/rules/rust-conventions.md`:
#
#   > Errors must carry context: what happened, which entity, what was expected.
#
# `panic!` and `unimplemented!` undermine that rule; `todo!` by definition
# ships as unfinished work.
#
# Allowlist at `.claude/rules/panic-allowlist.txt` uses the same
# format as the unwrap allowlist (file or file:line entries).
#
# Usage:
#   scripts/check-panic.sh              # strict
#   scripts/check-panic.sh --report     # report, exit 0
#   scripts/check-panic.sh --json
#   scripts/check-panic.sh --only=panic       # skip todo/unimplemented
#   scripts/check-panic.sh --only=todo
#
# Exit:
#   0  clean OR all allowlisted
#   1  unlisted occurrences OR expired allowlist entry
#   2  invocation error

set -euo pipefail

MODE="strict"
SCOPE="all"
for arg in "$@"; do
    case "$arg" in
        --report) MODE="report" ;;
        --json)   MODE="json" ;;
        --only=panic)          SCOPE="panic" ;;
        --only=todo)           SCOPE="todo" ;;
        --only=unimplemented)  SCOPE="unimplemented" ;;
        --help|-h) sed -n '2,30p' "$0"; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

ALLOWLIST_FILE=".claude/rules/panic-allowlist.txt"
TODAY="$(date -u +%Y-%m-%d)"

declare -A ALLOW
declare -A ALLOW_SUNSET
declare -A ALLOW_REASON
ALLOW_PATTERN_GLOBS=()
ALLOW_PATTERN_REGEXES=()

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
        ALLOW_PATTERN_GLOBS+=("$part1")
        ALLOW_PATTERN_REGEXES+=("$part2")
    done < <(emit_recognized_patterns panic)
fi

collect() {
    local pattern="$1"
    if command -v rg >/dev/null 2>&1; then
        rg -n --no-heading \
            --glob 'crates/**/src/**/*.rs' \
            --glob 'apps/**/src/**/*.rs' \
            --glob '!**/tests/**' \
            --glob '!**/benches/**' \
            --glob '!**/target/**' \
            -- "$pattern" 2>/dev/null || true
    else
        grep -rn -E --include='*.rs' --exclude-dir=tests --exclude-dir=target --exclude-dir=benches \
            "$pattern" crates/ apps/ 2>/dev/null || true
    fi
}

# Same test-block filter used by check-unwrap.sh.
# Kept in sync: compound #[cfg(all(test, ...))] + comment-line filtering.
filter_production() {
    awk -F: 'BEGIN{OFS=":"}
        {
          path=$1
          if (!(path in cfg_line)) {
            # Match outer (`#[cfg(test)]`) and inner (`#![cfg(test)]`)
            # forms PLUS the compound form `#[cfg(all(test, feature = "..."))]`
            # used by feature-gated test modules (e.g. tantivy-backend).
            cmd = "grep -nE '\''^[[:space:]]*#!?\\[cfg\\((all\\([^)]*\\b)?test\\b'\'' " path " | head -1"
            got = (cmd | getline first) ; close(cmd)
            if (got > 0) {
              split(first, parts, ":"); cfg_line[path] = parts[1] + 0
            } else {
              cfg_line[path] = 0
            }
          }
          if (cfg_line[path] == 0 || ($2 + 0) < cfg_line[path]) {
            # Drop comment-only lines: rustdoc (`///`, `//!`) AND plain
            # line comments (`//`). A macro like `panic!` in a comment is
            # documentation, not executed code.
            content=$0
            sub("^[^:]+:[0-9]+:", "", content)
            if (content !~ /^[[:space:]]*\/\//) {
              print
            }
          }
        }'
}

run() {
    local kind="$1" pattern="$2"
    [[ "$SCOPE" != "all" && "$SCOPE" != "$kind" ]] && return
    local raw filtered
    raw="$(collect "$pattern")"
    [[ -z "$raw" ]] && return
    filtered="$(printf '%s\n' "$raw" | filter_production)"
    [[ -z "$filtered" ]] && return

    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        path_line="${line%%:*}"
        rest="${line#*:}"
        line_no="${rest%%:*}"
        key="$path_line:$line_no"
        local matched_key=""
        if [[ -n "${ALLOW[$key]-}" ]]; then
            matched_key="$key"
        elif [[ -n "${ALLOW[$path_line]-}" ]]; then
            matched_key="$path_line"
        fi
        if [[ -n "$matched_key" ]]; then
            allow_hits=$((allow_hits + 1))
            local sunset="${ALLOW_SUNSET[$matched_key]}"
            if [[ -n "$sunset" && "$TODAY" > "$sunset" ]]; then
                expired+=("$matched_key — sunset $sunset (${ALLOW_REASON[$matched_key]})")
            fi
            continue
        fi

        # ADR-021 recognized-pattern check: line content match.
        local content="${line#*:}"
        content="${content#*:}"
        local p_idx=0
        local pattern_match=0
        while (( p_idx < ${#ALLOW_PATTERN_GLOBS[@]} )); do
            local glob="${ALLOW_PATTERN_GLOBS[$p_idx]}"
            local regex="${ALLOW_PATTERN_REGEXES[$p_idx]}"
            p_idx=$((p_idx + 1))
            case "$path_line" in
                $glob)
                    if [[ "$content" =~ $regex ]]; then
                        pattern_match=1
                        allow_hits=$((allow_hits + 1))
                        break
                    fi
                    ;;
            esac
        done
        (( pattern_match )) && continue

        violations+=("$kind $line")
    done <<< "$filtered"
}

violations=()
expired=()
allow_hits=0

run panic         'panic!\('
run todo          'todo!\('
run unimplemented 'unimplemented!\('

total="${#violations[@]}"

if [[ "$MODE" == "json" ]]; then
    printf '{\n  "violations": %d,\n  "allowlisted": %d,\n  "expired": %d,\n  "items": [' \
        "$total" "$allow_hits" "${#expired[@]}"
    first=1
    for v in "${violations[@]}"; do
        (( first )) || printf ','; first=0
        esc="${v//\"/\\\"}"
        printf '\n    "%s"' "$esc"
    done
    printf '\n  ]\n}\n'
else
    printf 'panic/todo/unimplemented gate\n'
    printf '  violations:  %d\n' "$total"
    printf '  allowlisted: %d\n' "$allow_hits"
    printf '  expired:     %d\n\n' "${#expired[@]}"
    if (( total > 0 )); then
        printf 'Production panic!/todo!/unimplemented! (first 50):\n'
        for v in "${violations[@]:0:50}"; do printf '  - %s\n' "$v"; done
    fi
    (( total == 0 && ${#expired[@]} == 0 )) && printf 'OK — no unallowed production panics.\n'
fi

if [[ "$MODE" == "strict" ]] && (( total > 0 || ${#expired[@]} > 0 )); then
    exit 1
fi
exit 0
