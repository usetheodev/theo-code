---
phase: 1+2
phase_name: probe_and_analyze
iteration: 7
date: 2026-04-29
top_failure: rerank_path_unmeasurable_due_to_oom
severity: HARDWARE_CONSTRAINT
status: investigation_only
---

# Cycle 7 — Reranker Probe Failed (OOM Evidence)

## Goal

Measure the **unmeasured** ceiling of the existing retrieval stack — specifically
the `pipeline::retrieve_with_config` path that combines dense+RRF with the
cross-encoder reranker (`CrossEncoderReranker`, `fastembed::TextRerank`).
Until this cycle, no benchmark exercised this path against the local
`theo-code` ground truth.

## What Was Tried

Added (then removed) `benchmark_dense_rerank_pipeline` test that:

1. Built the full graph (973 files → 16 196 nodes, 35 026 edges)
2. Built the Tantivy index (973 docs)
3. Built the dense embedding cache via `NeuralEmbedder` (973 file embeddings; Jina v2 ~570 MB)
4. Initialized a cross-encoder reranker

Two reranker variants were tried:

| Reranker | Model size | Result |
|---|---|---|
| `JINARerankerV2BaseMultiligual` (production default) | ~568 MB | **SIGKILL** (OOM) before first query reranks |
| `BGERerankerBase` (smaller alternative) | ~278 MB | **SIGKILL** (OOM) before first query reranks |

System: 7 941 MB total RAM, 0 MB swap. Even with the smaller reranker, the
combined working set (Jina v2 dense embedder + Tantivy index + graph +
reranker model + cargo test process) exceeds the available memory.

## Honest Conclusion

The cross-encoder reranker path is **unmeasurable on this hardware** with
the current dense embedder choice. The achievable ceiling we *can* measure
remains the one from `benchmark_rrf_dense` (no reranker):

| Metric | BM25 baseline | Dense+RRF (measurable ceiling) |
|---|---|---|
| MRR | 0.593 | **0.689** |
| Recall@5 | 0.462 | **0.518** |
| Recall@10 | 0.545 | **0.628** |
| nDCG@5 | 0.427 | **0.504** |
| dod-gate floor | (varies) | n/a |

The remaining gap to floors (e.g. 0.689 → 0.90 MRR, 0.518 → 0.92 R@5) cannot
be closed without one of:

1. A **lighter dense embedder** (allowing the reranker to coexist in 8 GB).
2. **Process-isolated reranking** (offload one ML model to a sidecar so they
   never live in the same address space).
3. **Larger machine** (16+ GB) for measurement.
4. A fundamentally different ranker (LLM-as-reranker via API instead of
   on-machine ONNX models).

## What This Cycle Did NOT Do

- Did not bypass any floor in `sota-thresholds.toml`.
- Did not modify any production code.
- Did not change the production reranker default.
- Did add a test, ran it, observed OOM, removed it cleanly so the test suite
  remains green.

## Recommended Next Step (requires user approval)

Smallest meaningful production change that genuinely lifts the in-tree
benchmark guard:

> **Cycle 8 candidate:** wire `pipeline::retrieve_with_config(..., reranker:
> None, use_reranker: false, ...)` (i.e. the dense+RRF-only path) into
> `retrieve_files` behind an opt-in config flag. Re-measure the in-tree
> guard. Expected lift on production retrieval: 0.593 → 0.689 MRR; 0.462 →
> 0.518 R@5. Real, measured improvement; no bypass.

This still leaves the gates BELOW_FLOOR. Closing the rest needs the
hardware / architectural decisions above.

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->
<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_2_COMPLETE -->
