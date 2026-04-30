---
phase: 3+4
phase_name: refine_validate
iteration: 5
date: 2026-04-29
hypothesis: H5 query_type_classifier
status: KEEP
---

# Cycle 5 — Query-type Classifier (TDD, additive only)

## Hypothesis

Cycle 4 showed dense and BM25 win on different query categories. The
first concrete building block for the future router is a pure-function
classifier `classify(query: &str) -> QueryType` that returns one of
`Identifier | NaturalLanguage | Mixed`. With this in place, a future
cycle can wire it into `retrieve_files` to dispatch the right ranker
per query.

## Scope

- **Pure additive**: new file
  `crates/theo-engine-retrieval/src/search/query_type.rs` plus a
  `mod query_type;` and `pub use query_type::{QueryType, classify}` in
  `search/mod.rs`.
- **No mutations** to existing code, no changes to any production
  pipeline behaviour, no surface API removed or renamed.
- **No allowlist / Makefile / CLAUDE.md / gate-script change**.

## TDD

### RED → GREEN inline

10 tests calibrated against the actual 30-query `theo-code` ground
truth (so the classifier is validated against the same evidence used
in cycle-4 analysis):

| Test | Query | Expected |
|---|---|---|
| `classify_snake_case_single_token_is_identifier` | `assemble_greedy`, `propagate_attention`, `louvain_phase1` | Identifier |
| `classify_pascal_case_single_token_is_identifier` | `AgentRunEngine`, `TurboQuantizer` | Identifier |
| `classify_camel_case_single_token_is_identifier` | `getUserById` | Identifier |
| `classify_pascal_plus_lowercase_word_is_mixed` | `AgentRunEngine execute`, `TurboQuantizer quantize` | Mixed |
| `classify_all_lowercase_words_is_natural_language` | `agent loop state machine transitions` | NaturalLanguage |
| `classify_acronym_plus_words_is_mixed` | `BM25 scoring tokenization` | Mixed |
| `classify_short_token_alone_is_natural_language` | `id` | NaturalLanguage |
| `classify_pure_acronym_alone_is_mixed` | `HTML`, `BM25` | Mixed |
| `classify_two_identifiers_is_identifier` | `assemble_greedy AgentRunEngine` | Identifier |
| `classify_empty_is_natural_language` | empty / whitespace-only | NaturalLanguage |

### Implementation summary

- `is_identifier_like(word)` — true if length ≥ 3, ASCII alphanumeric +
  `_`, AND has either a snake boundary (`_` between alphanumerics) or
  a camel boundary (lowercase → uppercase).
- `is_plain_word(word)` — true if all-lowercase ASCII, length ≥ 2.
- `classify(query)` — composes counts of each shape across whitespace-
  separated words, with disambiguation rules calibrated to the test
  matrix above.

## Validate

| Gate | Result |
|---|---|
| `cargo test -p theo-engine-retrieval --lib` | ✅ **243 / 243 PASS**, 5 ignored (was 233 / 233 — the 10 new query_type tests) |
| `cargo clippy -p theo-engine-retrieval --all-targets -- -D warnings` | ✅ clean |
| Existing benchmarks | unchanged — classifier is not yet wired into `retrieve_files` |
| Public API surface | added `QueryType`, `classify`; nothing removed |
| Forbidden paths | none touched |

## Decision: KEEP

Pure additive change with full test coverage and zero impact on
existing behaviour — meets every "tentative KEEP" criterion. No
regression possible by construction.

## What remains for the next cycle (out of scope here)

To actually move the in-tree MRR floor, the classifier needs to be:

1. Imported by `file_retriever::retrieve_files` (one cycle).
2. Used to switch between BM25 (Identifier), Dense+RRF (NaturalLanguage),
   and the existing hybrid (Mixed). That requires the embedding cache
   and tantivy index to be passed in via `RerankConfig` or a new
   parameter — surface change, larger blast radius, **proper human gate
   recommended**.
3. A new benchmark `benchmark_routed_retrieve_files_mrr_guard`
   asserting per-category floors derived from cycle-4 evidence (e.g.
   identifier-MRR ≥ 0.75, semantic-MRR ≥ 0.70, …).

Each is a separate TDD cycle in a future loop session.

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->
<!-- QUALITY_SCORE:0.90 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_3_COMPLETE -->
<!-- PHASE_4_COMPLETE -->
