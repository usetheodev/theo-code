#!/usr/bin/env bash
# check-allowlist-progress.sh
#
# T0.1 of docs/plans/god-files-2026-07-23-plan.md.
#
# Reads `.claude/rules/size-allowlist.txt` and reports progress against
# the 2026-04-28 baseline:
#   - entries_remaining
#   - total_loc_above_default_ceiling
#   - largest_remaining
#   - WOULD_FAIL list (entries whose current LOC exceeds their ceiling)
#
# Usage:
#   scripts/check-allowlist-progress.sh                 # default report
#   scripts/check-allowlist-progress.sh --table         # markdown table
#   scripts/check-allowlist-progress.sh --baseline      # compare to docs/audit/god-files-baseline-2026-04-28.md
#
# Exit codes:
#   0 always (informational; this is not a gate, it's progress tracking)

set -euo pipefail
shopt -s globstar

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

ALLOWLIST=".claude/rules/size-allowlist.txt"
DEFAULT_CEILING=800
MODE="default"

for arg in "$@"; do
    case "$arg" in
        --table)    MODE="table" ;;
        --baseline) MODE="baseline" ;;
        --help|-h)
            sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

entries_remaining=0
total_above_default=0
largest_remaining_path=""
largest_remaining_loc=0
would_fail=()

declare -a TABLE_ROWS

while IFS='|' read -r path ceiling _sunset _reason; do
    case "$path" in
        ''|'#'*) continue ;;
    esac
    [[ -z "$ceiling" ]] && continue

    entries_remaining=$((entries_remaining + 1))

    if [[ -f "$path" ]]; then
        current=$(wc -l < "$path")
    else
        current=0
    fi

    above_default=$(( current > DEFAULT_CEILING ? current - DEFAULT_CEILING : 0 ))
    total_above_default=$((total_above_default + above_default))

    if (( current > largest_remaining_loc )); then
        largest_remaining_loc=$current
        largest_remaining_path=$path
    fi

    if (( current > ceiling )); then
        would_fail+=("$path (current=$current > ceiling=$ceiling)")
    fi

    TABLE_ROWS+=("$path|$ceiling|$current")
done < "$ALLOWLIST"

case "$MODE" in
    table)
        printf '| Path | Ceiling | Current | Headroom |\n'
        printf '|---|---:|---:|---:|\n'
        for row in "${TABLE_ROWS[@]}"; do
            IFS='|' read -r p c cur <<< "$row"
            printf '| %s | %s | %s | %s |\n' "$p" "$c" "$cur" "$((c - cur))"
        done
        ;;
    baseline)
        BASELINE="docs/audit/god-files-baseline-2026-04-28.md"
        if [[ ! -f "$BASELINE" ]]; then
            echo "baseline missing: $BASELINE" >&2
            exit 0
        fi
        # Match table rows of the per-entry table (path always starts with `crates/` or `apps/`):
        baseline_count=$(grep -cE '^\| [0-9]+ \| `(crates|apps)/' "$BASELINE" || true)
        printf 'baseline:    %d entries (frozen 2026-04-28)\n' "$baseline_count"
        printf 'current:     %d entries (now)\n' "$entries_remaining"
        printf 'delta:       %d (negative = progress)\n' "$((entries_remaining - baseline_count))"
        ;;
    *)
        printf 'allowlist progress\n'
        printf '  entries remaining:               %d\n' "$entries_remaining"
        printf '  total LOC above default ceiling: %d\n' "$total_above_default"
        if [[ -n "$largest_remaining_path" ]]; then
            printf '  largest remaining:               %s (%d LOC)\n' "$largest_remaining_path" "$largest_remaining_loc"
        fi
        if (( ${#would_fail[@]} > 0 )); then
            printf '\nWOULD_FAIL (current > ceiling):\n'
            for w in "${would_fail[@]}"; do printf '  - %s\n' "$w"; done
        else
            printf '  WOULD_FAIL:                      0 (every entry within its ceiling)\n'
        fi
        ;;
esac

exit 0
