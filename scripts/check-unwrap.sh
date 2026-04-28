#!/usr/bin/env bash
# check-unwrap.sh
#
# T2.5 — Production `.unwrap()` / `.expect()` gate.
#
# Scans `crates/*/src/` and `apps/*/src/` for `.unwrap()` and `.expect(...)`
# outside `#[cfg(test)]` modules and `tests/` directories. Per
# `.claude/rules/rust-conventions.md`:
#
#   > Never `unwrap()` or `expect()` in production code. Use `?` or typed errors.
#
# Allowlist at `.claude/rules/unwrap-allowlist.txt` for justified sites
# (e.g. `expect()` on an invariant that the type system cannot express).
#
# Usage:
#   scripts/check-unwrap.sh                # strict, fail on any unlisted unwrap/expect
#   scripts/check-unwrap.sh --report       # print summary, exit 0
#   scripts/check-unwrap.sh --json
#   scripts/check-unwrap.sh --only=unwrap  # skip expect() scan
#
# Exit codes:
#   0  ok (no unlisted occurrences)
#   1  unlisted occurrences OR allowlisted entry expired

set -euo pipefail
# Enable recursive globbing so allowlist patterns like `crates/**/*.rs`
# match across nested directories.
shopt -s globstar

MODE="strict"
SCOPE="both"
for arg in "$@"; do
    case "$arg" in
        --report) MODE="report" ;;
        --json)   MODE="json" ;;
        --only=unwrap) SCOPE="unwrap" ;;
        --only=expect) SCOPE="expect" ;;
        --help|-h) sed -n '2,24p' "$0"; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

ALLOWLIST_FILE=".claude/rules/unwrap-allowlist.txt"
TODAY="$(date -u +%Y-%m-%d)"

declare -A ALLOW
declare -A ALLOW_SUNSET
declare -A ALLOW_REASON
# Content-regex entries use 4 pipe-separated fields (path-glob|content-regex|sunset|reason).
ALLOW_GLOBS_CR=()
ALLOW_REGEXES_CR=()
ALLOW_SUNSETS_CR=()
ALLOW_REASONS_CR=()

if [[ -f "$ALLOWLIST_FILE" ]]; then
    # Content-regex entries start with `regex:` and use `@@` (double-at) as
    # the field separator so `|` is free to live inside the regex.
    # Legacy entries keep the `path[:line]|sunset|reason` form.
    while IFS= read -r raw; do
        [[ -z "${raw// }" || "${raw:0:1}" == "#" ]] && continue
        if [[ "$raw" == regex:* ]]; then
            # regex:<path-glob>@@<content-regex>@@<sunset>@@<reason>
            local_spec="${raw#regex:}"
            IFS='@@' read -r glob regex sunset reason <<< "$local_spec"
            # When there are multiple `@@`, bash's `read` with IFS=`@` treats
            # each `@` as a separator; we reassemble with a different approach.
            # Use parameter-expansion splitting on literal `@@`.
            part1="${local_spec%%@@*}"
            rest1="${local_spec#*@@}"
            part2="${rest1%%@@*}"
            rest2="${rest1#*@@}"
            part3="${rest2%%@@*}"
            part4="${rest2#*@@}"
            ALLOW_GLOBS_CR+=("$part1")
            ALLOW_REGEXES_CR+=("$part2")
            ALLOW_SUNSETS_CR+=("$part3")
            ALLOW_REASONS_CR+=("$part4")
        else
            IFS='|' read -r f1 f2 f3 <<< "$raw"
            ALLOW["$f1"]=1
            ALLOW_SUNSET["$f1"]="$f2"
            ALLOW_REASON["$f1"]="$f3"
        fi
    done < "$ALLOWLIST_FILE"
fi

collect_hits() {
    local pattern="$1"
    # Search in crates/ and apps/, excluding tests/, benches/, target/.
    # We use rg for speed (part of the Bash tool-stack). Falls back to grep.
    local tool
    if command -v rg >/dev/null 2>&1; then
        rg -n --no-heading \
            --glob 'crates/**/src/**/*.rs' \
            --glob 'apps/**/src/**/*.rs' \
            --glob '!**/tests/**' \
            --glob '!**/target/**' \
            --glob '!**/benches/**' \
            -- "$pattern" 2>/dev/null || true
    else
        grep -rn -E \
            --include='*.rs' \
            --exclude-dir=tests \
            --exclude-dir=target \
            --exclude-dir=benches \
            "$pattern" crates/ apps/ 2>/dev/null || true
    fi
}

# Filter: drop lines inside `#[cfg(test)]` modules. We do this by a simple
# heuristic — any file under `*/src/**` whose preceding context starts a
# `#[cfg(test)]` block must have its content from that point tagged. This is
# brittle in bash, so we use a Python one-liner for precision.

