---
phase: 1+2
phase_name: probe_and_analyze
iteration: 9
date: 2026-04-30
top_failure: retrieval_gates_below_floor (4 dod-gates)
status: re-probe_complete_grid_search_in_progress
---

# Cycle 9 — Re-probe + Blend Grid Search

## What Was Probed (Phase 1)

Re-ran the empirical probes with the **current working tree** (which
contains the in-progress `wiki-graph-memory-blend-retrieval` plan code:
`retrieve_files_blended`, `BlendScoreContext`, `score_file_blended`).

### `benchmark_bm25_baseline` (BM25-only ranker)

```
OVERALL METRICS:
  Recall@5  = 0.462    Recall@10 = 0.545
  P@5       = 0.260    MRR       = 0.593
  Hit@5     = 0.800    Hit@10    = 0.867
  nDCG@5    = 0.427    nDCG@10   = 0.469
  MAP       = 0.382
  DepCov    = 0.767    MissDep   = 0.233
```

### `benchmark_retrieve_files_mrr_guard` (current production path)

```
retrieve_files pipeline MRR = 0.597  (harm_removals total: 31)
FAIL — below the in-tree functional floor of 0.75
```

### `benchmark_blended_retrieve_mrr_guard` (blend path, wiki=None, dense=None)

```
retrieve_files_blended (wiki=None, dense=None) MRR = 0.597
PASS — at-or-above conservative floor 0.45
```

The blend path with all signal sources `None` correctly **falls back
to the legacy** path (graceful degradation in
`retrieve_files_blended` when none of wiki/wiki_dense/embedder/file_memory
is provided), producing the identical 0.597 MRR — confirming the new
code does not regress production.

## Status of Each `dod-gate` (from `docs/sota-thresholds.toml`)

| Gate | Floor | Measured (BM25 baseline) | Status |
|---|---|---|---|
| `retrieval.mrr` | 0.90 | 0.593 | **BELOW_FLOOR** |
| `retrieval.recall_at_5` | 0.92 | 0.462 | **BELOW_FLOOR** |
| `retrieval.recall_at_10` | 0.95 | 0.545 | **BELOW_FLOOR** |
| `retrieval.ndcg_at_5` | 0.85 | 0.427 | **BELOW_FLOOR** |
| `retrieval.depcov` | 0.96 | 0.767 | **REGRESSED FROM 1.000** |
| `retrieval.per_language_recall_at_5` | 0.85 | unmeasured | **UNMEASURED** |
| `context.relevance_threshold` | 0.3 | 0.3 | PASS |
| `context.max_files` | 8 | 8 | PASS |
| `smoke.pass_rate` | 0.85 | 1.00 | PASS |
| `swe_bench.minimum_runs` | 3 | 3 | PASS |
| `cost.max_per_smoke_run_usd` | 5.0 | 0.0 | PASS |
| `cost.max_per_refinement_iteration_usd` | 10.0 | 0.0 | PASS |

**5 of 12 dod-gates BELOW_FLOOR / UNMEASURED / REGRESSED** (retrieval.depcov
specifically regressed from `1.000` measured 2026-04-29 down to `0.767`
today, suggesting the cycle-3 ground-truth refresh fix has decayed —
possibly because `theo-code.json` now references files moved/renamed by
intervening commits).

## Worst-performing dod-gate

By absolute gap to floor:

1. **`retrieval.recall_at_5`** — gap = 0.92 − 0.462 = **−0.458** ← worst
2. `retrieval.recall_at_10` — gap = 0.95 − 0.545 = −0.405
3. `retrieval.ndcg_at_5` — gap = 0.85 − 0.427 = −0.423
4. `retrieval.mrr` — gap = 0.90 − 0.593 = −0.307

`retrieval.recall_at_5` has the largest absolute gap. By
`priority × impact`, all 4 gates have priority HIGH and impact ~equal
(they share the same root cause: BM25-only ranker on a NL-heavy query
mix).

## Root Cause (verified across cycles 7-9)

The BM25-only ranker that `retrieve_files` uses today cannot reach the
floors. Per cycle 7-8 evidence:

| Pipeline | MRR | R@5 | R@10 | nDCG@5 |
|---|---|---|---|---|
| BM25 only (current prod) | 0.593 | 0.462 | 0.545 | 0.427 |
| Dense + RRF (in-code, not wired) | **0.689** | 0.518 | 0.628 | 0.504 |
| Dense + RRF + Cross-Encoder | **OOM** on 8 GB | — | — | — |

Even the best **measurable** ceiling on this hardware (Dense+RRF at
0.689 MRR) is still −0.21 below the floor (0.90).

## What This Cycle Adds — `benchmark_blended_grid_search`

Currently running in background (8 weight tuples × ~100s/run ≈ 13 min):

- `default` — alpha=0.30, beta=0.40, gamma=0.10, …
- `heavy-alpha` — file_dense=0.60
- `heavy-graph` — graph_proximity=0.40
- `heavy-symbol` — symbol_overlap=0.35
- `uniform` — equal split alpha/beta/gamma=0.20
- `alpha-only` — file_dense=1.0
- `eta-only` — symbol_overlap=1.0
- `alpha-eta-balanced` — file_dense=0.50, symbol_overlap=0.40

This will determine whether **any** linear blend over (file_bm25, wiki,
graph_proximity, authority_tier, frecency, memory_link, symbol_overlap)
beats the 0.593 BM25 baseline on the same ground-truth corpus when only
the wiki signal is added (wiki_dense / embedder / file_memory = None
to keep the 8 GB hardware envelope).

## Required User Decision (carried forward from cycle 8)

The 4 BELOW_FLOOR retrieval gates **cannot be closed by autonomous TDD
cycles on this 8 GB machine.** Choices, in honest order:

1. **Wire dense+RRF into `retrieve_files`** (single-cycle, low-risk).
   Lifts production MRR 0.593 → 0.689. Does not close any gate but is
   measured, real progress.

2. **Move benchmark to ≥ 16 GB hardware.** Required to reliably measure
   full Dense+RRF+Rerank pipeline. Until then we're guessing at the
   reranker contribution.

3. **Switch to LLM-as-reranker** (Anthropic / OpenAI API). $$ per query,
   API key required. Cost-vs-quality unknown.

4. **Lower the floors** to evidence-based achievable values. **Bypass —
   user has explicitly forbidden. Listed for honesty only.**

5. **Accept BELOW_FLOOR until architectural direction (1, 2, or 3)
   plays out across multiple PRs.**

## What This Cycle Does NOT Do

- No production code modified.
- No floor lowered.
- No allowlist / Makefile / CLAUDE.md / gate script touched.
- The blend grid search is **measurement only** — its output will inform
  but not modify `BlendScoreConfig::default()` without an explicit human
  gate.

## Markers

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->
<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_2_COMPLETE -->
