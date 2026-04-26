# God-Files Decomposition Plan (T4.5)

Each file below exceeds the 800-LOC limit (400 LOC for UI) and is currently
grandfathered into `.claude/rules/size-allowlist.txt` with sunset
**2026-07-23**. This document captures the decomposition contract per file
so the refactor work is pre-planned before the sunset bites.

Status legend: ⬜ not started · 🔄 in progress · ✅ complete

## Top-12 god files (> 1 000 LOC)

### ⬜ `crates/theo-agent-runtime/src/run_engine.rs` — 2 514 LOC
**Function `execute_with_history`** alone is 1 714 LOC (CCN ~ 201).
Owned by T4.1.

**Decomposition target:**

```
run_engine.rs                        < 500 LOC (orchestrator only)
  ├── run_engine/prepare_turn.rs     < 300 LOC  — assembles context, enforces budget
  ├── run_engine/dispatch_tools.rs   < 300 LOC  — dispatches tool calls through bridge
  ├── run_engine/collect_results.rs  < 200 LOC  — aggregates outputs + telemetry
  ├── run_engine/finalize_turn.rs    < 200 LOC  — applies state transitions
  └── run_engine/persist_episode.rs  < 200 LOC  — writes the episode to memory
```

Each sub-module gets its own `#[cfg(test)] mod tests` + at least one
integration case. Existing regression suites must pass unchanged.

**Blocking:** T1.1 (agent-runtime → infra-llm/tooling traits) should
land first so the sub-modules can depend on traits instead of concrete
provider types. If T1.1 is not ready by 2026-05-15 we start T4.1 anyway
and refactor sub-modules when T1.1 completes.

---

### ⬜ `crates/theo-engine-retrieval/src/wiki/generator.rs` — 2 019 LOC
Generates the Wiki content from the code graph.

**Decomposition target:**

```
wiki/generator.rs                      < 400 LOC (entry point + orchestration)
  ├── wiki/generator/sections.rs       < 500 LOC  — per-section builders
  ├── wiki/generator/templates.rs      < 400 LOC  — markdown templates + helpers
  ├── wiki/generator/graph_snippets.rs < 400 LOC  — graph-derived code excerpts
  └── wiki/generator/link_rewriter.rs  < 300 LOC  — cross-link resolution
```

Owned by T4.5 (no blockers).

---

### ⬜ `crates/theo-engine-parser/src/extractors/language_behavior.rs` — 1 758 LOC
Per-language behaviour overrides for the symbol extractor.

**Decomposition target:** one module per language family, capped at 400 LOC:

```
extractors/language_behavior.rs       < 300 LOC (dispatch)
extractors/language_behavior/c_family.rs     — C, C++, C#, Java, Go
extractors/language_behavior/web.rs          — JS, TS, PHP
extractors/language_behavior/dynamic.rs      — Python, Ruby, Kotlin, Scala, Swift
extractors/language_behavior/rust.rs
```

**Risk:** tree-sitter grammar differences leak through; keep each
sub-module self-contained.

---

### ⬜ `crates/theo-application/src/use_cases/graph_context_service.rs` — 1 735 LOC
**Decomposition target:** split the orchestration from the per-strategy
adapters.

```
graph_context_service.rs               < 500 LOC (orchestrator)
  ├── graph_context/retrieval_adapter.rs   < 400 LOC
  ├── graph_context/wiki_adapter.rs        < 400 LOC
  └── graph_context/assembly_adapter.rs    < 500 LOC
```

Blocked by T1.6 (retrieval dep already reconciled). Low risk.

---

### ⬜ `crates/theo-engine-parser/src/types.rs` — 1 666 LOC
Pure type definitions. Split by domain:

```
extractors/types/symbol.rs            — Symbol, SymbolKind, Scope
extractors/types/span.rs              — Span, Location, Range
extractors/types/reference.rs         — Reference, Edge
extractors/types/diagnostic.rs
```

