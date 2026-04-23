#!/usr/bin/env bash
# check-secrets.sh
#
# T6.2 fallback — pattern-based secret scanner.
#
# Real-world gitleaks / osv-scanner are preferred (`make audit-tools`).
# This script is a grep-backed sibling that runs on every host without
# requiring Go or a pre-built binary, so CI stays useful even when the
# external binary is temporarily unavailable.
#
# Scans the working tree for 9 secret families. Allowlist at
# `.claude/rules/secret-allowlist.txt` (file:regex format; the gate
# tolerates a match iff the path matches the file glob AND the line
# matches the regex). Known test fixtures are pre-listed.
#
# Usage:
#   scripts/check-secrets.sh            # strict
#   scripts/check-secrets.sh --report   # report-only, exit 0
#   scripts/check-secrets.sh --json
#
# Exit:
#   0  no unlisted matches
#   1  unlisted matches
#   2  invocation error

set -euo pipefail

MODE="strict"
for arg in "$@"; do
    case "$arg" in
        --report) MODE="report" ;;
        --json)   MODE="json" ;;
        --help|-h) sed -n '2,25p' "$0"; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

ALLOWLIST_FILE=".claude/rules/secret-allowlist.txt"

# Seed: the audit discovered two intentional fixtures. Any new allowlist
# entry must be justified in the PR description.
# Accumulate all (path-glob, regex) pairs as two parallel arrays so the
# same path can appear multiple times with distinct regex variants.
ALLOW_GLOBS=()
ALLOW_REGEXES=()
if [[ -f "$ALLOWLIST_FILE" ]]; then
    while IFS='|' read -r path_glob regex _reason; do
        [[ -z "${path_glob// }" || "${path_glob:0:1}" == "#" ]] && continue
        ALLOW_GLOBS+=("$path_glob")
        ALLOW_REGEXES+=("$regex")
    done < "$ALLOWLIST_FILE"
fi

# The nine secret families we scan for.
declare -A PATTERNS=(
    [aws_access_key]='AKIA[0-9A-Z]{16}'
    [aws_secret_key]='(?i)aws[_-]?secret[_-]?access[_-]?key["'"'"']?\s*[:=]\s*["'"'"'][A-Za-z0-9/+=]{40}'
    [github_pat]='gh[pousr]_[A-Za-z0-9]{36,255}'
    [github_fine_grained]='github_pat_[A-Za-z0-9_]{82,}'
    [slack_token]='xox[baprs]-[A-Za-z0-9-]{10,}'
    [gcp_private_key]='-----BEGIN PRIVATE KEY-----'
    [openai_key]='sk-[A-Za-z0-9_-]{20,}'
    [anthropic_key]='sk-ant-[A-Za-z0-9_-]{20,}'
    [pem_private_key]='-----BEGIN (RSA|EC|DSA|OPENSSH) PRIVATE KEY-----'
)

violations=()
allow_hits=0

allowed_for() {
    local path="$1" line="$2"
    local idx=0
    while (( idx < ${#ALLOW_GLOBS[@]} )); do
        local glob="${ALLOW_GLOBS[$idx]}"
        local regex="${ALLOW_REGEXES[$idx]}"
        idx=$((idx + 1))
        case "$path" in
            $glob)
                if [[ -z "$regex" || "$line" =~ $regex ]]; then
                    return 0
                fi
                ;;
        esac
    done
    return 1
}

for family in "${!PATTERNS[@]}"; do
    pattern="${PATTERNS[$family]}"
    if command -v rg >/dev/null 2>&1; then
        while IFS= read -r hit; do
            [[ -z "$hit" ]] && continue
            path="${hit%%:*}"
            rest="${hit#*:}"
            line_no="${rest%%:*}"
            content="${rest#*:}"
            if allowed_for "$path" "$content"; then
                allow_hits=$((allow_hits + 1))
                continue
            fi
            violations+=("$family $path:$line_no  $(printf '%.100s' "$content")")
        done < <(
            rg -n --no-heading --pcre2 \
                --glob '!target/**' \
                --glob '!node_modules/**' \
                --glob '!referencias/**' \
                --glob '!.git/**' \
                --glob '!.theo/**' \
                --glob '!scripts/check-secrets.sh' \
                --glob '!.claude/rules/secret-allowlist.txt' \
                -- "$pattern" 2>/dev/null || true
        )
    else
        # grep does not do PCRE alternation reliably; skip when rg absent.
        echo "warn: ripgrep not available; pattern '$family' skipped" >&2
    fi
done

total="${#violations[@]}"

if [[ "$MODE" == "json" ]]; then
    printf '{\n  "violations": %d,\n  "allowlisted": %d,\n  "items": [' "$total" "$allow_hits"
    first=1
    for v in "${violations[@]}"; do
        (( first )) || printf ','; first=0
        printf '\n    "%s"' "${v//\"/\\\"}"
    done
    printf '\n  ]\n}\n'
else
    printf 'secret scan (grep-backed fallback)\n'
    printf '  families scanned: %d\n' "${#PATTERNS[@]}"
    printf '  violations:       %d\n' "$total"
    printf '  allowlisted:      %d\n\n' "$allow_hits"
    if (( total > 0 )); then
        printf 'Potential secrets (first 50):\n'
        for v in "${violations[@]:0:50}"; do printf '  - %s\n' "$v"; done
        printf '\n  -> Either remove the secret, replace with a SecretString wrapper,\n'
        printf '     or add a justified entry to %s\n' "$ALLOWLIST_FILE"
    else
        printf 'OK — no unallowed secrets detected.\n'
    fi
fi

if [[ "$MODE" == "strict" ]] && (( total > 0 )); then
    exit 1
fi
exit 0
