---
phase: 3+4
phase_name: refine_and_validate
iteration: 14
date: 2026-04-30
hypothesis: query-type routing (Identifier→BM25, NL/Mixed→Dense+RRF) strictly improves over Dense+RRF k=20 on at least one of {MRR, R@5, R@10, nDCG@5} without regressing any > 0.01
status: REJECT HYPOTHESIS / KEEP CODE
result_summary: routing wins MRR (+0.021) but loses recall (R@5 −0.025, R@10 −0.039) — strict-dominance criterion not met; code retained as documentation of trade-off
---

# Cycle 14 — Query-Type Routing (REJECT HYPOTHESIS, KEEP CODE)

## Hypothesis (gap-iteration-14.md)

Cycles 5-6 added `QueryType::classify` and threaded it into
`FileRetrievalResult.query_type`, but no production code branched on
the value. Cycle 4 motivated routing qualitatively: BM25 dominates
Identifier queries (per-token exact match), Dense+RRF dominates
NaturalLanguage queries (semantic understanding).

**Cycle 14 hypothesis:** wiring the latent classifier into a routed
retriever (`Identifier → retrieve_files`, `NaturalLanguage|Mixed →
retrieve_files_dense_rrf`) should improve at least one of
{MRR, R@5, R@10, nDCG@5} versus the cycle-12 Dense+RRF k=20 baseline
without regressing any of them by more than 0.01 absolute.

## Implementation (TDD)

### RED — `benchmark_retrieve_files_routed_guard`

`crates/theo-engine-retrieval/tests/benchmark_suite.rs` — new
`#[ignore]`d benchmark behind `#[cfg(feature = "dense-retrieval")]`.
Asserts MRR ≥ 0.65 (cycle-12 floor calibration). Tightly compares
4 metrics for the keep/discard decision.

Confirmed RED:

```
$ cargo build -p theo-engine-retrieval --features dense-retrieval --tests
error[E0432]: unresolved import `theo_engine_retrieval::file_retriever::retrieve_files_routed`
```

### Hygiene (covered by user-approved option 2)

Removed unused `let k = 5;` at `benchmark_suite.rs:604`
(`benchmark_symbol_first_ab`) — pre-existing rustc warning surfaced
by the cycle-14 probe.

### GREEN — `pub fn retrieve_files_routed`

`crates/theo-engine-retrieval/src/file_retriever.rs` — single new
public entry point behind `dense-retrieval` feature, ~20 LOC of
dispatch:

```rust
match classify(query) {
    QueryType::Identifier =>
        retrieve_files(graph, communities, query, config, previously_seen),
    QueryType::NaturalLanguage | QueryType::Mixed =>
        retrieve_files_dense_rrf(/* ... */),
}
```

No new model, no new memory pressure. Composes existing
measured-green paths.

## Empirical Result (the data)

```
retrieve_files_routed: MRR=0.695  R@5=0.482  R@10=0.538  nDCG@5=0.485
                       (harm_removals=17, routed_to_bm25=4, routed_to_dense=26)
test benchmark_retrieve_files_routed_guard ... ok
```

| Metric | Routed (cycle 14) | Dense+RRF k=20 (cycle 14 probe) | Δ vs Dense+RRF | Verdict |
|---|---|---|---|---|
| **MRR** | **0.695** | 0.674 | **+0.021** | ✅ improved |
| **R@5** | 0.482 | 0.507 | **−0.025** | ❌ regressed > 0.01 |
| **R@10** | 0.538 | 0.577 | **−0.039** | ❌ regressed > 0.01 |
| **nDCG@5** | 0.485 | 0.495 | **−0.010** | ❌ regressed = 0.01 (floor) |
| harm_removals | 17 | 7 | +10 | (informational) |

Routing decisions:
- `Identifier` (4 queries) → BM25
- `NaturalLanguage`/`Mixed` (26 queries) → Dense+RRF k=20

## Verdict — REJECT hypothesis, KEEP code

The cycle-14 falsifiability criterion required: improve ≥ 1 metric
**without** regressing any other by > 0.01. The result improves MRR
(+0.021) but regresses R@5 (−0.025) and R@10 (−0.039) above the
threshold, with nDCG@5 at the floor (−0.010). **Strict-dominance
criterion is not met. Hypothesis REJECTED.**

