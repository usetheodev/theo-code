---
phase: 1+2
phase_name: probe_and_analyze
iteration: 8
date: 2026-04-29
top_failure: hardware_bounded_retrieval_ceiling
status: investigation_complete_user_decision_required
---

# Cycle 8 — Reranker Probe Hardware Constraint Confirmed

## What Was Attempted

Three runs of `benchmark_bm25_rerank_lite` (BM25 → BGE-Base cross-encoder
rerank, dense embedder skipped to free ~570 MB):

| Run ID | Result |
|---|---|
| `bvx79s7qv` | Completed (1338 s = 22 min, exit 0). Aggregates lost to operator-side `tail -25` truncation. |
| `buntuzm4r` | Hung > 1 h, killed. |
| `b2sdobgzy` | Cargo wrapper killed during run; underlying binary continued and was killed cleanly. No metrics file produced. |

## Honest Findings

1. **The rerank path can technically run on this hardware** — the 22-min
   completed run proves it.
2. **Reliability is poor** — two of three runs failed (one hang, one
   indeterminate exit). Memory pressure (8 GB total, no swap) puts the
   workload right at the OOM edge.
3. **Aggregates not yet captured** — operator error (the tail filter)
   on the only successful run wiped the OVERALL/GATES section from
   our reach.

## What Is Verified Beyond Doubt

| Pipeline | MRR | R@5 | R@10 | nDCG@5 | Hardware fit |
|---|---|---|---|---|---|
| BM25 only (currently in `retrieve_files`) | 0.593 | 0.462 | 0.545 | 0.427 | ✅ |
| Dense + RRF (in code, not wired into prod) | 0.689 | 0.518 | 0.628 | 0.504 | ✅ |
| Dense + RRF + Rerank (full prod pipeline) | n/a | n/a | n/a | n/a | ❌ OOM (cycle 7) |
| BM25 + Rerank (lighter, dense skipped) | unknown | unknown | unknown | unknown | flaky |

## Floors vs Verified Ceiling

| dod-gate | Floor | Verified ceiling (Dense+RRF) | Gap |
|---|---|---|---|
| `retrieval.mrr` | 0.90 | 0.689 | **−0.211** |
| `retrieval.recall_at_5` | 0.92 | 0.518 | **−0.402** |
| `retrieval.recall_at_10` | 0.95 | 0.628 | **−0.322** |
| `retrieval.ndcg_at_5` | 0.85 | 0.504 | **−0.346** |

Even at the **best measured pipeline** (which uses every retrieval
component already shipped except the reranker), the gap to the floors
is enormous — −0.21 to −0.40 absolute on every metric. The reranker, if
measurable, *might* close 5–15pp more (typical SOTA cross-encoder lift
in published benchmarks), bringing MRR to ~0.79 — still ~0.10 below the
0.90 floor.

## What This Cycle Did NOT Do

- Did not lower any floor.
- Did not modify any production code.
- Did not modify any allowlist / Makefile / CLAUDE.md / gate script.
- Did add (and kept) the test `benchmark_bm25_rerank_lite` for future
  re-measurement on a machine with adequate memory.

## Required User Decision

The 4 BELOW_FLOOR retrieval gates **cannot be closed by autonomous
TDD cycles on this 8 GB machine.** Choose one of:

1. **Wire dense+RRF into `retrieve_files`** (single TDD cycle I can do
   with approval). Lifts production MRR 0.593 → 0.689. Does NOT close
   any gate but is genuine improvement and lays groundwork for routing.
   *Cost:* ~30 min coding + verification. *Risk:* low (RRF path is
   already tested in benchmark_rrf_dense).

2. **Move benchmark execution to a 16 GB+ machine.** Required to
   reliably measure full Dense+RRF+Rerank pipeline. Until then we're
   guessing at the rerank lift. *Cost:* infra setup. *Outcome:* unknown
   but likely closes another 0.05-0.10 on every metric, still short of
   floors.

3. **Switch to LLM-as-reranker.** Replace the on-machine cross-encoder
   with API calls (Anthropic / OpenAI). *Cost:* per-query $$, requires
   API key, code change. *Outcome:* potentially much higher quality but
   we don't know without measuring.

4. **Lower the floors to evidence-based achievable values.** This is
   the *bypass* path the user has explicitly forbidden — listed only
   for completeness so the option-space is honest, NOT a recommendation.

5. **Accept that the 4 retrieval dod-gates will stay BELOW_FLOOR**
   until the chosen architectural direction (1, 2, or 3) plays out
   over multiple PRs.

I cannot autonomously pick between (1)/(2)/(3). They are stewardship
decisions about cost, infra, and direction.

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->
<!-- QUALITY_SCORE:0.80 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_2_COMPLETE -->
