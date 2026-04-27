#!/usr/bin/env bash
# check-complexity.sh
#
# Closes Global DoD #6 (partial — `complexity` subset) of
# `docs/plans/sota-tier1-tier2-plan.md`:
# "code-audit checks (complexity, coverage, lint, size) verde em
#  TODOS os crates modificados".
#
# `lint` and `size` already have dedicated gates
# (`check-arch-contract.sh` + `cargo clippy -- -D warnings` and
# `check-sizes.sh`). `complexity` and `coverage` were the two
# subitems still unverified locally. This script closes
# `complexity`.
#
# Strategy: use `cargo clippy -W clippy::too_many_lines` (default
# threshold 100 LOC per function — the canonical Rust complexity
# heuristic). A baseline allowlist locks the current per-crate
# violation count so:
#
#   - existing 75-function debt doesn't block the gate today
#   - any new function that crosses 100 LOC in a crate that's
#     already at its allowlist ceiling fails the gate
#   - refactoring lowers a crate's count → bump the allowlist down
#     to lock the new floor
#
# Allowlist file: `.claude/rules/complexity-allowlist.txt`
# Format: `<crate>|<max-allowed-violations>|<reason>`
#
# Usage:
#   scripts/check-complexity.sh           # strict, exit != 0 on regression
#   scripts/check-complexity.sh --report  # never fail, print summary
#
# Exit codes:
#   0  every crate is at-or-below its allowlist ceiling
#   1  one or more crates above ceiling
#   2  invocation error

set -euo pipefail

MODE="strict"
for arg in "$@"; do
    case "$arg" in
        --report) MODE="report" ;;
        --help|-h) sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

ALLOWLIST_FILE=".claude/rules/complexity-allowlist.txt"

# The 16 cargo workspace crates excluding Tauri-backed apps. Same
# set used by `check-sota-dod.sh`.
CRATES=(
    theo-domain
    theo-engine-graph
    theo-engine-retrieval
    theo-governance
    theo-engine-parser
    theo-isolation
    theo-infra-mcp
    theo-tooling
    theo-infra-llm
    theo-agent-runtime
    theo-infra-auth
    theo-infra-memory
    theo-test-memory-fixtures
    theo-api-contracts
    theo-application
    theo
)

# Build `-p crate ...` cargo args.
CRATES_P=()
for c in "${CRATES[@]}"; do
    CRATES_P+=( -p "$c" )
done

# ---------------------------------------------------------------------------
# Run clippy and aggregate violations by crate
# ---------------------------------------------------------------------------

printf 'Running clippy with `-W clippy::too_many_lines` ...\n' >&2

raw="$(cargo clippy "${CRATES_P[@]}" --lib --tests --no-deps \
    -- -W clippy::too_many_lines 2>&1 || true)"

# Parse: each "this function has too many lines" warning is followed
# (a few lines later) by `--> <path>:<line>:<col>`. Extract the crate
# from the path.
declare -A COUNT
while IFS= read -r path; do
    if [[ "$path" =~ ^crates/([^/]+) ]]; then
        crate="${BASH_REMATCH[1]}"
    elif [[ "$path" == apps/theo-cli/* ]]; then
        crate="theo"  # crate name `theo`, dir `apps/theo-cli`
    elif [[ "$path" == apps/* ]]; then
        # marklive / desktop / ui / benchmark — out of audit scope.
        continue
    else
        continue
    fi
    COUNT[$crate]=$(( ${COUNT[$crate]:-0} + 1 ))
done < <(printf '%s\n' "$raw" | awk '
    /this function has too many lines/ { has_warn = 1; next }
    has_warn && /^[[:space:]]*-->/ {
        has_warn = 0
        sub(/^[[:space:]]*-->[[:space:]]*/, "")
        sub(/:[0-9]+:[0-9]+$/, "")
        print
    }
')

# ---------------------------------------------------------------------------
# Load allowlist
# ---------------------------------------------------------------------------

declare -A ALLOWED
declare -A REASON
if [[ -f "$ALLOWLIST_FILE" ]]; then
    while IFS='|' read -r crate ceiling reason; do
        # Skip blanks + comments.
        [[ -z "${crate// }" || "${crate:0:1}" == "#" ]] && continue
        ALLOWED[$crate]="$ceiling"
        REASON[$crate]="$reason"
    done < "$ALLOWLIST_FILE"
fi

# ---------------------------------------------------------------------------
# Compare and report
# ---------------------------------------------------------------------------

total=0
violations=0
all_crates=$(printf '%s\n' "${CRATES[@]}" | sort)

printf '\ncomplexity gate (clippy::too_many_lines, threshold=100 LOC)\n'
printf '%s\n' "------------------------------------------------------------"
printf '%-30s %8s %8s  %s\n' "crate" "got" "ceiling" "status"
printf '%s\n' "------------------------------------------------------------"

for crate in $all_crates; do
    got=${COUNT[$crate]:-0}
    ceiling=${ALLOWED[$crate]:-0}
    total=$(( total + got ))
    if (( got > ceiling )); then
        status="REGRESSION (+$((got - ceiling)))"
        violations=$(( violations + 1 ))
    elif (( got < ceiling && ceiling > 0 )); then
        status="below ceiling — refresh allowlist"
    else
        status="ok"
    fi
    printf '%-30s %8d %8d  %s\n' "$crate" "$got" "$ceiling" "$status"
done

printf '%s\n' "------------------------------------------------------------"
printf 'total violations: %d (sum across all crates)\n' "$total"

if (( violations > 0 )); then
    if [[ "$MODE" == "strict" ]]; then
        printf '✗ %d crate(s) regressed past their complexity ceiling.\n' "$violations" >&2
        printf '  Either refactor the new oversized function OR raise\n' >&2
        printf '  the ceiling in `%s`\n' "$ALLOWLIST_FILE" >&2
        printf '  with a reason naming the offending function.\n' >&2
        exit 1
    fi
    printf '⚠ %d crate(s) regressed (--report mode, not failing).\n' "$violations" >&2
    exit 0
fi

printf '✓ Every crate is at-or-below its complexity ceiling.\n'
exit 0
