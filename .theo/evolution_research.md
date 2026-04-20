# Evolution Research — Cycle evolution/apr20-1553

**Prompt:** `Recomendação SOTA próxima cycle: priorizar (1) RM2 Tantivy + (2) decay enforcer`
**Builds on:** `outputs/agent-memory-plan.md` + cycle `evolution/apr20` (12 commits, memory subsystem landed).

## Targets

1. **RM2 Tantivy closure** — `RetrievalBackedMemory` provider (cycle apr20) binds to the `MemoryRetrieval` trait; no concrete backend exists. Need a Tantivy-backed adapter.
2. **Decay enforcer** — `MemoryLifecycle` tier enum (Active → Cooling → Archived) exists in `theo-domain/episode.rs:139` but has no `tick()` driving transitions from (age, usefulness, hit_count). MemGPT 3-tier parity requires enforced decay.

## Current state (verified)

- `theo-domain/episode.rs:139-182` — `MemoryLifecycle::next()` does naive tier bump, no signals.
- `theo-engine-retrieval/src/tantivy_search.rs` — 940 lines, `FileTantivyIndex` is strictly over `CodeGraph` File nodes (7 code-specific fields: path/filename/symbol/signature/doc/imports/path_segments). Not a drop-in home for memory docs.
- `theo-infra-memory/src/retrieval.rs` (cycle apr20) — `MemoryRetrieval` trait + `RetrievalBackedMemory` provider; threshold per `SourceType` (Code 0.35 / Wiki 0.50 / Reflection 0.60) + 15% token budget; binds to any `MemoryRetrieval` impl.

## Reference patterns

| Source | Pattern | Applied where |
|---|---|---|
| **MemGPT** [@packer2023] 3-tier (main/archival/recall) | Tier transitions driven by (staleness, hit_count, usefulness). No time-based auto-flush in Theo. | `theo-domain` — new `MemoryLifecycleEnforcer::tick()`. |
| **MemCoder** [@deng2026] structured memory with lifecycle | Typed knowledge object + gates (already in `MemoryLesson`). Extend to `EpisodeSummary` via the tier enforcer. | Pure logic in `theo-domain`; wiring to `EpisodeSummary` deferred. |
| **hermes-agent** markdown-backed LTM per user | Namespace isolation via separate files. Matches memory-wiki mount rule (RM5a). | New `MemoryTantivyIndex` sibling to `FileTantivyIndex` — no shared schema. |
| **Karpathy LLM Wiki** (2026) | Separate compiled artefact, namespaced `[[memory:slug]]` vs `[[code:slug]]`. | Same rationale for a separate Tantivy index. |

## Scope this cycle

- **P1**: `MemoryLifecycleEnforcer` (pure domain logic) + `DecayThresholds` + `tick(age, usefulness, hit_count)` → new tier. `#[test]` RED-GREEN pairs for each transition.
- **P2**: `MemoryTantivyIndex` in `theo-engine-retrieval` (new file `memory_tantivy.rs`), indexing memory docs by `(slug, namespace, body, source_type)`. Implements `MemoryRetrieval` via a thin adapter in `theo-infra-memory/src/retrieval/tantivy_adapter.rs`.
- **P3**: Wire a `TantivyMemoryBackend::from_pages()` constructor that ingests `MemoryWikiPage` + `MemoryLesson` + journal entries and honors `SourceType`.

## Defer

- `EpisodeSummary` auto-decay integration (runtime wiring of enforcer to session tick).
- MemCoder git-log intent mining.
- Desktop Tauri shim + vitest coverage.

## LOC budget

- Decay enforcer: ~120 LOC (domain, pure logic, TDD).
- MemoryTantivyIndex: ~150 LOC + adapter ~60 LOC.
- Total ~330 LOC across 2-3 commits, each ≤ 200 LOC.
