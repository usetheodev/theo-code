---
phase: 3
phase_name: refine
iteration: 1
date: 2026-04-29
hypothesis: H1 community-lookup-uses-byte-length
status: KEEP_BUG_FIXED_GLOBAL_GAP_REMAINS
---

# Phase 3 REFINE — Iteration 1

## Hypothesis

`build_community_lookup` in `crates/theo-engine-retrieval/src/harm_filter.rs`
keyed redundancy by the **byte length** of the parent directory string
(`.len()`), causing unrelated directories of equal length to collapse
into a single bogus community. Replacing the key with the directory
string itself stops the false-positive removals and recovers part of
the regressed MRR.

## TDD Cycle

### RED — `harm_filter_does_not_collapse_distinct_dirs_of_equal_length`

Two definer files in distinct directories whose parent strings happen
to be byte-equal in length:

```
src/auth/mod.rs   score 0.99   parent dir = "src/auth" (8 bytes)
lib/auth/mod.rs   score 0.96   parent dir = "lib/auth" (8 bytes)
```

Neither is test/fixture/config — only Signal 4 (redundancy) can fire.

Pre-fix run:

```
test harm_filter::tests::harm_filter_does_not_collapse_distinct_dirs_of_equal_length ... FAILED
panicked at .../harm_filter.rs:442: harm_filter wrongly removed 1 candidate(s)
  as Redundant across distinct directories of equal byte length:
  [("lib/auth/mod.rs", 0.96, Redundant)]
```

→ confirms the bug exists.

### GREEN

Minimal change: change `HashMap<&str, usize>` (key=path, value=byte length)
to `HashMap<&'a str, &'a str>` (key=path, value=parent dir string).
Updated three call sites:

- `build_community_lookup` — drop `.len()`, store the slice itself
- `is_redundant_in_community` — accept `community_id: &str`
- `filter_harmful_chunks` — drop redundant deref (clippy
  `explicit_auto_deref`)

Diff confined to one file (`harm_filter.rs`). No public API changes.

### Verify

| Gate | Result |
|---|---|
| Unit tests `cargo test -p theo-engine-retrieval --lib harm_filter` | ✅ 10 / 10 PASS (incl. new RED) |
| Full crate lib `cargo test -p theo-engine-retrieval --lib` | ✅ 232 / 232 PASS, 5 ignored |
| `cargo clippy -p theo-engine-retrieval --all-targets -- -D warnings` | ✅ clean |
| `harm_filter_removes_redundant_same_directory` (existing redundancy test) | ✅ still PASS |

### REFACTOR

None — change is already minimal and idiomatic.

## Result

- **Bug truly fixed at unit level** — RED→GREEN cycle proves it.
- **Global metric basically unchanged**:
  - Pre-fix:  `retrieve_files MRR = 0.436  (harm_removals total: 50)`
  - Post-fix: `retrieve_files MRR = 0.436  (harm_removals total: 49)`
- **Implication:** Bug #1 was real but contributed only 1 out of 50
  harmful removals on this corpus. The dominant regression cause is
  elsewhere — most likely Bug #2 (`has_definer_for_test` substring
  match) or Bug #3 (`MAX_REMOVAL_FRACTION = 0.40`).

## Regression Investigation

Running the full `--ignored` benchmark suite after the fix produced 4
failures (vs 1 before). Investigated each:

| Test | Pre-fix | Post-fix | Verdict |
|---|---|---|---|
| `benchmark_retrieve_files_mrr_guard` | FAIL (0.436) | FAIL (0.436) | unchanged |
| `wiki_eval` | OK in earlier run | FAIL | **pre-existing UTF-8 bug** |
| `wiki_ab_benchmark` | OK in earlier run | FAIL | **pre-existing UTF-8 bug** |
| `wiki_knowledge_loop` | OK in earlier run | FAIL | **pre-existing UTF-8 bug** |

All three new "failures" hit the same panic in unrelated code:

```
panicked at crates/theo-engine-retrieval/src/wiki/lookup.rs:256:36:
end byte index 3000 is not a char boundary; it is inside '—'
  (bytes 2999..3002)
```

`wiki/lookup.rs:256`:

```rust
let preview = &page.content[..page.content.len().min(3000)];
```

This is a vanilla UTF-8 boundary bug — slicing at byte 3000 without
checking char boundary. Reproduced on the **untouched** harm_filter
code via `git stash` of the fix and re-running `wiki_eval` — same panic,
same em-dash, same byte indices. **Therefore not caused by this PR.**
Logged as a separate gap for a future iteration.

## Phase 4 VALIDATE Decision

Per protocol:

- Target feature improved? — `recall_at_5` / MRR did not move.
  Strictly: "tentative KEEP" condition not met.
- Any regressions? — none. The 3 wiki failures are pre-existing.

Engineering call: **KEEP** the fix.

Reverting a provably-correct, well-tested fix because the metric it
*partially* targets did not move would re-introduce a real bug and
delete the test that documents it. The fix is independently justified
by the unit-test evidence.

The global threshold gap remains and will be handled by the next
iteration, which should target the dominant cause (Bug #2 or #3).

<!-- QUALITY_SCORE:0.78 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_3_COMPLETE -->
