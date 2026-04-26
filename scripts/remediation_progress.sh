#!/usr/bin/env bash
# remediation_progress.sh — dashboard for docs/reviews/theo-agent-runtime/REMEDIATION_PLAN.md.
#
# Counts the quantitative exit criteria from the Definition of Done section of
# the remediation plan. Run before/after each phase to track progress.
#
# Usage:
#   scripts/remediation_progress.sh          # print plain text
#   scripts/remediation_progress.sh --json   # machine-readable

set -euo pipefail

CRATE="crates/theo-agent-runtime/src"
MODE="plain"
for arg in "$@"; do
    case "$arg" in
        --json) MODE="json" ;;
        --help|-h)
            sed -n '2,14p' "$0"
            exit 0
            ;;
        *)
            echo "unknown flag: $arg" >&2
            exit 2
            ;;
    esac
done

count_pattern() {
    # ripgrep count (treats errors / no-match as 0 so the script never aborts).
    local pattern="$1"
    rg --type rust -c "$pattern" "$CRATE" 2>/dev/null \
        | awk -F: '{s+=$2} END {print (s==""?0:s)}'
}

god_files=$(find "$CRATE" -type f -name '*.rs' -exec wc -l {} + 2>/dev/null \
    | awk '$2 != "total" && $1 > 500 {count++} END {print (count==""?0:count)}')

unwraps=$(count_pattern '\.expect\(|\.unwrap\(\)|panic!')
silent_swallow=$(count_pattern 'let _ = tokio::fs::|let _ = std::fs::')
env_reads=$(count_pattern 'std::env::var')
sync_cmd=$(count_pattern 'std::process::Command')
phase_tags=$(count_pattern 'Phase \d+')

if [[ "$MODE" == "json" ]]; then
    printf '{\n'
    printf '  "god_files_over_500_loc": %d,\n' "$god_files"
    printf '  "expect_unwrap_panic": %d,\n' "$unwraps"
    printf '  "silent_swallow": %d,\n' "$silent_swallow"
    printf '  "env_var_reads": %d,\n' "$env_reads"
    printf '  "sync_command": %d,\n' "$sync_cmd"
    printf '  "phase_tags": %d\n' "$phase_tags"
    printf '}\n'
else
    printf '=== theo-agent-runtime remediation progress ===\n\n'
    printf 'God-files (>500 LOC):\n'
    find "$CRATE" -type f -name '*.rs' -exec wc -l {} + 2>/dev/null \
        | awk '$2 != "total" && $1 > 500 {print}' \
        | sort -rn | head -20
    printf '\n'
    printf '.expect()/.unwrap()/panic! count:  %s\n' "$unwraps"
    printf 'silent-swallow count:             %s\n' "$silent_swallow"
    printf 'std::env::var count:              %s\n' "$env_reads"
    printf 'std::process::Command count:      %s\n' "$sync_cmd"
    printf 'Phase N tags count:               %s\n' "$phase_tags"
    printf '\nBaseline (see REMEDIATION_PLAN.md):\n'
    printf '  God-files ~20  unwraps ~1071  silent-swallow ~61\n'
    printf '  env::var ~25   sync-cmd ~2   phase-tags ~310\n'
fi
