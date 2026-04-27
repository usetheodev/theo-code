#!/usr/bin/env bash
# check-phase-artifacts.sh
#
# Closes the AUTOMATABLE half of Global DoD #1 of
# `docs/plans/sota-tier1-tier2-plan.md`: "All 16 phases completed".
#
# "Completed" couples with the empirical items #10/#11 (paid LLM
# API + bench runs) for the per-phase E2E manual validations
# (`theo "..."` invocations) — those are out-of-scope. But the
# CODE half is gate-able: each phase promised specific artifacts
# (types, fields, modules, tools) and we can grep-verify each
# one is present at the canonical site.
#
# This is a **content** audit (does the artifact appear at all?)
# not a behaviour audit (does the artifact work end-to-end?).
# The behaviour half lives in:
#   - `cargo test --workspace` (every artifact has at least one
#     test that exercises it)
#   - `default_registry_tool_id_snapshot_is_pinned` (every SOTA
#     tool id is in the registry)
#   - `audit.yml::eval` (the future bench job that needs paid API)
#
# Usage:
#   scripts/check-phase-artifacts.sh           # strict
#   scripts/check-phase-artifacts.sh --json    # CI consumption
#
# Exit codes:
#   0  every phase's artifacts are present
#   1  one or more phases have missing artifacts
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
# Artifact definitions
# ---------------------------------------------------------------------------
#
# Format: arrays in shell are awkward, so encode as parallel arrays.
# For each phase: name, path, regex (or fixed string).
#
# A phase is "covered" when EVERY one of its artifacts is found at
# its canonical path. Missing artifact → phase fails.

# Phase numbers in iteration order (Phase 0..16).
# Per-phase artifacts encoded as: phase|description|path|grep_pattern.
ARTIFACTS=(
    # Phase 0 — Multimodal foundation
    "0|ContentBlock enum (T0.1)|crates/theo-infra-llm/src/types.rs|enum ContentBlock"
    "0|Message.content_blocks field (T0.1)|crates/theo-infra-llm/src/types.rs|content_blocks: Option<Vec<ContentBlock>>"
    # Phase 1 — Multimodal / Vision
    "1|screenshot tool (T1.1)|crates/theo-tooling/src/screenshot/mod.rs|pub struct ScreenshotTool"
    "1|read_image tool (T1.2)|crates/theo-tooling/src/read_image/mod.rs|pub struct ReadImageTool"
    "1|read_image registered (T1.2)|crates/theo-tooling/src/registry/mod.rs|ReadImageTool"
    # Phase 2 — Browser automation
    "2|BrowserSessionManager (T2.1)|crates/theo-tooling/src/browser/session_manager.rs|pub struct BrowserSessionManager"
    "2|browser_status tool (T2.1 follow-up)|crates/theo-tooling/src/browser/tool.rs|pub struct BrowserStatusTool"
    "2|browser_open tool (T2.1)|crates/theo-tooling/src/browser/tool.rs|pub struct BrowserOpenTool"
    # Phase 3 — LSP
    "3|LspSessionManager (T3.1)|crates/theo-tooling/src/lsp/session_manager.rs|pub struct LspSessionManager"
    "3|lsp_status tool (T3.1 follow-up)|crates/theo-tooling/src/lsp/tool.rs|pub struct LspStatusTool"
    "3|lsp_rename PREVIEW (T3.1)|crates/theo-tooling/src/lsp/tool.rs|pub struct LspRenameTool"
    # Phase 4 — Computer Use
    "4|ComputerActionTool (T4.1)|crates/theo-tooling/src/computer/tool.rs|pub struct ComputerActionTool"
    # Phase 5 — Auto-test-gen
    "5|GenPropertyTestTool (T5.1)|crates/theo-tooling/src/test_gen/property.rs|pub struct GenPropertyTestTool"
    "5|GenMutationTestTool (T5.2)|crates/theo-tooling/src/test_gen/mutation.rs|pub struct GenMutationTestTool"
    # Phase 6 — Adaptive replanning
    "6|PlanPatch enum (T6.1)|crates/theo-domain/src/plan_patch.rs|enum PlanPatch"
    "6|PlanTask.failure_count (T6.1)|crates/theo-domain/src/plan.rs|pub failure_count: u32"
    "6|plan_replan tool (T6.1)|crates/theo-tooling/src/plan/mod.rs|pub struct ReplanTool"
    # Phase 7 — Multi-agent claim
    "7|PlanTask.assignee (T7.1)|crates/theo-domain/src/plan.rs|pub assignee: Option<String>"
    "7|Plan.version_counter (T7.1)|crates/theo-domain/src/plan.rs|pub version_counter: u64"
    # Phase 8 — Cross-encoder reranker
    "8|CrossEncoderReranker always-on (T8.1)|crates/theo-engine-retrieval/src/reranker.rs|pub use inner::CrossEncoderReranker"
    # Phase 9 — Skill marketplace
    "9|skill_catalog use case (T9.1)|crates/theo-application/src/use_cases/skills.rs|pub mod skills|pub fn list"
    # Phase 10 — Cost-aware routing
    "10|RoutingConfig.cost_aware (T10.1)|crates/theo-infra-llm/src/routing/config.rs|cost_aware"
    # Phase 11 — Compaction stages
    "11|compaction_stages module (T11.1)|crates/theo-agent-runtime/src/compaction_stages.rs|pub fn"
    # Phase 12 — Continuous SOTA evaluation
    "12|eval CI workflow (T12.1)|.github/workflows/eval.yml|name: eval"
    # Phase 13 — DAP
    "13|DapSessionManager (T13.1)|crates/theo-tooling/src/dap/session_manager.rs|pub struct DapSessionManager"
    "13|debug_status tool (T13.1 follow-up)|crates/theo-tooling/src/dap/tool.rs|pub struct DebugStatusTool"
    "13|debug_launch tool (T13.1)|crates/theo-tooling/src/dap/tool.rs|pub struct DebugLaunchTool"
    # Phase 14 — Live tool streaming
    "14|partial-progress emit_progress (T14.1)|crates/theo-tooling/src/partial.rs|pub fn emit_progress"
    "14|RuntimeContext.partial_progress_tx (T14.1)|crates/theo-agent-runtime/src/run_engine|partial_progress_tx"
    # Phase 15 — External docs RAG
    "15|DocsSearchTool (T15.1)|crates/theo-tooling/src/docs_search|pub struct DocsSearchTool"
    "15|MarkdownDirSource (T15.1)|crates/theo-tooling/src/docs_search|pub struct MarkdownDirSource"
    # Phase 16 — RLHF feedback export
    "16|EnvelopeKind::Rating (T16.1)|crates/theo-agent-runtime/src/observability/envelope.rs|Rating,"
    "16|trajectory_export module (T16.1)|crates/theo-agent-runtime/src/trajectory_export.rs|pub fn"
)

