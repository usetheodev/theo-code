---
phase: 2+3+4
phase_name: combined_cycle_3
iteration: 3
date: 2026-04-29
top_failure: stale_ground_truth
severity: CRITICAL
---

# Cycle 3 — Stale Ground Truth (Combined ANALYZE + REFINE + VALIDATE)

## Why this cycle is different

Iteration 2 surfaced that the BM25 baseline alone was MRR 0.426 — i.e.,
the ranker was failing on multiple "easy" symbol queries with MRR 0.00.
Investigating those queries revealed the actual cause is upstream of the
retrieval engine entirely: the **ground truth itself is stale**.

## Evidence

```
$ python -c '... count missing expected_files ...'
Total unique expected files: 63
Present in tree: 53
Missing: 25 references (10 unique paths)
```

10 unique paths from `crates/theo-engine-retrieval/tests/benchmarks/ground_truth/theo-code.json`
no longer exist:

| Old path | What happened |
|---|---|
| `…/theo-engine-retrieval/src/assembly.rs` | refactored into `assembly/{mod,greedy,…}.rs` |
| `…/theo-engine-retrieval/src/search.rs` | refactored into `search/{mod,file_bm25,…}.rs` |
| `…/theo-engine-retrieval/src/tantivy_search.rs` | refactored into `tantivy_search/` |
| `…/theo-engine-graph/src/cluster.rs` | refactored into `cluster/{mod,louvain}.rs` |
| `…/theo-agent-runtime/src/run_engine.rs` | refactored into `run_engine/{mod,lifecycle,delegate_handler,builders}.rs` |
| `…/theo-agent-runtime/src/agent_loop.rs` | refactored into `agent_loop/` |
| `…/theo-agent-runtime/src/compaction.rs` | refactored into `compaction/` |
| `…/theo-agent-runtime/src/pilot.rs` | refactored into `pilot/` |
| `…/theo-agent-runtime/src/state.rs` | renamed to `state_manager.rs` |
| `…/theo-application/src/use_cases/graph_context_service.rs` | refactored into `graph_context_service/` |

These 10 paths are referenced by **25 expected_files entries** spanning
the symbol, module, and cross-cutting query categories. Their queries
were scoring MRR 0.00 because the answer file no longer exists at the
expected path — the ranker could be perfect and would still miss.

This is exactly what the Phase 5 report (cycle 2) listed as priority
investigation #2: "Ground-truth audit — confirm the 30 queries × 99
expected files in `theo-code.json` are still aligned with the current
source tree (file moves/renames silently invalidate ground truth)."

## Hypothesis

Updating each stale path to its current location (typically `X.rs` →
`X/mod.rs` after Rust submodule extraction; `state.rs` → `state_manager.rs`)
will substantially improve `recall_at_5` and `MRR` because the ranker
will once again be measured against attainable targets.

## Fix Applied

`crates/theo-engine-retrieval/tests/benchmarks/ground_truth/theo-code.json`:
25 expected_files references rewritten via the mapping above. Dependency
edges (`source` / `target`) referring to those same paths were also
updated. JSON is fixture data, not gate / allowlist / Makefile / source
code.

The mapping was derived programmatically:

- if `<path>.rs` is missing but directory `<path>/` exists → use
  `<path>/mod.rs`;
- otherwise look for a file in the same directory whose stem starts with
  the missing stem (caught the `state.rs` → `state_manager.rs` case).

10 / 10 missing paths mapped, 0 unmapped, 25 / 25 references updated.

## Validate (Same Cycle)

Reran both retrieval benchmarks immediately:

| Metric | Pre-fix | Post-fix | Δ |
|---|---|---|---|
| `benchmark_bm25_baseline` MRR | 0.426 | **0.593** | **+0.167 (+39 %)** |
| `benchmark_bm25_baseline` P@5 | 0.207 | **0.260** | +0.053 |
| `benchmark_bm25_baseline` symbol-MRR | 0.443 | **0.798** | **+0.355 (+80 %)** |
| `benchmark_bm25_baseline` module-MRR | 0.487 | 0.669 | +0.182 |
| `benchmark_retrieve_files_mrr_guard` MRR | 0.436 | **0.597** | **+0.161 (+37 %)** |
| Lib tests | 233 / 233 | 233 / 233 | unchanged |
| Clippy `-D warnings` | clean | clean | unchanged |

Symbol queries are now near 0.80 MRR — appropriate for an exact-name
lookup. The remaining shortfall is concentrated in cross-cutting and
semantic NL queries (e.g. "community detection clustering algorithm",
"BM25 scoring tokenization") — those need semantic search /
embeddings, not lexical fixes, and are out of scope for a single TDD
cycle.

## Decision: KEEP

| Question | Answer |
|---|---|
| Target metric improved? | ✅ +37 % on the in-tree guard, +39 % on BM25 baseline |
| Any regressions? | ❌ none — lib tests still 233 / 233, clippy clean |
| Forbidden paths touched? | ❌ no — fixture JSON is not gate / allowlist / Makefile |
| Fix justifiable independently? | ✅ ground-truth pointing at non-existent files is wrong regardless of metric |

KEEP unconditionally.

## Remaining Gap

`benchmark_retrieve_files_mrr_guard` still fails: 0.597 vs 0.75 floor.
Per-category breakdown of the remaining gap on the BM25 baseline:

| Category | MRR | Status |
|---|---|---|
| symbol | 0.798 | acceptable for lexical |
| module | 0.669 | mid |
| semantic | 0.546 | lexical limit |
| cross-cutting | 0.357 | lexical fundamentally insufficient |

Cross-cutting and semantic queries describe code in natural language
("error types defined across crates", "agent loop state machine
transitions"). Lexical BM25 cannot match these reliably; recovering
the rest of the gap needs the dense / embedding stage of the pipeline
(`crates/theo-engine-retrieval/src/dense_search.rs`,
`embedding/`) to be wired into `retrieve_files` for those query types,
or the in-tree functional floor (0.75) to be honestly revised toward
the BM25 lexical ceiling on this corpus (≈ 0.60).

That is a multi-cycle architectural change and the appropriate next
investigation for a future loop.

<!-- FEATURES_STATUS:total=123,passing=36,failing=0 -->
<!-- QUALITY_SCORE:0.92 -->
<!-- QUALITY_PASSED:1 -->
<!-- PHASE_2_COMPLETE -->
<!-- PHASE_3_COMPLETE -->
<!-- PHASE_4_COMPLETE -->
