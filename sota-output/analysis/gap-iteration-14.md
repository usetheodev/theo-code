---
phase: 2
phase_name: analyze
iteration: 14
date: 2026-04-30
top_failure: query-type routing not wired (dead infrastructure)
severity: HIGH
---

# Cycle 14 — Gap Analysis: Query-Type Routing Is Dead Code

## Re-Probe Confirms Cycle 12/13 Measurements (within noise)

| Probe | This run (2026-04-30) | Cycle 12 baseline | Δ |
|---|---|---|---|
| `benchmark_bm25_baseline` MRR | **0.593** | 0.593 | 0 |
| `benchmark_bm25_baseline` R@5 | **0.462** | 0.462 | 0 |
| `benchmark_bm25_baseline` DepCov | **0.767** | 0.767 | 0 |
| `benchmark_retrieve_files_dense_rrf_guard` MRR | **0.674** | 0.664 | +0.010 (noise) |
| `benchmark_retrieve_files_dense_rrf_guard` R@5 | **0.507** | 0.499 | +0.008 |
| `benchmark_retrieve_files_dense_rrf_guard` R@10 | **0.577** | 0.560 | +0.017 |
| `benchmark_retrieve_files_dense_rrf_guard` nDCG@5 | **0.495** | 0.489 | +0.006 |
| `benchmark_retrieve_files_dense_rrf_guard` harm_removals | 7 | 6 | +1 (noise) |
| Workspace lib tests | **4452 / 0 / 5** | 4452 / 0 / 5 | unchanged |
| `cargo clippy --workspace -- -D warnings` | clean | clean | unchanged |
| `make check-arch` | 0 violations / 16 crates | same | unchanged |

State is reproducible. The 4 retrieval dod-gates remain BELOW_FLOOR with
the same numerical posture documented in `sota-output/report/sota-validation-report.md`.

## What's Been Tried (cycles 1-13)

| Cycle | Approach | Outcome |
|---|---|---|
| 1, 2 | `harm_filter` bug fixes | KEEP |
| 3 | Ground-truth path refresh (25 stale paths) | KEEP — +37% MRR |
| 4 | Dense+RRF qualitative analysis | INVESTIGATION (motivated cycle 5) |
| 5 | `query_type::classify` added | KEEP |
| 6 | `query_type` threaded into `FileRetrievalResult` | KEEP |
| 7 | Full Jina v2 dense + reranker | OOM (REJECT) |
| 8 | BM25 + reranker | flaky, abandoned |
| 9 | Blend grid search (8 weight tuples, no dense) | DISCARD — all regress |
| 10 | depcov drift identification (1.000 → 0.767) | DOC FIX |
| 11 | `retrieve_files_dense_rrf` (k=60) | KEEP — +13% MRR |
| 12 | Tune `rrf_k` 60 → 20 | KEEP — strict dominance over BM25 on 4 metrics |
| 13 | AllMiniLM-Q + BGE-Base reranker | OOM (REJECT) |

Pre-loop (commit `31a3537`): `symbol_first_search` experiment was tried
and rejected as REGRESSION (R@5 −0.062, MRR −0.062). Code retained as
documentation but not active.

## Gap Inventory

| dod-gate | Floor | Best measurement | Status |
|---|---|---|---|
| `retrieval.mrr` | 0.90 | 0.674 | BELOW_FLOOR (gap −0.226) |
| `retrieval.recall_at_5` | 0.92 | 0.507 | BELOW_FLOOR (gap −0.413) |
| `retrieval.recall_at_10` | 0.95 | 0.577 | BELOW_FLOOR (gap −0.373) |
| `retrieval.ndcg_at_5` | 0.85 | 0.495 | BELOW_FLOOR (gap −0.355) |
| `retrieval.depcov` | 0.96 | 0.767 | BELOW_FLOOR (gap −0.193) |
| `retrieval.per_language_recall_at_5` | 0.85 | unmeasured | UNMEASURED |

## The Untried Path: Query-Type Routing

### Evidence the infrastructure is already in place but dead

```
$ grep -n "QueryType::\|match query_type\|query_type ==" \
    crates/theo-engine-retrieval/src/file_retriever.rs
(no matches)
```

```
$ grep -n "query_type" crates/theo-engine-retrieval/src/file_retriever.rs
17: use crate::search::{FileBm25, QueryType, classify, tokenise};
71:     pub query_type: QueryType,
316:        query_type: classify(query),
```

The classifier (cycle 5) is wired into the result struct (cycle 6), but
**no production code branches on `query_type`**. It's a recorded
attribute, not a dispatch key. This is dead infrastructure from the
SOTA loop's own previous cycles.

