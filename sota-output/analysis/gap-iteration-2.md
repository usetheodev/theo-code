---
phase: 2
phase_name: analyze
iteration: 2
date: 2026-04-29
top_failure: harm_filter_signal_4_redundancy_too_aggressive
severity: CRITICAL
---

# Phase 2 ANALYZE — Iteration 2

## Carry-over from Iteration 1

- Bug #1 (community key uses byte length) was real and is now fixed,
  but contributed only 1/50 ≈ 2 % of harmful removals.
- Integration MRR still 0.436 ≪ 0.75 floor.

## New Evidence — Ground-Truth Composition

Parsed `crates/theo-engine-retrieval/tests/benchmarks/ground_truth/theo-code.json`:

| Metric | Value | Implication |
|---|---|---|
| Total queries | 30 | |
| Queries with ≥ 2 expected files | 29 / 30 | almost every query expects multiple files |
| Total expected-file pairs | 136 | |
| Pairs whose expected files share a directory | **75 / 136 (55 %)** | dominant pattern |
| Queries where ≥ 1 expected pair is same-dir | **25 / 30 (83 %)** | nearly universal |
| Expected files matching `is_test_file()` | 0 / 99 (0 %) | **Signal 1 cannot fire on the answer set** |
| Expected files matching `is_fixture_file()` | 0 / 99 (0 %) | **Signal 2 cannot fire on the answer set** |
| Expected files matching `is_config_build_file()` | 0 / 99 (0 %) | **Signal 3 cannot fire on the answer set** |

**Ground-truth files are exclusively production source files in `crates/*/src/`.**
Therefore the *only* signal that can be removing answer files is **Signal 4
(Redundancy)**.

## Why Signal 4 Is Hurting

`is_redundant_in_community` removes a candidate when:

1. There is a higher-ranked candidate with score ratio ≥ 0.95.
2. Both share the same community.

After Iteration 1's fix, "same community" = "same parent directory string".

For 83 % of the queries in the ground truth, **two answer files live
in the same directory**. The ranker scores both highly (because they
both legitimately match the query); their ratio is naturally close to 1;
Signal 4 fires; one of them is purged. Recall drops, MRR drops.

The core issue: directory-based redundancy assumes that two files in the
same dir are *redundant* with each other. In well-modularized code
(theo-code, every modern Rust workspace, many Python packages), files
in the same dir are *complementary*, not redundant. The heuristic is
**inverted** for this kind of codebase.

## Hypothesis H2

Tightening `REDUNDANCY_SCORE_RATIO` from 0.95 to **0.99** restricts the
purge to genuine near-duplicates (score within 1 % of each other).
Complementary files that the ranker scores at 0.95–0.98 ratios — the
common case for sibling source files — survive.

Justification for 0.99:

- A 1 % score gap typically indicates clearly distinguishable matches.
- Score ratios above 0.99 are unusual and indicate the ranker truly
  cannot tell two files apart (the genuine "this is a near-duplicate"
  signal Signal 4 is meant to catch).
- The cap `MAX_REMOVAL_FRACTION = 0.40` plus Signals 1–3 still keep
  the filter active — `total_harm_removals > 0` will still hold.

## Falsifiability

After tightening:

| Outcome | Verdict |
|---|---|
| MRR ≥ 0.75 | KEEP (target floor reached) |
| MRR strictly > 0.436 (improved but not at floor) | KEEP (partial recovery, loop again on Bug #2) |
| MRR ≤ 0.436 | DISCARD; revert; revisit (the issue is elsewhere) |

## Hypothesis NOT Chosen This Cycle

- **Bug #2 (substring test/definer match):** ground-truth contains 0
  test paths, so `has_definer_for_test` cannot remove any answer file.
  Bug #2 is a real bug but not the dominant cause for this corpus.
- **Bug #3 (`MAX_REMOVAL_FRACTION = 0.40`):** average removals per
  query are ~1.6, well under the cap. Cap isn't binding. Real bug,
  not the dominant cause.

H2 wins on the *priority × evidence-driven impact* ranking.

<!-- QUALITY_SCORE:0.88 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_2_COMPLETE -->