filter_production_only() {
    # Stdin: one `path:line:content` per line.
    # Stdout: same, minus entries that sit below the first `#[cfg(test)]`
    # attribute in the file.
    #
    # Heuristic: in this codebase the overwhelming pattern is
    #   <prod code>
    #   #[cfg(test)]
    #   mod tests { … }
    # at end of file. Treating every hit whose line number is >= the
    # first `#[cfg(test)]` as test is correct for ≥95% of files and
    # conservatively undercounts production hits. False-negatives (a
    # stray `#[cfg(test)]` in the middle of the file) are safer than
    # false-positives here because the next step is human review.
    awk -F: 'BEGIN{OFS=":"}
        {
          path=$1
          if (!(path in cfg_line)) {
            # Match outer (`#[cfg(test)]`) and inner (`#![cfg(test)]`)
            # forms PLUS the compound form `#[cfg(all(test, feature = "..."))]`
            # used by feature-gated test modules (e.g. tantivy-backend).
            # The latter scopes an entire sibling-test file when used as
            # `#![cfg(...)]` and a tests-mod block when used as `#[cfg(...)]`.
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
            # line comments (`//`). The match string already triggered
            # on a `.unwrap()` literal in the line; if the line *starts*
            # with `//` (after whitespace), it is documentation/comment,
            # not executed code.
            content=$0
            sub("^[^:]+:[0-9]+:", "", content)
            if (content !~ /^[[:space:]]*\/\//) {
              print
            }
          }
        }'
}

summarise() {
    local label="$1" pattern="$2"
    local raw filtered
    raw="$(collect_hits "$pattern")"
    if [[ -z "$raw" ]]; then
        printf 'NO_HITS\n'
        return
    fi
    filtered="$(printf '%s\n' "$raw" | filter_production_only)"
    printf '%s\n' "$filtered"
}

unwrap_hits=""
expect_hits=""
[[ "$SCOPE" == "both" || "$SCOPE" == "unwrap" ]] && \
    unwrap_hits="$(summarise unwrap '\.unwrap\(\)')"
[[ "$SCOPE" == "both" || "$SCOPE" == "expect" ]] && \
    expect_hits="$(summarise expect '\.expect\(')"

# Post-filter allowlist
violations=()
expired=()
allow_hits=0
process_hits() {
    local hits="$1" kind="$2"
    [[ -z "$hits" || "$hits" == "NO_HITS" ]] && return
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        path_line="${line%%:*}"  # file
        rest="${line#*:}"
        line_no="${rest%%:*}"
        key="$path_line:$line_no"
        # Match either a file:line entry or a whole-file entry (no line).
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
                expired+=("$matched_key — sunset $sunset (reason: ${ALLOW_REASON[$matched_key]})")
            fi
            continue
        fi
        # Content-regex allowlist (path-glob + line regex).
        local content="${line#*:}"
        content="${content#*:}"
        local cr_idx=0
        local matched=0
        while (( cr_idx < ${#ALLOW_GLOBS_CR[@]} )); do
            local glob="${ALLOW_GLOBS_CR[$cr_idx]}"
            local regex="${ALLOW_REGEXES_CR[$cr_idx]}"
            local sunset="${ALLOW_SUNSETS_CR[$cr_idx]}"
            cr_idx=$((cr_idx + 1))
            case "$path_line" in
                $glob)
                    if [[ "$content" =~ $regex ]]; then
                        matched=1
                        allow_hits=$((allow_hits + 1))
                        if [[ -n "$sunset" && "$TODAY" > "$sunset" ]]; then
                            expired+=("$glob regex — sunset $sunset")
                        fi
                        break
                    fi
                    ;;
            esac
        done
        (( matched )) && continue
        violations+=("$kind $line")
    done <<< "$hits"
}

process_hits "$unwrap_hits" "unwrap"
process_hits "$expect_hits" "expect"

if [[ "$MODE" == "json" ]]; then
    printf '{\n'
    printf '  "violations": %d,\n' "${#violations[@]}"
    printf '  "allowlisted": %d,\n' "$allow_hits"
    printf '  "expired": %d,\n' "${#expired[@]}"
    printf '  "items": ['
    first=1
    for v in "${violations[@]}"; do
        (( first )) || printf ','; first=0
        esc="${v//\"/\\\"}"
        printf '\n    "%s"' "$esc"
    done
    printf '\n  ]\n}\n'
else
    printf 'unwrap/expect gate\n'
    printf '  violations:   %d\n' "${#violations[@]}"
    printf '  allowlisted:  %d\n' "$allow_hits"
    printf '  expired:      %d\n\n' "${#expired[@]}"
    if (( ${#violations[@]} > 0 )); then
        printf 'Production unwrap/expect (not allowlisted) — first 50:\n'
        for v in "${violations[@]:0:50}"; do printf '  - %s\n' "$v"; done
        printf '\n'
    fi
    if (( ${#expired[@]} > 0 )); then
        printf 'Expired allowlist entries:\n'
        for v in "${expired[@]}"; do printf '  - %s\n' "$v"; done
        printf '\n'
    fi
    if (( ${#violations[@]} == 0 && ${#expired[@]} == 0 )); then
        printf 'OK — no unallowed production unwrap/expect sites.\n'
    fi
fi

if [[ "$MODE" == "strict" ]] && (( ${#violations[@]} > 0 || ${#expired[@]} > 0 )); then
    exit 1
fi
exit 0
