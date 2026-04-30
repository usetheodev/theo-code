---
phase: 4
phase_name: validate
iteration: 3
date: 2026-04-30
status: blend_grid_search_negative_result
decision: DISCARD changes to BlendScoreConfig::default()
---

# Cycle 9 — Validation: Blend Grid Search Result

## What Was Measured

`benchmark_blended_grid_search` ran 8 candidate weight tuples through
the blend pipeline (wiki BM25 + graph proximity + symbol overlap +
file BM25 — wiki_dense / embedder / file_memory all `None` for
hardware portability) on the cycle-3-corrected `theo-code` ground
truth (30 queries):

| Tuple | MRR | R@5 | R@10 | nDCG@5 |
|---|---|---|---|---|
| **BM25 baseline (reference)** | **0.593** | **0.462** | **0.545** | **0.427** |
| `default` (α=0.30, β=0.40, γ=0.10, δ=0.10, ε=0.05, ζ=0.03, η=0.02) | 0.422 | 0.292 | 0.314 | 0.280 |
| `heavy-alpha` (α=0.60, β=0.20, γ=0.10) | 0.478 | 0.308 | 0.338 | 0.307 |
| `heavy-graph` (γ=0.40, β=0.20, α=0.20) | 0.415 | 0.288 | 0.311 | 0.262 |
| `heavy-symbol` (η=0.35, α=0.30, β=0.20) | 0.458 | 0.377 | 0.417 | 0.328 |
| `uniform` (α=β=γ=0.20, η=0.30) | 0.421 | 0.316 | 0.356 | 0.277 |
| `alpha-only` (α=1.0) | **0.573** | 0.289 | 0.317 | 0.331 |
| `eta-only` (η=1.0) | 0.286 | 0.241 | 0.311 | 0.198 |
| `alpha-eta-balanced` (α=0.50, η=0.40, γ=0.10) | 0.488 | **0.404** | **0.439** | **0.361** |

## Decision: DISCARD blend-default change

**Every weight tuple regresses MRR and R@5 below the BM25 baseline.**
Best MRR (`alpha-only` 0.573) is still −0.020 below BM25 (0.593).
Best R@5 (`alpha-eta-balanced` 0.404) is −0.058 below BM25 (0.462).

The graceful-degradation fallback in
`retrieve_files_blended` — which routes to `retrieve_files` when
`wiki / wiki_dense / embedder / file_memory` are all `None` — is
**correctly defending production** against this regression, since the
default `RerankConfig::blend = None` and only callers that explicitly
opt in get the blend path.

## Why The Blend Underperforms (root cause analysis)

The wiki signal alone (BM25 over wiki page text) is not strong enough
to compensate for what the legacy reranker already does (joint signal
of BM25 + community + cochange + graph). Specifically:

1. **Wiki BM25 ≠ Dense.** The blend wiki path is BM25-only when no
   embedder is present. It captures the same lexical signal as
   `FileBm25` and adds noise rather than orthogonal information.
2. **Graph proximity needs good seeds.** The current seed set is
   (BM25 top 5 + wiki top 3). Without dense or memory hydration,
   these seeds carry the same biases as BM25 — proximity expands
   from those biases, doesn't correct them.
3. **Symbol overlap (η) is too sparse.** `eta-only` MRR=0.286 vs
   BM25 0.593 — symbol-name Jaccard is a sharp but low-recall signal,
   and pure symbol matches only fire on a fraction of queries.

## What This Validates

| Hypothesis | Evidence | Verdict |
|---|---|---|
| Linear blend over (BM25, wiki_BM25, graph, symbol) beats BM25 alone | All 8 tuples below 0.593 MRR | **REJECTED** |
| Graceful-degradation fallback prevents regression | Default `blend = None` keeps prod on legacy path | **CONFIRMED** |
| Closing the floors needs dense / rerank / API signal | Best blend without those signals = 0.573 MRR vs 0.85 floor | **CONFIRMED** |
| Lighter pipeline (no embedder OOM) can still close gates | Best blend MRR = 0.573 vs 0.85 floor | **REJECTED** |

## What Was NOT Modified

- `BlendScoreConfig::default()` — left at the original calibration
- `retrieve_files` and `retrieve_files_blended` — no code change
- `docs/sota-thresholds.toml` — no floor lowered
- Any allowlist / Makefile / CLAUDE.md / gate script — untouched

## Recommendation

The 4 BELOW_FLOOR retrieval gates remain the dominant SOTA gap. The
options are unchanged from cycle 8:

1. **Wire dense+RRF into `retrieve_files`** (Dense+RRF measured at
   0.689 MRR — closes ~30 % of the gap to 0.85 floor; no new memory
   pressure beyond the embedder which fits comfortably).
2. **Move benchmark to ≥ 16 GB hardware** to measure
   Dense+RRF+CrossEncoder.
3. **LLM-as-reranker via API** — measure cost-vs-quality.

This cycle adds no new options; it falsifies one (lightweight blend
without dense). The path forward needs an explicit user decision.

## Markers

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->
<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_4_COMPLETE -->
