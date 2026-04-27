#!/usr/bin/env bash
# check-changelog-phase-coverage.sh
#
# Closes Global DoD #7 of `docs/plans/sota-tier1-tier2-plan.md`:
# "CHANGELOG.md atualizado com entrada [Unreleased]/Added por phase".
#
# The plan has 17 phases (Phase 0..16). For each phase, this script
# checks the CHANGELOG.md `[Unreleased]` section for either:
#
#   (1) A literal "Phase N" mention, OR
#   (2) A mention of any of the phase's tied task IDs
#       (Phase N → TN.1 / TN.2 / ... per the plan)
#
# A phase is "covered" when at least one of those literals appears
# in the [Unreleased] block. Friction is the point: a phase ships
# WITH a CHANGELOG entry or fails this gate.
#
# Usage:
#   scripts/check-changelog-phase-coverage.sh           # strict
#   scripts/check-changelog-phase-coverage.sh --json    # CI consumption
#
# Exit codes:
#   0  every phase 0..16 has at least one mention
#   1  one or more phases have zero mentions
#   2  invocation error

set -euo pipefail

OUTPUT="text"
for arg in "$@"; do
    case "$arg" in
        --json)    OUTPUT="json" ;;
        --help|-h) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

CHANGELOG="CHANGELOG.md"
if [[ ! -f "$CHANGELOG" ]]; then
    echo "ERROR: $CHANGELOG not found" >&2
    exit 2
fi

# ---------------------------------------------------------------------------
# Phase → task IDs mapping (from sota-tier1-tier2-plan.md)
# ---------------------------------------------------------------------------

declare -A PHASE_TASKS=(
    [0]="T0.1"
    [1]="T1.1 T1.2"
    [2]="T2.1"
    [3]="T3.1"
    [4]="T4.1"
    [5]="T5.1 T5.2"
    [6]="T6.1"
    [7]="T7.1"
    [8]="T8.1"
    [9]="T9.1"
    [10]="T10.1"
    [11]="T11.1"
    [12]="T12.1"
    [13]="T13.1"
    [14]="T14.1"
    [15]="T15.1"
    [16]="T16.1"
)

PHASE_ORDER=(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)

# ---------------------------------------------------------------------------
# Extract the [Unreleased] section
# ---------------------------------------------------------------------------

# Everything between `## [Unreleased]` and the next `## [` line.
UNRELEASED="$(awk '
    /^## \[Unreleased\]/ { in_block = 1; next }
    in_block && /^## \[/ { exit }
    in_block { print }
' "$CHANGELOG")"

if [[ -z "$UNRELEASED" ]]; then
    echo "ERROR: CHANGELOG has no [Unreleased] section" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Audit
# ---------------------------------------------------------------------------

declare -A PHASE_HITS
declare -A PHASE_MARKERS
total_uncovered=0

for phase in "${PHASE_ORDER[@]}"; do
    tasks="${PHASE_TASKS[$phase]}"
    found=0
    markers=""
    # Check (1): "Phase N" literal (with word boundary so "Phase 1"
    # doesn't match "Phase 10"). Use word-boundary regex.
    if grep -qE "Phase ${phase}\b" <<< "$UNRELEASED"; then
        found=$((found + 1))
        markers+="Phase${phase} "
    fi
    # Check (2): each tied task ID. Word boundary again so T1.1 doesn't
    # match T1.10 (hypothetical).
    for task in $tasks; do
        # Escape `.` in task IDs.
        escaped="${task//./\\.}"
        if grep -qE "\b${escaped}\b" <<< "$UNRELEASED"; then
            found=$((found + 1))
            markers+="${task} "
        fi
    done
    PHASE_HITS[$phase]=$found
    PHASE_MARKERS[$phase]="${markers% }"
    if [[ $found -eq 0 ]]; then
        total_uncovered=$((total_uncovered + 1))
    fi
done

# ---------------------------------------------------------------------------
# Render report
# ---------------------------------------------------------------------------

if [[ "$OUTPUT" == "json" ]]; then
    printf '{\n'
    printf '  "phases": {\n'
    first=1
    for phase in "${PHASE_ORDER[@]}"; do
        [[ $first -eq 0 ]] && printf ',\n'
        first=0
        printf '    "%d": {"hits": %d, "tasks": "%s", "markers": "%s"}' \
            "$phase" "${PHASE_HITS[$phase]}" "${PHASE_TASKS[$phase]}" "${PHASE_MARKERS[$phase]}"
    done
    printf '\n  },\n'
    printf '  "uncovered_count": %d\n' "$total_uncovered"
    printf '}\n'
else
    printf 'CHANGELOG phase coverage (Global DoD #7)\n'
    printf '%s\n' "------------------------------------------------------------"
    printf '%-6s %-15s %-6s  %s\n' "Phase" "Tasks" "Hits" "Markers found"
    printf '%s\n' "------------------------------------------------------------"
    for phase in "${PHASE_ORDER[@]}"; do
        n="${PHASE_HITS[$phase]}"
        if [[ $n -eq 0 ]]; then
            mark="MISS"
        else
            mark=" OK "
        fi
        printf '[%s] P%-3d  %-15s  %-3d  %s\n' \
            "$mark" "$phase" "${PHASE_TASKS[$phase]}" \
            "$n" "${PHASE_MARKERS[$phase]}"
    done
    printf '%s\n' "------------------------------------------------------------"
    if [[ $total_uncovered -gt 0 ]]; then
        printf '✗ %d phase(s) have ZERO mention in [Unreleased].\n' "$total_uncovered"
        printf '  Add at least one CHANGELOG line per phase before release.\n'
    else
        printf '✓ Every phase 0..16 has at least one mention in\n'
        printf '  CHANGELOG.md [Unreleased]. DoD #7 closed.\n'
    fi
fi

if [[ $total_uncovered -gt 0 ]]; then
    exit 1
fi
exit 0
