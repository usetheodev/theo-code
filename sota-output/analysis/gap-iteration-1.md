---
phase: 2
phase_name: analyze
iteration: 1
date: 2026-04-29
top_failure: retrieve_files_pipeline_mrr
severity: CRITICAL
---

# SOTA Validation — Phase 2 ANALYZE (Iteration 1)

## Worst-Performing DoD-Gate

| Probe | Floor | Measured | Gap |
|---|---|---|---|
| `benchmark_retrieve_files_mrr_guard` (in-tree benchmark) | 0.75 | **0.436** | **−31.4 pp (−42% rel.)** |
| `retrieval.recall_at_5` (thresholds.toml) | 0.92 | 0.76 | −16 pp |
| `retrieval.recall_at_10` (thresholds.toml) | 0.95 | 0.86 | −9 pp |

The in-tree guard is the most direct, reproducible signal: it actually
**fails the test suite right now** (`#[test] #[ignore]`, runnable via
`cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored`).
Run output:

```
retrieve_files pipeline MRR = 0.436  (harm_removals total: 50)
panicked at .../benchmark_suite.rs:1810: retrieve_files MRR 0.436
  regressed below the 0.75 functional floor
```

Test comment confirms the regression history (line 1747):

> "The baseline was 0.809 when the plan was drafted; we assert the
>  pipeline keeps MRR ≥ 0.75 to allow for the subtractive nature of
>  harm_filter ..."

**Drop attributable to harm_filter wiring: 0.809 → 0.436 = −0.373
absolute (−46 % relative).** This is the same family of regression that
the recall@5/recall@10 numbers reflect — same pipeline, same root cause.

## Pipeline Under Test

`crates/theo-engine-retrieval/src/file_retriever.rs::retrieve_files`:

```
Stage 2  FileBm25::search          (lexical baseline)
Stage 3  Community flatten          (graph-aware grouping)
Stage 4  Reranker
Stage 4.5 harm_filter   ← introduced regression
Stage 5  Graph expansion
```

Stages 2–4 + 5 were the pre-regression baseline (MRR 0.809). Stage 4.5
was added to remove "harmful" candidates (test/fixture/redundant files)
on the basis of CODEFILTER 2024 results. Stage 4.5 is the only new
component, so it is the proximate cause.

## Root-Cause Walkthrough — `crates/theo-engine-retrieval/src/harm_filter.rs`

### Bug #1 (LIKELY DOMINANT) — community lookup uses string length, not directory

`build_community_lookup`, lines 236–252:

```rust
let dir_hash = path
    .rfind('/')
    .map(|i| &path[..i])
    .unwrap_or("")
    .len();                             // ← .len() instead of the slice
lookup.insert(path.as_str(), dir_hash); // ← inserts the length
```

`dir_hash: usize` is the **byte length of the directory prefix**, not
the directory itself. Every two files whose parent paths happen to have
the same character count are placed in the same "community". Then
`is_redundant_in_community` sees them as duplicates and removes the
lower-ranked one (lines 258–281).

Concrete example, all length-19:
- `crates/theo-domain` (19 chars)
- `apps/theo-cli/src/x` (19 chars)
- `docs/plans/sota/x/y` (19 chars)

A high-scoring `theo-domain` file with score 1.0 makes a `theo-cli` file
with score 0.96 look "redundant" (`0.96 / 1.0 = 0.96 ≥ REDUNDANCY_SCORE_RATIO=0.95`)
and the `theo-cli` file is purged — even though they share nothing
beyond a coincident prefix length.

### Bug #2 (SECONDARY) — over-aggressive test-file removal

`has_definer_for_test`, lines 215–232:

```rust
let cleaned = test_stem
    .trim_end_matches("_test")        // …
    .trim_end_matches(".spec");
definer_files.iter().any(|definer| {
    let definer_stem = …;
    !cleaned.is_empty() && definer_stem.contains(cleaned)  // substring!
})
```

`definer_stem.contains(cleaned)` is a **substring** check. A test stem
of `mod` (after stripping suffixes) matches any definer with `mod` in
its filename — `module.rs`, `model.rs`, `mode.rs`, `mod.rs`, … — so the
test file gets purged whenever an unrelated definer is in the top-K.

### Bug #3 (POLICY) — removal cap is too generous

`MAX_REMOVAL_FRACTION = 0.40` (line 29). For top-10 retrieval the filter
may remove 4 items. Two of the top-5 disappearing is enough to drop
recall@5 by 0.2 alone — exactly the gap we see (0.92 → 0.76).

## Ranking by Priority × Impact

| Bug | Priority impact | Implementation impact | Score |
|---|---|---|---|
| #1 community lookup uses `.len()` | retrieval gate (DoD) | one-line fix | **A** |
| #2 substring test/definer match | retrieval gate (DoD) | small, but logic-deep | B |
| #3 0.40 cap too loose | retrieval gate (DoD) | one constant | C |

Bug #1 is the cleanest, most isolated, most testable, and has the
narrowest blast radius. It is the recommended target for Phase 3.

## Hypothesis for Phase 3 REFINE

**H1 (single fix):** Replacing the byte-length-of-directory key in
`build_community_lookup` with the directory string itself stops the
filter from collapsing unrelated directories into a single "community",
prevents the `is_redundant_in_community` false positives, and recovers a
material fraction of the lost MRR. We expect:

- `benchmark_retrieve_files_mrr_guard` → recover toward the 0.75 floor.
- `total_harm_removals` to drop substantially (less aggressive collapse).
- No regression on existing harm_filter unit tests
  (`harm_filter_removes_test_file_when_definer_present`, etc.) because
  those tests exercise Signals 1–3, not the community redundancy path.

**Falsifiable:** if MRR after the fix is still well below 0.75, the
dominant bug is elsewhere (probably Bug #2 or #3) and we discard.

## What is NOT Affected

- Architecture contract — fix is internal to one file.
- TDD posture — RED test already exists (`benchmark_retrieve_files_mrr_guard`
  is failing right now); GREEN is to make it pass.
- Other crates' tests — change is local to `theo-engine-retrieval`.
- Allowlists, CLAUDE.md, Makefile, gate scripts — untouched per protocol.

<!-- QUALITY_SCORE:0.85 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_2_COMPLETE -->
