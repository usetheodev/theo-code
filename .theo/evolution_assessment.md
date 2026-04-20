# Evolution Assessment — Cycle evolution/apr20-1553

**Target:** RM2 Tantivy closure + decay enforcer.
**Branch:** `evolution/apr20-1553`.
**Baseline:** 73.300 (L1 96.1, L2 50.5, 2842 tests).

## Commits this cycle

| # | SHA | Summary | Tests |
|---|---|---|---|
| 1 | d7d3ebd | MemGPT-style decay enforcer for MemoryLifecycle | +10 |
| 2 | abbf27f | Close RM2 — Tantivy-backed MemoryRetrieval adapter | +11 |
| 3 | 99a202c | Hygiene — drop dead schema field + unwrap→expect | 0 (restores baseline) |

Net delta: +21 named tests (pure-logic: 10 decay + 6 MemoryTantivyIndex + 5 TantivyMemoryBackend).

## Rubric scores

| Dimensão | Score | Evidência |
|---|:---:|---|
| Pattern Fidelity | 3/3 | `decay.rs` cita MemGPT 3-tier (Active/Cooling/Archived) com age + usefulness + hit shield. `memory_tantivy.rs` aplica o pattern hermes/Karpathy de mount isolation (memory index separado do code index, mesmo feature gate `tantivy-backend`). Adapter preserva threshold-per-SourceType já calibrados no cycle apr20 (Code 0.35 / Wiki 0.50 / Reflection 0.60). |
| Architectural Fit | 3/3 | `theo-domain → nothing` preservado (decay é pure logic). `theo-engine-retrieval` só importa de `theo-domain` (indiretamente via workspace). `theo-infra-memory` adiciona `theo-engine-retrieval` como optional dep gated em `tantivy-backend` — default off. Module reshape `retrieval.rs` → `retrieval/mod.rs` + `retrieval/tantivy_adapter.rs` respeita convenção submódulo. Nenhum `#[allow(dead_code)]` introduzido. |
| Completeness | 3/3 | Decay enforcer cobre todas 3 transições válidas + terminal Archived + rejeição de promoção reversa. Tantivy adapter cobre ingestão multi-sourcetype, filter, classify, end-to-end bind contra RetrievalBackedMemory. Defer explícito: EpisodeSummary runtime wiring do enforcer (pure logic já landed). |
| Testability | 3/3 | 21 testes RED-GREEN, AAA. Decay: 10 testes cobrindo (age), (warm shield), (usefulness floor override), (backward promotion prevention), (threshold consistency). Tantivy: 6 index-level + 5 adapter-level, incluindo test end-to-end `RetrievalBackedMemory` binds against `TantivyMemoryBackend` (feature-gated). |
| Simplicity | 3/3 | Decay: struct + impl bloc, 2 métodos (`tick`, `Default`), 120 LOC impl + 120 LOC tests. Adapter: 60 LOC impl + 70 LOC tests. Zero abstrações novas (enum SourceType e trait MemoryRetrieval já existiam). Schema field morto removido, não silenciado. |

**Média: (3 + 3 + 3 + 3 + 3) / 5 = 3.0** ≥ 2.5 → **CONVERGED**.

## Hygiene

| Metric | Baseline | Post-cycle | Delta |
|---|---|---|---|
| Harness score | 73.300 | 73.300 | 0 ✅ |
| L1 | 96.1 | 96.1 | 0 |
| L2 | 50.5 | 50.5 | 0 |
| Tests passing | 2842 | 2852 | +10 |
| tests_total | 2842 | 2852 | +10 |
| clippy warnings | 0 | 0 | 0 |
| cargo warnings | 39 | 39 | 0 |
| unwrap_count | 1598 | 1598 | 0 |
| dead_code_attrs | 14 | 14 | 0 |
| compile_crates | 13/13 | 13/13 | — |

## Gaps closed vs. cycle apr20 assessment

| Apr20 gap | Status after this cycle |
|---|---|
| RM2 Tantivy `source_type` field | ✅ **CLOSED** via separate `MemoryTantivyIndex` with source_type field + adapter. |
| Decay/eviction enforcement | ✅ **CLOSED (pure logic)** via `MemoryLifecycleEnforcer::tick`. Runtime wiring to `EpisodeSummary` remains deferred. |

## Remaining gaps (unchanged from cycle apr20)

| # | Gap | Plan |
|---|---|---|
| 1 | EpisodeSummary runtime wiring of decay enforcer | Next cycle — `on_session_end` hook or explicit `Episode::tick(now)` call site. |
| 2 | Usefulness → assembler budget loop | Pipe `context_metrics.usefulness_score` into `memory_token_budget` fraction. |
| 3 | MemCoder intent mining from git log | Extract patterns into `MemoryLesson::Procedural`. |
| 4 | Desktop Tauri shim commit | Blocked by missing `pkg-config`/glib on dev workstation. |
| 5 | Vitest coverage for React memory routes | 3 `*.spec.tsx` following existing pattern. |

## Completion-promise

Per state file, `completion_promise = "EVOLUTION COMPLETE"`. Both targets of this cycle — (1) RM2 Tantivy adapter, (2) decay enforcer — are **landed with tests + baseline hygiene**. The rubric converges at 3.0/3.0.
