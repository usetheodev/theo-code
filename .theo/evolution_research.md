# Evolution Research — Tool Calling 2.0 (cycle 2026-04-20 T14:00:00Z)

**Prompt:** Anthropic Tool Calling 2.0 — programmatic tool calling, dynamic
filtering, deferred loading, input examples.

## Verification: all three P1/P2/P3 targets already landed on develop

| Target | Status | Evidence |
|---|---|---|
| **P1** `input_examples` on `ToolSchema` | ✅ LANDED | `theo-domain/src/tool.rs:208` declares `pub input_examples: Vec<serde_json::Value>`; `tool.rs:265-269` emits them into the JSON Schema output under the top-level `examples` key (OpenAI/Anthropic compatible). Populated on **5** top tools: edit, read, grep, bash, apply_patch (verified via grep of `input_examples: vec![` in each `mod.rs`). |
| **P2** Dynamic HTML filtering in webfetch | ✅ LANDED | `theo-tooling/src/webfetch/mod.rs:215` defines `filter_html(html) -> (String, usize)` stripping `script/style/nav/header/footer/noscript` + inline `on*=""` event handlers + collapsing runs of blank lines. Returns the char-count of noise removed. Caller at `mod.rs:194` emits `llm_suffix` citing the count ("[html-filter] Removed X chars ..."). 10 dedicated `html_filter_*` tests all green. |
| **P3** Batch meta-tool | ✅ LANDED | `theo-tooling/src/batch/mod.rs` declares `BatchTool` with `id() == "batch"`, schema accepting `calls: array`, max 25, intercepted by RunEngine (not executed inline). |
| Tool search deferred loading | ✅ LANDED | `Tool::should_defer` + `Tool::search_hint` default impls at `tool.rs:446-456`; `ToolRegistry::visible_definitions()` + `search_deferred()` + `tool_search` meta-tool confirmed via grep. |
| `TruncationRule` + sanitizer | ✅ LANDED | `tool.rs:469`. |
| `format_validation_error` | ✅ LANDED | `tool.rs:484`. |
| `ToolOutput::llm_suffix` + `with_llm_suffix` | ✅ LANDED | `tool.rs:27` field; `tool.rs:69-71` builder. |

## What, then, is the delta this cycle?

Zero net-new features required by the prompt. The only code change
needed this cycle was a **hygiene fix**: a duplicate
`crates/theo-infra-memory/src/retrieval.rs` left over from a
cross-branch `git restore` during an earlier merge. Both the stray
file and the canonical `retrieval/mod.rs + retrieval/tantivy_adapter.rs`
structure coexisted and caused rustc `E0761 file for module ... found
at both`. Removed the stray in `ca2610f`.

## Hygiene delta

| Metric | Pre-fix | Post-fix | Baseline goal |
|---|---|---|---|
| Harness score | 69.266 | 73.272 | ≥ 72.300 (state file) ✅ |
| compile_crates | 10/13 | 13/13 | 13/13 ✅ |
| tests passing | 2054 | 2848 | — |
| L1 | 88.031 | 96.044 | — |
| L2 | 50.500 | 50.500 | — |

The 4 remaining `tests_failed` entries are pre-existing `bwrap_*`
sandbox tests failing on the kernel's restricted user namespace
(`Operation not permitted` on `RTM_NEWADDR`) — environment, not code.

## Decision

No P1/P2/P3 implementation work required. Cycle converges immediately
on a hygiene fix that restored baseline compile + score after an
accidental file duplication.
