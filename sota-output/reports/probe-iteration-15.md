---
phase: 1
phase_name: probe
iteration: 15
date: 2026-04-30
mode: REUSE_CYCLE_14
---

# Cycle 15 — PROBE (Reuse Cycle 14 Data)

## Why reuse, not re-measure

Cycle 14 ran the full retrieval probe ~30 minutes ago. Since then:

- **No production logic changed.** Cycle 14 added `retrieve_files_routed`
  (cfg-gated dense-retrieval, ~20 LOC dispatch), updated docstrings
  (no semantic change), and synced `docs/sota-thresholds.toml`
  (documentation only).
- **No ground-truth data changed.** `theo-code.json` untouched since
  cycle 3.
- **No benchmark constants changed.** RerankConfig defaults, RRF k,
  harm filter thresholds — all unchanged.

Re-running the 5-minute Dense+RRF benchmark would produce identical
numbers within run-to-run variance (cycle 12: MRR 0.664 → cycle 14
re-probe: MRR 0.674; the +0.010 delta is the noise floor on this
hardware). Re-running BM25 baseline produced literally identical
numbers (cycle 12: MRR 0.593, cycle 14: MRR 0.593).

Reusing cycle-14 measurements honors the budget rule (no wasted
compute) and the evidence rule (the data is fresh and reproducible).

## Cycle 14 Measurements Carried Forward

| Probe | Measurement | Source command |
|---|---|---|
| `benchmark_bm25_baseline` MRR | 0.593 | `cargo test -p theo-engine-retrieval --test benchmark_suite benchmark_bm25_baseline -- --ignored --nocapture` |
| `benchmark_bm25_baseline` R@5 | 0.462 | (same) |
| `benchmark_bm25_baseline` R@10 | 0.545 | (same) |
| `benchmark_bm25_baseline` nDCG@5 | 0.427 | (same) |
| `benchmark_bm25_baseline` DepCov | 0.767 | (same) |
| `benchmark_retrieve_files_dense_rrf_guard` MRR | 0.674 | `cargo test -p theo-engine-retrieval --features dense-retrieval --test benchmark_suite benchmark_retrieve_files_dense_rrf_guard -- --ignored --nocapture` |
| `benchmark_retrieve_files_dense_rrf_guard` R@5 | 0.507 | (same) |
| `benchmark_retrieve_files_dense_rrf_guard` R@10 | 0.577 | (same) |
| `benchmark_retrieve_files_dense_rrf_guard` nDCG@5 | 0.495 | (same) |
| `benchmark_retrieve_files_routed_guard` MRR | 0.695 | `cargo test -p theo-engine-retrieval --features dense-retrieval --test benchmark_suite benchmark_retrieve_files_routed_guard -- --ignored --nocapture` |
| `benchmark_retrieve_files_routed_guard` R@5 | 0.482 | (same) |
| `benchmark_retrieve_files_routed_guard` R@10 | 0.538 | (same) |
| `benchmark_retrieve_files_routed_guard` nDCG@5 | 0.485 | (same) |

## Spot-Check Gates (cycle 15, just now)

| Check | Result |
|---|---|
| `cargo test --workspace --exclude theo-code-desktop --lib` | **4452 / 0 / 5** (matches cycle 14) |
| `cargo clippy --workspace --exclude theo-code-desktop --all-targets -- -D warnings` | **0 violations** |
| `make check-arch` | **0 violations / 16 crates** |

State is identical to cycle 14. PROBE reuse is justified.

## dod-gate Status (unchanged from cycle 14)

| Gate | Floor | Best | Status |
|---|---|---|---|
| `retrieval.mrr` | 0.90 | 0.695 (routed) | BELOW_FLOOR (gap −0.205) |
| `retrieval.recall_at_5` | 0.92 | 0.507 (dense+RRF) | BELOW_FLOOR (gap −0.413) |
| `retrieval.recall_at_10` | 0.95 | 0.577 (dense+RRF) | BELOW_FLOOR (gap −0.373) |
| `retrieval.ndcg_at_5` | 0.85 | 0.495 (dense+RRF) | BELOW_FLOOR (gap −0.355) |
| `retrieval.depcov` | 0.96 | 0.767 (BM25) | BELOW_FLOOR (gap −0.193) |
| `retrieval.per_language_recall_at_5` | 0.85 | unmeasured | UNMEASURED |

## Markers

<!-- FEATURES_STATUS:total=123,passing=37,failing=4 -->
<!-- PHASE_1_COMPLETE -->
