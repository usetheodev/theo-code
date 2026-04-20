# SOTA Criteria — Cycle evolution/apr20-1553

**Target:** RM2 Tantivy closure + decay enforcer.

## Convergence requires

1. **Decay enforcer** — `MemoryLifecycleEnforcer::tick(age, usefulness, hit_count) -> MemoryLifecycle` with 3 transitions (Active→Cooling, Cooling→Archived, Archived→Archived) each covered by a named test. Pure logic, zero IO.
2. **Tantivy adapter** — `MemoryTantivyIndex` implements `MemoryRetrieval` from `theo-infra-memory`. Ingests typed memory docs with `source_type` filter. Per-type threshold honored via existing `RetrievalBackedMemory` config.
3. **Hygiene preserved** — score 73.300, zero new warnings, 0 test failures.
4. **Respects boundaries** — `theo-domain → nothing`. `theo-engine-retrieval → theo-domain only`. `theo-infra-memory` may add `theo-engine-retrieval` as a feature-gated optional dep.

## Scoring anchors

- **Pattern Fidelity** ≥ 2.5: cite MemGPT tier-decay + hermes isolated-mount rule per commit.
- **Architectural Fit** ≥ 2.5: new `MemoryTantivyIndex` sibling of `FileTantivyIndex` (no schema mixing); adapter crosses infra-memory only.
- **Completeness** ≥ 2.5: every promised method has coverage; runtime wiring of enforcer to `EpisodeSummary` is explicitly deferred.
- **Testability** ≥ 2.5: transitions + index round-trip exercised with in-memory fakes and real Tantivy RAM index.
- **Simplicity** ≥ 2.5: no new abstractions unless the reference pattern requires it.
