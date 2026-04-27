#!/usr/bin/env bash
# check-adr-coverage.sh
#
# Closes Global DoD #8 of `docs/plans/sota-tier1-tier2-plan.md`:
# "ADRs D1-D16 referenciados nos commits relevantes".
#
# The plan defines 16 ADRs (D1-D16) each tied to one or more tasks
# (T0.1, T1.1, T2.1, ...). We can't directly grep `Dx` in commits
# (most commits reference task IDs, not ADR IDs), so the audit is
# transitive:
#
#   ADR Dx  →  tied to task IDs  →  commits mentioning those task IDs
#
# An ADR is considered "covered" when at least one commit on the
# branch mentions one of its tied task IDs. The mapping below comes
# from the "ADRs" section of the plan (lines 58-138).
#
# Usage:
#   scripts/check-adr-coverage.sh                  # full report
#   scripts/check-adr-coverage.sh --since=v1.0.0   # restrict log range
#   scripts/check-adr-coverage.sh --json           # machine-readable
#
# Exit codes:
#   0  every ADR has >= 1 commit referencing one of its tasks
#   1  one or more ADRs have zero commit coverage
#   2  invocation error

set -euo pipefail

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

SINCE=""
OUTPUT="text"
for arg in "$@"; do
    case "$arg" in
        --since=*)  SINCE="${arg#--since=}" ;;
        --json)     OUTPUT="json" ;;
        --help|-h)
            sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

# ---------------------------------------------------------------------------
# ADR → task mapping (from docs/plans/sota-tier1-tier2-plan.md lines 60-138)
# ---------------------------------------------------------------------------
#
# Format: ADR_TASKS[<adr>]="<task1> <task2> ..."

declare -A ADR_TASKS=(
    # D1 — Multimodal via blocos de conteúdo
    [D1]="T0.1 T1.1 T1.2"
    # D2 — Browser via Playwright sidecar
    [D2]="T2.1"
    # D3 — LSP via tower-lsp/lsp-types cliente
    [D3]="T3.1"
    # D4 — Replanning é uma operação do Plan
    [D4]="T6.1"
    # D5 — Multi-agent claim via assignee
    [D5]="T7.1"
    # D6 — Computer Use feature-gated por provider
    [D6]="T4.1"
    # D7 — Auto-test-gen via tools especializadas
    [D7]="T5.1 T5.2"
    # D8 — Reranker é always-on
    [D8]="T8.1"
    # D9 — Bump de schema para state e plans
    [D9]="T0.1 T6.1 T7.1"
    # D10 — Skill marketplace = agent_spec format
    [D10]="T9.1"
    # D11 — Cost routing usa complexity_hint no AgentConfig
    [D11]="T10.1"
    # D12 — Compactação Compact stage via LLM auxiliary
    [D12]="T11.1"
    # D13 — Eval CI usa modelo gratuito primeiro
    [D13]="T12.1"
    # D14 — DAP via debug_adapter_protocol cliente Rust
    [D14]="T13.1"
    # D15 — Streaming via canal MPSC PartialToolResult
    [D15]="T14.1"
    # D16 — RLHF dataset é apenas export
    [D16]="T16.1"
)

# Stable iteration order: D1, D2, ..., D16.
ADR_ORDER=(D1 D2 D3 D4 D5 D6 D7 D8 D9 D10 D11 D12 D13 D14 D15 D16)

# ---------------------------------------------------------------------------
# Audit helpers
# ---------------------------------------------------------------------------

git_log_args() {
    if [[ -n "$SINCE" ]]; then
        printf '%s\n' "$SINCE..HEAD"
    fi
}

# Count commits whose subject OR body mentions a literal task id like
# `T0.1` / `T6.1` / `T13.1`. Word-boundary regex avoids false-positive
# substring matches (e.g. "T16" inside "T160").
count_commits_for_task() {
    local task="$1"
    # Escape the dot in `T0.1` so it's a literal in the regex.
    local escaped="${task//./\\.}"
    # `\b` (GNU-grep extension) avoids matching `T13.10` for `T13.1`.
    local args
    args=$(git_log_args)
    if [[ -n "$args" ]]; then
        git log --grep="\b${escaped}\b" --extended-regexp --oneline "$args" 2>/dev/null | wc -l
    else
        git log --grep="\b${escaped}\b" --extended-regexp --oneline 2>/dev/null | wc -l
    fi
}

# ---------------------------------------------------------------------------
# Run audit
# ---------------------------------------------------------------------------

declare -A ADR_COMMIT_COUNT
declare -A ADR_TASK_HITS
total_uncovered=0

for adr in "${ADR_ORDER[@]}"; do
    tasks="${ADR_TASKS[$adr]}"
    sum=0
    hits=""
    for task in $tasks; do
        n=$(count_commits_for_task "$task")
        sum=$((sum + n))
        if [[ $n -gt 0 ]]; then
            hits+="${task}=${n} "
        fi
    done
    ADR_COMMIT_COUNT[$adr]=$sum
    ADR_TASK_HITS[$adr]="${hits% }"
    if [[ $sum -eq 0 ]]; then
        total_uncovered=$((total_uncovered + 1))
    fi
done

# ---------------------------------------------------------------------------
# Render report
# ---------------------------------------------------------------------------

if [[ "$OUTPUT" == "json" ]]; then
    printf '{\n'
    printf '  "since": "%s",\n' "$SINCE"
    printf '  "adrs": {\n'
    first=1
    for adr in "${ADR_ORDER[@]}"; do
        [[ $first -eq 0 ]] && printf ',\n'
        first=0
        printf '    "%s": {"commits": %d, "tasks": "%s", "task_hits": "%s"}' \
            "$adr" "${ADR_COMMIT_COUNT[$adr]}" "${ADR_TASKS[$adr]}" "${ADR_TASK_HITS[$adr]}"
    done
    printf '\n  },\n'
    printf '  "uncovered_count": %d\n' "$total_uncovered"
    printf '}\n'
else
    printf 'ADR coverage report (Global DoD #8 of sota-tier1-tier2-plan)\n'
    printf '%s\n' "------------------------------------------------------------"
    printf '%-4s  %-20s  %s\n' "ADR" "Tasks" "Commits"
    printf '%s\n' "------------------------------------------------------------"
    for adr in "${ADR_ORDER[@]}"; do
        n="${ADR_COMMIT_COUNT[$adr]}"
        if [[ $n -eq 0 ]]; then
            mark="MISS"
        else
            mark=" OK "
        fi
        printf '[%s] %-3s  %-20s  %d  (%s)\n' \
            "$mark" "$adr" "${ADR_TASKS[$adr]}" \
            "$n" "${ADR_TASK_HITS[$adr]}"
    done
    printf '%s\n' "------------------------------------------------------------"
    if [[ $total_uncovered -gt 0 ]]; then
        printf '✗ %d ADR(s) have ZERO commit coverage. Fix:\n' "$total_uncovered"
        printf '  - either add a commit referencing the missing task id\n'
        printf '  - or update the ADR_TASKS map in this script if the\n'
        printf '    plan was reorganised after this script was written.\n'
    else
        printf '✓ Every ADR (D1-D16) has at least one commit referencing\n'
        printf '  one of its tied task IDs.\n'
    fi
fi

if [[ $total_uncovered -gt 0 ]]; then
    exit 1
fi
exit 0