The code is **KEPT** in the tree (precedent: cycle 13's
`retrieve_files_dense_rrf_with_rerank` was retained as documentation
of the OOM hardware constraint) because:

1. The function compiles clean, all gates pass.
2. The benchmark is a real measurement, not a synthetic test.
3. The docstring honestly documents the empirical trade-off.
4. Future engineers can use it as ready infrastructure for a
   smarter router (e.g., score-blend instead of binary dispatch).
5. Deleting it would erase the falsification evidence.

The cycle-12 **Dense+RRF k=20 path remains the recommended default**.

## Why Routing Loses Recall on Identifier Queries

For the 4 identifier queries that got routed to BM25:
- BM25 finds the exact-name file as top-1 → high MRR
- BM25 misses related files (callers, types, helpers) → low recall
- Dense+RRF finds the exact-name file at lower rank but *also* finds
  the semantic neighbors → moderate MRR but high recall

Score-blending (taking BM25's top-1 confidence + Dense+RRF's recall
from rank 2-10) would likely Pareto-dominate both. That is a
multi-cycle architectural change, out of scope for this single TDD
iteration.

## Validation (workspace untouched)

| Check | Result |
|---|---|
| `benchmark_retrieve_files_routed_guard` | PASS (MRR 0.695 ≥ 0.65 floor) |
| `cargo test -p theo-engine-retrieval --lib` | 292 / 292 passing, 5 ignored |
| `cargo build --workspace --exclude theo-code-desktop` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings |
| `make check-arch` | 0 violations / 16 crates |
| BM25 baseline path (`retrieve_files`) | unchanged |
| Cycle 12 dense+RRF path (`retrieve_files_dense_rrf`) | unchanged, 0.674 MRR |

## Files Changed

```
M crates/theo-engine-retrieval/src/file_retriever.rs
    + pub fn retrieve_files_routed (cfg-gated dense-retrieval)
    + docstring documents the empirical trade-off

M crates/theo-engine-retrieval/tests/benchmark_suite.rs
    + benchmark_retrieve_files_routed_guard (cfg-gated dense-retrieval)
    - removed unused `let k = 5;` warning at line 604
```

NOT touched: any other production code, allowlists, CLAUDE.md,
Makefile, gate scripts, ground-truth JSON, sota-thresholds.toml,
BM25 baseline path, Dense+RRF path, harm filter, ranker config.

## Updated dod-gate Status

| Gate | Floor | Best measurement | Source | Status |
|---|---|---|---|---|
| `retrieval.mrr` | 0.90 | **0.695** (routed) / 0.674 (dense+RRF) | cycle 14 / cycle 14 probe | BELOW_FLOOR (gap −0.205) |
| `retrieval.recall_at_5` | 0.92 | **0.507** (dense+RRF) | cycle 14 probe | BELOW_FLOOR (gap −0.413) |
| `retrieval.recall_at_10` | 0.95 | **0.577** (dense+RRF) | cycle 14 probe | BELOW_FLOOR (gap −0.373) |
| `retrieval.ndcg_at_5` | 0.85 | **0.495** (dense+RRF) | cycle 14 probe | BELOW_FLOOR (gap −0.355) |
| `retrieval.depcov` | 0.96 | 0.767 (BM25 baseline) | cycle 9-10 | BELOW_FLOOR (gap −0.193) |

The MRR gap **closed by another 0.021** (cycle 12: −0.226 → cycle 14:
−0.205) when routing is applied. R@5/R@10 are best served by Dense+RRF
alone, not routing. Per-metric "best path" is now:

- **MRR**: routed (0.695) — choose if precision-on-top is the goal
- **R@5, R@10, nDCG@5, depcov**: Dense+RRF k=20 — choose if recall is the goal

A blended router that takes the best of both is the natural next
investigation.

## Markers

<!-- FEATURES_STATUS:total=123,passing=37,failing=4 -->
<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_3_COMPLETE -->
<!-- PHASE_4_COMPLETE -->
