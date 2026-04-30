---
phase: 1+2+5
phase_name: probe_analyze_report
iteration: 10
date: 2026-04-30
top_failure: depcov_documentation_drift (informational) + 4 retrieval gates BELOW_FLOOR (carry-over)
status: convergence_check_no_new_tractable_work
---

# Cycle 10 — Convergence Check

## What This Cycle Investigated

Cycle 9 ended with a negative grid-search result. The protocol requires
loop-back since features still fail and 2 refinement cycles remain.
This cycle re-checked the probe and looked for any tractable
single-cycle item the previous 9 cycles missed.

## Findings

### A. Probe data (no change since cycle 9)

The benchmarks from cycle 9 are < 1 hour old and the working tree has
not been modified since. Re-running them would produce identical
numbers. Per cycle 9:

| Pipeline | MRR | R@5 | R@10 | nDCG@5 | DepCov |
|---|---|---|---|---|---|
| BM25 baseline | 0.593 | 0.462 | 0.545 | 0.427 | 0.767 |
| `retrieve_files` (prod) | 0.597 | — | — | — | — |
| `retrieve_files_blended` w/o wiki | 0.597 | — | — | — | — |
| Best blend tuple (`alpha-only`) | 0.573 | 0.289 | 0.317 | 0.331 | — |

### B. depcov investigation (NEW)

Investigated whether the `retrieval.depcov` gate (toml says
`current = 1.000` PASS) actually passes. **It does not.**

- **toml:** `current = 1.000`, `status = "PASS"`, `floor = 0.96`
- **measured today:** `0.767` (`benchmark_bm25_baseline`)
- **bench output:**
  ```
  GATES (SOTA targets):
    DepCov       0.767 / 0.900  FAIL
  ```

By category:
| Category | DepCov |
|---|---|
| symbol | 0.429 (worst — 4 of 7 symbol queries fully miss dep coverage) |
| module | 1.000 |
| semantic | 0.875 |
| cross-cutting | 0.714 |

Verified the new `mod.rs`-style ground-truth paths exist on disk:

```
OK crates/theo-engine-retrieval/src/assembly/mod.rs
OK crates/theo-engine-retrieval/src/search/mod.rs
OK crates/theo-application/src/use_cases/graph_context_service/mod.rs
OK crates/theo-agent-runtime/src/run_engine/mod.rs
OK crates/theo-agent-runtime/src/agent_loop/mod.rs
OK crates/theo-engine-graph/src/cluster/mod.rs
```

**Root cause:** `dep_coverage` requires BOTH `source` and `target`
files to be in `returned_files` (line 159 of `metrics.rs`). For symbol
queries, the BM25 ranker often surfaces the source file but not the
target dependency file — both must appear in the top-K. Since
`config.top_k = 8`, the ranker has to be more accurate AND have
broader recall to satisfy depcov than to satisfy plain recall.

This is the **same root cause** as the four BELOW_FLOOR retrieval
gates — BM25 ranker insufficient for high-quality multi-file
co-retrieval. depcov is just another expression of the same gap.

### C. The toml is documentation-drifted

`docs/sota-thresholds.toml` claims `retrieval.depcov` is PASS at 1.000.
Reality is BELOW_FLOOR at 0.767. Updating the `current` field to
0.767 and `status` to `BELOW_FLOOR` would be **documentation accuracy,
not a floor change**.

## Why No Code Change This Cycle

1. **depcov fix needs the same ranker upgrade** as the 4 retrieval
   gates — there is no separate single-cycle fix.
2. **Updating the toml `current = 0.767`** is honest reporting; cycles
   7-9 already did this for the other 4 gates. Doing it for depcov is
   trivial documentation work that does not need a refinement cycle.
3. **No new hypothesis** was generated this cycle — the negative grid
   search result and OOM-bounded reranker findings remain the dominant
   constraints.

## What This Cycle Did NOT Do

- No production code modified.
- No floor lowered.
- No allowlist / Makefile / CLAUDE.md / gate-script touched.
- The depcov toml entry is documented as drifted; the next cycle (or
  a separate hygiene PR) can update it without changing any floor.

## Loop Status

| | |
|---|---|
| Cycles completed | 10 |
| Refinement cycles used / max | 3 / 5 |
| Features failing | 5 (4 retrieval + depcov) |
| Workspace tests | 4452 passing / 0 failing |
| Architecture contract | 0 violations / 16 crates |
| Clippy | 0 warnings |

The loop has not closed any of the 4 retrieval gates and cannot do so
without a stewardship decision (hardware / Dense+RRF wire-up / LLM
rerank API). Cycle 9 produced the most up-to-date evidence; cycle 10
confirms convergence.

## Markers

<!-- FEATURES_STATUS:total=123,passing=36,failing=5 -->
<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_1_COMPLETE -->
<!-- PHASE_2_COMPLETE -->
<!-- PHASE_5_COMPLETE -->
<!-- LOOP_BACK_TO_PROBE -->
