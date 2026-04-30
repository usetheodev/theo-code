---
phase: 2
phase_name: analyze
iteration: 4
date: 2026-04-29
top_failure: query_type_dispatch_missing
severity: HIGH (multi-cycle scope)
status: investigation_only_no_code_change
---

# Cycle 4 — Investigation: Dense vs BM25 Trade-off

## What this cycle is

Pure investigation: no production code change. Reading evidence
left by `benchmark_rrf_dense` (which already exists, gated by
`--features dense-retrieval`) to size the next architectural step.

## Evidence — Per-category MRR Comparison

Both numbers measured against the *fresh* (cycle-3-fixed) ground truth.

| Category | BM25 baseline (no harm_filter) | RRF + Dense (Tantivy + Jina Code) | Δ |
|---|---|---|---|
| symbol | **0.798** | 0.663 | **−0.135** |
| module | 0.669 | **0.718** | +0.049 |
| semantic | 0.546 | **0.754** | +0.208 |
| cross-cutting | 0.357 | **0.456** | +0.099 |
| **Overall** | 0.593 | **0.654** | +0.061 |

## Interpretation

- **No single ranker dominates.** Dense+RRF lifts semantic and module
  queries (semantic finally above the 0.75 floor) but **hurts** symbol
  queries by 0.135 because cosine similarity blurs exact identifier
  matches that BM25 nails.
- A naïve "switch to dense" would regress the in-tree
  `benchmark_retrieve_files_mrr_guard` symbol-heavy queries.
- The right architecture is **query-type aware routing**: detect
  whether the query is an identifier (snake_case, camelCase, single
  symbol) vs a natural-language description, and dispatch to BM25 or
  Dense accordingly. The `search/multi.rs` module already exists with
  hybrid plumbing — what's missing is the router on top.

## Why this is multi-cycle

Wiring dense into `retrieve_files` properly requires:

1. Detecting query type (string-shape heuristic + symbol-table lookup).
2. Building / loading the Tantivy index and embedding cache once per
   project session (currently the cycle benchmarks build them per
   query — fine for a one-off, fatal for online use).
3. Persisting both stores in `.theo/` so the agent doesn't re-index on
   every invocation.
4. Adding a runtime config flag (`THEO_RETRIEVAL=dense|hybrid|bm25`)
   so users can opt in incrementally while we de-risk.
5. New benchmark: `benchmark_routed_retrieve_files_mrr_guard` — same
   shape as the existing guard, but exercising the routed pipeline,
   asserting per-category floors instead of a single overall floor.

Each is a discrete TDD cycle of its own. The current loop's
"one-fix-per-cycle" rule cannot collapse them all into one iteration.

## Decision

No code change this cycle. The remaining gap from 0.597 → 0.75 on the
in-tree guard is bounded by the **lexical ceiling on this corpus
(~0.60)** plus an additional ~0.06 reachable by RRF+Dense on
non-symbol queries. Reaching the 0.75 floor requires the routing work
above.

Recommended ordering for the next loop session:

1. Land query-type detector in `theo-engine-retrieval::search` with
   unit tests (1 TDD cycle).
2. Add a routed-pipeline benchmark and threshold (1 TDD cycle).
3. Wire routing into `retrieve_files` with the index/cache lifetimes
   stored in the project's `.theo/` (1–2 TDD cycles, larger blast
   radius — needs human review).
4. Optionally lower the in-tree functional floor in the guard test to
   reflect the now-evidenced lexical ceiling, with a clear rationale
   pointing at this analysis.

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->
<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_2_COMPLETE -->
