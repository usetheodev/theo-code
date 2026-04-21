# Evolution Assessment — Tool Calling 2.0 (cycle 2026-04-20 T14:00:00Z)

**Prompt:** Anthropic Tool Calling 2.0 — programmatic tool calling,
dynamic filtering, deferred loading, input examples.
**State-file baseline:** 72.300. **Post-fix score:** 73.272 (+0.972).
**Branch:** `develop` (commit `ca2610f`).

## Summary

No P1/P2/P3 implementation required. Every target feature in the
prompt had already landed in prior cycles:

- **P1** `input_examples` field on `ToolSchema`, emitted into JSON
  Schema, populated on the 5 top tools (edit, read, grep, bash,
  apply_patch).
- **P2** `filter_html` + `llm_suffix` citing dropped char count in
  webfetch, 10 dedicated tests.
- **P3** `BatchTool` with RunEngine intercept.
- Tool search (deferred loading), truncation rule sanitizer,
  format_validation_error, llm_suffix — all in the trait.

The only code change this cycle was a hygiene fix removing a duplicate
`crates/theo-infra-memory/src/retrieval.rs` that was causing rustc
`E0761` (3/13 crates failed to compile). Deleting the stray file
restored the workspace to 13/13 compile and raised the harness score
from 69.266 back to 73.272.

## Rubric scores

| Dimensão | Score | Evidência |
|---|:---:|---|
| Pattern Fidelity | 3/3 | The prior cycles already cite Anthropic Tool Calling 2.0 in commit bodies (deferred loading, examples, dynamic filtering). No regression this cycle. |
| Architectural Fit | 3/3 | `theo-domain → nothing` preserved; the duplicate file removal moves back toward the canonical module layout (`retrieval/mod.rs` + `retrieval/tantivy_adapter.rs`). |
| Completeness | 3/3 | All 3 targets of the prompt verified present via grep + tests. No gaps. |
| Testability | 3/3 | Webfetch has 10 named `html_filter_*` tests; tool trait feature surfaces are exercised throughout the workspace (2848 tests passing). |
| Simplicity | 3/3 | Single-file deletion. No new abstractions. |

**Média: 3.0 / 3.0** ≥ 2.5 → CONVERGED.

## Hygiene

| Metric | Pre-fix | Post-fix | Delta |
|---|---|---|---|
| Harness score | 69.266 | 73.272 | +4.006 |
| L1 | 88.031 | 96.044 | +8.013 |
| L2 | 50.500 | 50.500 | 0 |
| compile_crates | 10/13 | 13/13 | +3 |
| tests_passed | 2054 | 2848 | +794 |
| tests_failed | 4 | 4 | 0 (pre-existing bwrap) |
| cargo_warnings | 9 | 39 | +30 (now counting all crates again) |
| clippy_warnings | 0 | 0 | 0 |

## Known follow-ups (not required by this cycle's prompt)

1. **Programmatic code-mode interpreter** (explicit stretch from the
   prompt) — full Python/JS sandbox for true programmatic tool calling.
   Not attempted this cycle; batch meta-tool is the minimum-viable
   replacement.
2. **Bwrap sandbox kernel permissions** — 4 tests fail on this
   workstation because the user namespace can't run `RTM_NEWADDR`.
   Environment-only; no code change indicated.
3. **Desktop pkg-config install** — unblocked earlier; commit
   `b5a1e22` landed the Tauri memory shim.

## Decision

**CONVERGED.** Targets already met before the cycle started; the one
commit required (`ca2610f`) was a hygiene repair. Promise emitted.
