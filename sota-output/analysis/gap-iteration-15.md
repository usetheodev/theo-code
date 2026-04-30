---
phase: 2
phase_name: analyze
iteration: 15
date: 2026-04-30
top_failure: cycle-14 Pareto trade-off (BM25 wins MRR, Dense+RRF wins recall) — needs score-blend not binary dispatch
severity: HIGH
---

# Cycle 15 — Gap Analysis: Score-Blend Router

## Cycle 14 Established the Trade-Off

The cycle-14 routed retriever proved that the two strongest paths
have **complementary strengths** rather than one dominating the
other:

| Path | Best at | Worst at |
|---|---|---|
| BM25 (`retrieve_files`) | top-1 precision on Identifier queries (exact-name match) | recall on identifier queries (misses callers/types) |
| Dense+RRF k=20 (`retrieve_files_dense_rrf`) | recall everywhere (semantic neighbors) | top-1 precision on Identifier queries (rank exact match lower) |

The cycle-14 routed function picked **one** path per query (binary
dispatch on QueryType). The result was a Pareto trade-off:
+0.021 MRR, but −0.025 R@5 and −0.039 R@10 vs Dense+RRF k=20.

The fix is structural: **don't pick one path, fuse both**. RRF
(Reciprocal Rank Fusion) over the two ranked lists composes:

- BM25's top-1 ranks (precision contribution)
- Dense+RRF's broader rank coverage (recall contribution)

## Why RRF Is The Right Fusion

Cycles 11-12 already use RRF inside `hybrid_rrf_search` to fuse
3 single-modality rankers (BM25, Tantivy, Dense embeddings). The
**meta-fusion** of `retrieve_files` + `retrieve_files_dense_rrf` is
mathematically identical: each ranked list contributes
`1 / (k + rank_i)` to the final score per file. The textbook k=60
or the cycle-12 k=20 are both well-defined choices.

RRF properties relevant to this hypothesis:

1. **Rank-only fusion** — robust to score scale differences between
   BM25 (TF-IDF magnitudes) and Dense+RRF (already RRF-normalized).
2. **Top-1 dominance** — a file at rank 1 in either source dominates
   files at lower ranks in both (since `1/21` >> `1/(21+10)+1/(21+10)`).
3. **Recall preservation** — files appearing in only one source
   still contribute their `1/(k+rank)` and are ranked.

This means: a file BM25 finds at rank 1 (identifier exact-match)
will be ranked 1st even if Dense+RRF puts it at rank 5. And a file
Dense+RRF finds at rank 3 (semantic neighbor) that BM25 ranks 100th
will still surface in the top-K via Dense+RRF's contribution.

## Hypothesis (Cycle 15)

**Build `retrieve_files_blended_rrf`**: at query time, compute both
`retrieve_files` and `retrieve_files_dense_rrf` candidate lists,
fuse via RRF (k=20 matching cycle-12's empirical optimum), apply
ghost-path filter + harm filter + graph expansion identically to
the existing entry points.

```rust
#[cfg(feature = "dense-retrieval")]
pub fn retrieve_files_blended_rrf(/* same args as routed */) -> FileRetrievalResult {
    // 1. Compute both rankings.
    let bm25_result = retrieve_files(graph, communities, query, config, previously_seen);
    let dense_result = retrieve_files_dense_rrf(/* args */);

    // 2. Extract ranked lists (path, rank).
    // 3. RRF-fuse with k=20.
    // 4. Top-K with ghost filter, harm filter, graph expansion.
}
```

### Falsifiability

The blended metric must:
- improve at least ONE of {MRR, R@5, R@10, nDCG@5} vs the best
  of {BM25, Dense+RRF k=20, routed} on that metric;
- NOT regress any of the four below the second-best path on that
  metric by more than 0.01 absolute.

Per cycle 14:
- MRR best: 0.695 (routed); second-best: 0.674 (Dense+RRF)
- R@5 best: 0.507 (Dense+RRF); second-best: 0.482 (routed)
- R@10 best: 0.577 (Dense+RRF); second-best: 0.545 (BM25)
- nDCG@5 best: 0.495 (Dense+RRF); second-best: 0.485 (routed)

So blended MUST be:
- MRR ≥ 0.674 (Dense+RRF — must not be worse than NOT routing)
- R@5 ≥ 0.472 (second-best 0.482 minus 0.01)
- R@10 ≥ 0.535 (second-best 0.545 minus 0.01)
- nDCG@5 ≥ 0.475 (second-best 0.485 minus 0.01)
- AND must improve at least one above its current best

### Hardware-feasibility

Composes existing measured-green paths only. No new model. Memory
footprint is identical to running the Dense+RRF benchmark (since
BM25 path uses negligible additional memory once the graph is
loaded). 8 GB envelope is preserved (cycle 11-12 + cycle 14
re-probe evidence).

### Why this is genuinely untried

- Cycles 1-3: bug fixes and ground-truth refresh.
- Cycles 4-6: query-type infrastructure (no fusion).
- Cycle 7-8: cross-encoder reranker (OOM blocker).
- Cycle 9: blend grid search at the **single-ranker** signal level
  (BM25 vs wiki vs graph vs symbol), NOT at the meta-ranker level.
  All weight tuples regressed because the underlying signals don't
  add information that BM25 lacks. The cycle-15 hypothesis fuses
  **two complete pipelines** (BM25 + Dense+RRF), not raw signals.
- Cycles 11-12: Dense+RRF k=20 as standalone path.
- Cycle 13: cross-encoder reranker (OOM blocker again).
- Cycle 14: binary dispatch (Pareto trade-off documented).

Score-blend at the meta-ranker level over BM25 + Dense+RRF is the
direct, evidence-motivated next step from cycle 14's finding. The
function `retrieve_files_blended` exists in the file but does
**linear weighted blending of single signals**, not RRF over two
complete pipelines (verified: `grep -n 'blend' file_retriever.rs`
shows `BlendScoreConfig` weights individual signals).

## Why This Cycle Should Be Attempted

The cycle-13 OOM evidence and cycle-14 trade-off finding, taken
together, constrain the hypothesis space:

- Heavy models (cross-encoder reranker) → blocked by hardware.
- Single-ranker swaps → no improvement (cycle 9, cycle 14).
- Binary dispatch → trade-off, not strict win (cycle 14).
- Score-blend (RRF over both pipelines) → **untried**, fits in
  the same hardware envelope, mathematically should preserve both
  strengths.

If the hypothesis succeeds → MRR + recall both improve.
If it fails → another path empirically falsified, hypothesis space
narrows further toward "8 GB cannot close these gates without an
external API or hardware upgrade".

Either outcome advances the loop's understanding.

## Markers

<!-- FEATURES_STATUS:total=123,passing=37,failing=4 -->
<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_2_COMPLETE -->