**Risk:** downstream `use` imports. Ship a re-export module that
preserves `pub use extractors::types::*` to avoid breaking callers.

---

### ⬜ `crates/theo-engine-retrieval/src/assembly.rs` — 1 613 LOC
Assembles retrieval candidates into a context package.

**Decomposition:**

```
assembly.rs                            < 400 LOC
  ├── assembly/ranking.rs              < 400 LOC
  ├── assembly/budgeting.rs            < 300 LOC
  └── assembly/serialization.rs        < 400 LOC
```

---

### ⬜ `crates/theo-engine-graph/src/cluster.rs` — 1 586 LOC
Graph-clustering algorithms. Naturally factors by algorithm:

```
cluster.rs                             < 400 LOC (dispatcher + shared types)
  ├── cluster/louvain.rs               < 500 LOC
  ├── cluster/label_propagation.rs     < 400 LOC
  └── cluster/clique.rs                < 400 LOC
```

---

### ⬜ `crates/theo-engine-retrieval/src/file_retriever.rs` — 1 419 LOC
Path-aware retrieval wrapper.

**Decomposition:** pull out file-scoring, snippet extraction, and format
conversion into sibling modules. Target < 500 LOC each.

---

### ⬜ `crates/theo-engine-parser/src/extractors/symbols.rs` — 1 394 LOC
Symbol extraction from AST. Split by kind: definitions, usages, types.

---

### ⬜ `crates/theo-engine-parser/src/symbol_table.rs` — 1 350 LOC
Core symbol table — needs careful refactor because `pub` callers exist.
Split: `storage.rs` + `queries.rs` + `traversal.rs`. Owned by T4.5.

---

### ⬜ `crates/theo-domain/src/episode.rs` — 1 325 LOC
Episode type + serde. Split: `types.rs`, `builders.rs`, `serde_impls.rs`,
`validation.rs`. Owned by T4.5.

---

### ⬜ `apps/theo-cli/src/tui/app.rs` — 1 250 LOC
Owned by **T4.3** (`update` function alone is CCN ~ 52).

**Decomposition target:** one event handler per module, driven by the
`Msg` enum:

```
tui/app.rs                             < 400 LOC (update dispatcher)
  ├── tui/app/compose_msg.rs
  ├── tui/app/stream_msg.rs
  ├── tui/app/auth_msg.rs
  └── tui/app/navigation_msg.rs
```

Blocked by T1.2 (theo-cli decouple) — the infra types this file uses
need to be reachable through theo-application first.

---

## Mid-tier files (800–1 000 LOC, deadline 2026-07-23)

The broader Phase-4 scope lives in `size-allowlist.txt` under the
"Phase 4 debt" block. Each entry there is expected to be either
decomposed or re-allowlisted with a fresh rationale before sunset.
No individual plan is required for mid-tier files — the owners pick
the decomposition shape during the refactor PR.

## Cross-cutting principles

1. **Test first.** Every refactor PR must add or preserve at least one
   test per extracted sub-module. The file's baseline regression suite
   must stay green.
2. **No silent behaviour change.** Extracting a helper never alters
   observable outputs; prefer a `refactor(scope):` commit over
   `feat(scope):` unless a user-facing change is explicit in the PR
   description.
3. **Use re-exports for API compatibility.** When splitting a `types`
   module, ship a `pub use …::*` at the old path to keep downstream
   imports stable until the next breaking version.
4. **Update `size-allowlist.txt` in the same PR.** Either remove the
   entry (if the file dropped below the threshold) or raise the
   ceiling + extend the sunset with a new reason.
5. **CHANGELOG + ADR.** Each decomposition of a file > 1 500 LOC MUST
   reference an ADR (T6.6) explaining the factoring choice.

## Tracking

Reviewers should tick the checkbox at the top of each section once the
corresponding PR lands. The sunset column in `size-allowlist.txt` is
the hard deadline; extending it requires a PR with justification.
