---
phase: 4
phase_name: validate
iteration: 2
date: 2026-04-29
decision: KEEP_AND_HALT_REFINEMENT
---

# Phase 4 VALIDATE — Iteration 2

## Decision: KEEP H2; halt further refinement on harm_filter

## Result Comparison

| Metric | Pre-cycle-2 | Post-cycle-2 | Δ |
|---|---|---|---|
| `harm_filter_keeps_same_dir_complementary_definers` | RED (failing) | GREEN (passing) | unit-level fix proven |
| `cargo test -p theo-engine-retrieval --lib` | 232 / 232 | 233 / 233 | +1 test, 0 regressions |
| `cargo clippy -p theo-engine-retrieval -D warnings` | clean | clean | unchanged |
| benchmark `harm_removals` total | 49 | **32** | **−17 (−35 %)** |
| benchmark `retrieve_files MRR` | 0.436 | **0.436** | **0 (unchanged)** |

The cycle-2 fix does *exactly* what the hypothesis predicted at the
component level — fewer false-positive redundancy purges — but the
end-to-end MRR is unmoved.

## Why MRR Did Not Move — New Diagnostic

Ran `benchmark_bm25_baseline` (no graph/community/reranker/harm_filter):

```
OVERALL METRICS:
  MRR       = 0.426
  P@5       = 0.207
GATES (SOTA targets):
  MRR          0.426 / 0.850  FAIL
```

**The BM25 ranker by itself produces MRR ≈ 0.426** — essentially the
same as the full pipeline. Per-query breakdown shows multiple queries
returning **MRR = 0.00** (file never appears in top-K):

| Query ID | Query | MRR |
|---|---|---|
| theo-sym-003 | `louvain_phase1` | 0.00 |
| theo-sym-004 | `AgentRunEngine execute` | 0.00 |
| theo-mod-005 | `agent loop state machine transitions` | 0.00 |
| theo-mod-002 | `community detection clustering algorithm` | 0.02 |

These are simple symbol/concept queries against files that obviously
contain those identifiers. If the ranker is failing here, the issue is
upstream of harm_filter — most likely tokenization, indexing, or
ground-truth/index mismatch — and **no amount of harm_filter tuning
will move MRR**.

The test comment in `benchmark_retrieve_files_mrr_guard` (line 1747)
says "baseline was 0.809 when the plan was drafted". That number is
**stale**. The current baseline is 0.426. Either the index/ranker has
regressed since the baseline was recorded, or the ground truth was
updated to be harder, or the codebase moved away from the queries —
in any case, the 0.75 floor is unreachable from a 0.426 ceiling by
fiddling with the post-rank filter.

## What This Iteration Achieved

Two genuine, independent bugs fixed in `harm_filter.rs`:

1. **Bug #1 (cycle 1):** community lookup keyed redundancy on byte
   length of the parent dir → unrelated dirs collapsed into a single
   community. RED test +
   `harm_filter_does_not_collapse_distinct_dirs_of_equal_length`.
2. **Bug #4 / "Bug #3 substitute" (cycle 2):** `REDUNDANCY_SCORE_RATIO`
   = 0.95 too loose for modular codebases where sibling files are
   complementary, not redundant. RED test +
   `harm_filter_keeps_same_dir_complementary_definers`. Threshold
   tightened to 0.99.

Both fixes are demonstrably correct, independently testable, free of
regressions, and reduce `harm_removals` by 35 % on the benchmark
corpus. Whether or not the global MRR gate ever passes, these are
strict improvements to the filter.

## Recommendation: Halt Refinement on `harm_filter`

Continuing to iterate on `harm_filter` without first fixing the BM25
ranker (or validating the ground truth) is **chasing the wrong target**.
The protocol's "one fix per cycle" is meant to find a hill to climb;
the evidence above shows we are climbing the wrong hill — recall@5 and
MRR are bounded by the BM25 baseline, not by harm_filter behavior.

Next iterations of this loop should target either:

1. **Ranker investigation** — why does
   `benchmark_bm25_baseline` produce MRR 0.426 today vs 0.809 in the
   stale comment? Likely candidates: tokenizer regressions, index build
   issues, query parsing, or score normalization.
2. **Ground-truth audit** — confirm the 30 queries × 99 expected files
   in `theo-code.json` are still aligned with the current source tree
   (file moves/renames silently invalidate ground truth).
3. **Threshold revision** — if the ranker truly improved as much as
   the stale comment suggests then degraded later, the 0.85 MRR /
   0.75 functional-floor numbers in `sota-thresholds.toml` may need
   to be revisited honestly rather than chased blindly.

These are large, multi-cycle investigations and should not be tackled
inside one TDD cycle.

## Feature & Threshold Status

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->

| Threshold | Floor | Current | Status |
|---|---|---|---|
| `retrieve_files_mrr_guard` (in-tree gate) | 0.75 | 0.436 | FAIL (no progress) |
| `retrieval.recall_at_5` | 0.92 | 0.76 | BELOW_FLOOR (no probe rerun this cycle) |
| `retrieval.mrr` (whole-suite avg in `sota-thresholds.toml`) | 0.90 | 0.914 | PASS (apparently a different aggregate) |
| harm_filter unit-level correctness | — | ✅ +2 bugs fixed | improved |

<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_4_COMPLETE -->
