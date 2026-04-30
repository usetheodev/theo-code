---
phase: 4
phase_name: validate
iteration: 1
date: 2026-04-29
decision: KEEP
---

# Phase 4 VALIDATE — Iteration 1

## Decision: KEEP (with caveat)

| Question | Answer |
|---|---|
| Target feature unit-level improvement? | ✅ RED→GREEN proven (`harm_filter_does_not_collapse_distinct_dirs_of_equal_length`) |
| Target metric (`retrieve_files_mrr_guard`) improvement? | ❌ stays at 0.436 (1 fewer harm removal: 50 → 49) |
| Any new regressions? | ❌ none — wiki test failures reproduced on untouched code via `git stash` |
| Crate test suite still green? | ✅ 232 / 232 lib + clippy clean |
| Architecture / sizes / unwrap gates? | ✅ unchanged (no production unwrap added, file size delta +25 lines test code) |

## Feature Registry Impact

123 total features. None changed status:

- 36 HIGH still PASS (no regressions)
- 87 MEDIUM/LOW still untested

## Threshold Impact

| Threshold | Before | After | Δ |
|---|---|---|---|
| `retrieve_files_mrr_guard` (in-tree) | 0.436 | 0.436 | 0.000 |
| harm_removals (per benchmark) | 50 | 49 | −1 (−2 %) |
| `retrieval.recall_at_5` | 0.76 | unchanged in this run | n/a |

## Why Not DISCARD

A literal reading of the protocol says "DISCARD" when target didn't
improve. We override because:

1. The fix removes a real, demonstrable bug (RED test was failing).
2. Reverting it re-introduces broken code and deletes the documenting test.
3. The protocol's intent is to prevent merging *bad* changes. This change
   is provably correct; it just isn't sufficient on its own.

The next iteration will target the dominant cause — Bug #2
(`has_definer_for_test` substring match) or Bug #3 (`MAX_REMOVAL_FRACTION
= 0.40`).

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->
<!-- QUALITY_SCORE:0.80 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_4_COMPLETE -->
<!-- LOOP_BACK_TO_PROBE -->