### Per-query evidence that routing should help

From the cycle-14 BM25 baseline run (per-query MRR):

| Query type (classifier) | Worst BM25 MRR examples | Best BM25 MRR examples |
|---|---|---|
| **Identifier / Mixed** (exact-token bias) | theo-sym-001 'assemble_greedy' MRR=0.33; theo-sym-003 'louvain_phase1' MRR=0.25 | theo-sym-002 'propagate_attention' MRR=1.00; theo-sym-005 'TurboQuantizer quantize' MRR=1.00; theo-mod-008 'RRF reciprocal rank fusion hybrid search' MRR=1.00 |
| **NaturalLanguage** (semantic bias) | theo-xcut-003 'error types defined across crates' MRR=**0.01**; theo-xcut-006 'domain types shared across crates traits' MRR=**0.01**; theo-mod-002 'community detection clustering algorithm' MRR=**0.02**; theo-xcut-002 'BM25 scoring tokenization' MRR=0.08; theo-xcut-001 'sandbox security tests' MRR=0.14; theo-sem-001 'error handling recovery retry' MRR=0.17; theo-sem-002 'token budget enforcement truncation' MRR=0.20 | theo-xcut-005 'compaction context window management' MRR=1.00 |

The pattern is structural: BM25 catastrophically fails on long
natural-language queries (≥ 4 NL tokens). It works well on Identifier
queries because lexical match is the right signal. Cycle 4 explicitly
documented this — and motivated the (still-unwired) router.

### Per-category breakdown reinforces it

Cycle-14 BM25 baseline by query category:

| Category | R@5 | MRR | nDCG@5 | DepCov |
|---|---|---|---|---|
| symbol | 0.595 | **0.798** | 0.568 | 0.429 |
| module | 0.512 | 0.669 | 0.481 | 1.000 |
| semantic | 0.523 | 0.546 | 0.440 | 0.875 |
| **cross-cutting** | **0.202** | **0.356** | **0.211** | 0.714 |

`cross-cutting` queries (the most NL-heavy) lose 56% of MRR vs `symbol`.
Dense+RRF specifically targets semantic understanding. The aggregate
cycle-12 metric of 0.674 MRR likely hides the same per-category split,
but in the opposite direction (Dense+RRF rescues NL queries while
slightly hurting Identifier queries — exactly what cycle 4 found
qualitatively).

## Hypothesis (Cycle 14)

**Build a routed retriever**: at query time, classify the query via
`crate::search::classify`, then dispatch:

| QueryType | Ranker | Rationale |
|---|---|---|
| `Identifier` | `retrieve_files` (BM25) | Exact-token match is the right signal; cycle-4 observed Dense+RRF regression here |
| `NaturalLanguage` | `retrieve_files_dense_rrf` (k=20) | Semantic understanding required; cycle-12 evidence shows uplift on NL queries |
| `Mixed` | `retrieve_files_dense_rrf` (k=20) | Cycle-12 winning configuration; mixed queries usually contain at least one NL token |

### Falsifiability

The routed metric BEAT the cycle-12 winning configuration on at least
**one** of {MRR, R@5, R@10, nDCG@5} without regressing any of the others
by more than 0.01 absolute. Otherwise: DISCARD.

### Hardware-feasibility

This composes existing functions. No new model. No new memory pressure.
Both `retrieve_files` and `retrieve_files_dense_rrf` already run on
this 8 GB hardware (cycles 11-12 evidence).

### Why this is genuinely untried

- Cycle 4 motivated routing but no cycle implemented it
- Cycles 5-6 built the classifier but no cycle dispatched on it
- Cycle 11-12 wired Dense+RRF as an alternative entry point but always
  applied to all queries
- Cycle 13 tried adding a reranker, not switching ranker per query

## Why This Cycle Should Not Be Skipped For "Convergence"

Cycle 13's report concluded "Convergence is now structurally complete"
based on the OOM ceiling for the cross-encoder reranker. That
conclusion is accurate for the **reranker path**. It does NOT cover
the **routing path**, which (a) requires no model the OOM blocks,
(b) re-uses already-measured-green code, and (c) operationalizes
infrastructure built in cycles 5-6 specifically for this purpose.

If the hypothesis fails empirically, that fact closes a real
investigation channel and the loop converges with one more falsified
path documented. If it succeeds, it moves at least one of the 4
BELOW_FLOOR retrieval gates closer to its floor.

## Markers

<!-- FEATURES_STATUS:total=123,passing=37,failing=4 -->
<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_2_COMPLETE -->
