#!/usr/bin/env bash
# check-env-var-coverage.sh
#
# Structural audit for SOTA-documented env vars: every `THEO_*`
# (or related) env var mentioned in CHANGELOG / docs / plans MUST
# be referenced somewhere in production source (Rust, CI YAML,
# bench Python). A documented-but-unread env var is dead
# documentation that misleads users into setting a flag that
# does nothing.
#
# Same lesson as iter-25/26/27 (CONTENT ≠ STRUCTURAL):
#   - CONTENT: env var is documented in CHANGELOG.
#   - STRUCTURAL: code actually reads / honours that env var.
# Both are needed. This script closes the structural side.
#
# Scope: only env vars in the curated SOTA list below. The
# reverse direction (production-read vars not documented) is
# out of scope — too noisy with internal/test/template vars.
#
# Usage:
#   scripts/check-env-var-coverage.sh           # strict
#   scripts/check-env-var-coverage.sh --json    # CI consumption
#
# Exit codes:
#   0  every documented env var is referenced in source
#   1  one or more env vars are dead documentation
#   2  invocation error

set -uo pipefail

OUTPUT="text"
for arg in "$@"; do
    case "$arg" in
        --json)    OUTPUT="json" ;;
        --help|-h) sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

# ---------------------------------------------------------------------------
# Curated list of SOTA / user-facing env vars. Each MUST appear in
# production code somewhere. Keep this list tight: a generic
# "everything THEO_*" sweep produces too many false positives from
# internal template substitution vars (THEO_TOOL_DURATION_MS etc.)
# and test-only vars (THEO_TEST_EMPTY etc.).
# ---------------------------------------------------------------------------

ENV_VARS=(
    # T6.1 — adaptive replanning
    "THEO_AUTO_REPLAN|theo-cli pilot — opt-in to LLM-driven auto-replan on task failure"
    # T2.1 — browser sidecar
    "THEO_BROWSER_NODE|theo-tooling registry — override Node binary for Playwright sidecar"
    "THEO_BROWSER_SIDECAR|theo-tooling registry — override sidecar script path"
    # T9.1 — skill catalog
    "THEO_HOME|skill catalog discovery root (default ~/.theo)"
    # Phase 33-39 — MCP
    "THEO_MCP_AUTO_DISCOVERY|subagent — toggle auto-discovery of MCP tools"
    "THEO_MCP_DISCOVER_TIMEOUT_SECS|MCP discover CLI — per-server timeout override"
    # T14.1 — streaming UI
    "THEO_PROGRESS_STDERR|theo-cli headless — emit partial-progress chunks to stderr"
    # Phase 52-56 — prompt A/B
    "THEO_PROMPT_HOST|tbench setup — host serving prompt variant files"
    "THEO_PROMPT_VARIANT|tbench setup — name of the prompt variant to fetch"
    "THEO_SKIP_BIN_INSTALL|tbench setup — skip binary install (unit-test escape)"
    "THEO_SYSTEM_PROMPT_FILE|theo-cli — verbatim system-prompt override path"
    # T8.1 — reranker
    "THEO_RERANKER_PRELOAD|theo-application — opt-in lazy preload of cross-encoder model"
    # T10.1 — cost-aware routing
    "THEO_ROUTING_COST_AWARE|theo-infra-llm routing — toggle cost-aware classifier"
    # Phase 5 — onboarding
    "THEO_SKIP_ONBOARDING|theo-agent-runtime onboarding — skip first-run bootstrap"
    # Eval CI (DoD #10/#11 scaffold)
    "THEO_GROQ_API_KEY|eval.yml smoke job — Groq free-tier API key"
)

# Search roots for STRUCTURAL evidence.
SEARCH_ROOTS=(crates apps .github)

declare -a STALE=()
total=0

for entry in "${ENV_VARS[@]}"; do
    IFS='|' read -r var desc <<< "$entry"
    total=$((total + 1))
    # Word-boundary regex avoids THEO_AUTO_REPLAN matching THEO_AUTO_REPLANNING.
    if grep -RqE "\b${var}\b" "${SEARCH_ROOTS[@]}" 2>/dev/null; then
        :
    else
        STALE+=( "$var|$desc" )
    fi
done

# ---------------------------------------------------------------------------
# Render
# ---------------------------------------------------------------------------

if [[ "$OUTPUT" == "json" ]]; then
    printf '{\n  "total": %d,\n' "$total"
    printf '  "missing": [\n'
    first=1
    for s in "${STALE[@]}"; do
        IFS='|' read -r var desc <<< "$s"
        [[ $first -eq 0 ]] && printf ',\n'
        first=0
        printf '    {"var": "%s", "description": "%s"}' "$var" "$desc"
    done
    printf '\n  ],\n  "missing_count": %d\n}\n' "${#STALE[@]}"
else
    printf 'SOTA env-var coverage (CHANGELOG/plan → source)\n'
    printf '%s\n' "------------------------------------------------------------"
    printf '  documented env vars: %d\n' "$total"
    printf '  missing in source:   %d\n' "${#STALE[@]}"
    if [[ ${#STALE[@]} -gt 0 ]]; then
        printf '\n  Documented but NOT referenced anywhere in code:\n'
        for s in "${STALE[@]}"; do
            IFS='|' read -r var desc <<< "$s"
            printf '    [MISS] %s — %s\n' "$var" "$desc"
        done
    fi
    printf '%s\n' "------------------------------------------------------------"
    if [[ ${#STALE[@]} -gt 0 ]]; then
        printf '✗ %d documented env var(s) are dead documentation.\n' "${#STALE[@]}"
        printf '  Either implement the env-var read in production OR\n'
        printf '  remove the docs entry; otherwise users set a flag\n'
        printf '  that does nothing and waste time debugging.\n'
    else
        printf '✓ Every documented SOTA env var is referenced in\n'
        printf '  production source (rs/sh/yml/py). No dead\n'
        printf '  documentation.\n'
    fi
fi

exit $(( ${#STALE[@]} > 0 ? 1 : 0 ))