# ---------------------------------------------------------------------------
# Audit
# ---------------------------------------------------------------------------

declare -A PHASE_TOTAL
declare -A PHASE_HITS
declare -A PHASE_MISSES

# Custom IFS for parsing pipe-delimited tuples.
for tuple in "${ARTIFACTS[@]}"; do
    IFS='|' read -r phase desc path pattern <<< "$tuple"
    PHASE_TOTAL[$phase]=$(( ${PHASE_TOTAL[$phase]:-0} + 1 ))

    # Path can be either a single file OR a directory; grep -r handles both.
    found=0
    if [[ -e "$path" ]]; then
        if grep -qrE "$pattern" "$path" 2>/dev/null; then
            found=1
        fi
    fi

    if [[ $found -eq 1 ]]; then
        PHASE_HITS[$phase]=$(( ${PHASE_HITS[$phase]:-0} + 1 ))
    else
        PHASE_MISSES[$phase]+="${desc} @ ${path}|"
    fi
done

# ---------------------------------------------------------------------------
# Render report
# ---------------------------------------------------------------------------

PHASE_ORDER=(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
total_uncovered=0

if [[ "$OUTPUT" == "json" ]]; then
    printf '{\n  "phases": {\n'
    first=1
    for phase in "${PHASE_ORDER[@]}"; do
        [[ $first -eq 0 ]] && printf ',\n'
        first=0
        total=${PHASE_TOTAL[$phase]:-0}
        hits=${PHASE_HITS[$phase]:-0}
        printf '    "%d": {"artifacts": %d, "found": %d}' "$phase" "$total" "$hits"
        if [[ $total -ne $hits ]]; then
            total_uncovered=$((total_uncovered + 1))
        fi
    done
    printf '\n  },\n  "uncovered_phases": %d\n}\n' "$total_uncovered"
else
    printf 'Phase artifact coverage (Global DoD #1, code half)\n'
    printf '%s\n' "------------------------------------------------------------"
    printf '%-7s %s\n' "Phase" "Artifacts found / total"
    printf '%s\n' "------------------------------------------------------------"
    for phase in "${PHASE_ORDER[@]}"; do
        total=${PHASE_TOTAL[$phase]:-0}
        hits=${PHASE_HITS[$phase]:-0}
        if [[ $total -eq $hits ]]; then
            printf '[ OK ]  P%-3d  %d / %d\n' "$phase" "$hits" "$total"
        else
            printf '[MISS]  P%-3d  %d / %d  — missing:\n' "$phase" "$hits" "$total"
            misses="${PHASE_MISSES[$phase]}"
            while [[ -n "$misses" ]]; do
                first="${misses%%|*}"
                misses="${misses#*|}"
                [[ -n "$first" ]] && printf '          • %s\n' "$first"
            done
            total_uncovered=$((total_uncovered + 1))
        fi
    done
    printf '%s\n' "------------------------------------------------------------"
    if [[ $total_uncovered -gt 0 ]]; then
        printf '✗ %d phase(s) have missing artifacts.\n' "$total_uncovered"
        printf '  Either restore the artifact OR update the canonical\n'
        printf '  path / pattern in this script if it was renamed.\n'
    else
        printf '✓ Every phase 0..16 has all promised artifacts present.\n'
        printf '  (CODE half of DoD #1 closed; the BEHAVIOUR half via\n'
        printf '   E2E manuals + bench runs is OUT-OF-SCOPE for the\n'
        printf '   autonomous loop — see DoD #10/#11.)\n'
    fi
fi

if [[ $total_uncovered -gt 0 ]]; then
    exit 1
fi
exit 0
