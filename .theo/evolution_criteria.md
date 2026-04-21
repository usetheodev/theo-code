# SOTA Criteria — Memory Superiority (cycle evolution/apr21)

**Target:** Implement `docs/plans/PLAN_MEMORY_SUPERIORITY.md` to reach "Memory & State: Theo > Hermes".

## Global Success Criteria (from plan §"Criterio de Sucesso Global")

A feature is SOTA when BOTH of the following hold:
1. It is **production-wired** (called from the agent hot path), not just compiled.
2. It matches or surpasses the Hermes-agent reference on its axis.

| ID | Criterion | Convergence gate |
|---|---|---|
| G1 | Memory prefetch/sync rodando em producao | Hook sequence verified with `RecordingProvider` test; `FileMemoryStore` ad-hoc path removed |
| G2 | Cross-session keyword search | Trait `SessionSearch` in theo-domain + impl; <50ms for 100 episodes |
| G3 | Token/cost tracking per-session | `TokenUsage` struct in theo-domain; accumulated per LLM response; persisted in `EpisodeSummary`; CLI end-of-run display |
| G4 | Compaction with memory hooks + oversized protection | `on_pre_compress()` wired; token-based tail (20%); per-message cap `context_window/4`; anti-thrashing skip |
| G5 | Knowledge gating (lessons) | `apply_gates()` wired post-run; `LessonStatus::Invalidated`; `Confirmed` lessons injected into prefetch |
| G6 | Hypothesis feedback loop | Hypotheses persisted + loaded; Laplace confidence update; auto-prune `against > for*2` |
| G7 | Memory lifecycle decay | Sidecar `.meta.json`; per-entry lifecycle filter in prefetch; legacy entries graceful |
| G8 | Frozen snapshot | `OnceLock<String>` in `BuiltinMemoryProvider`; second+ prefetch bypasses state read |
| G9 | Retrieval with budget packing | **BLOCKED** — requires eval dataset + `BudgetConfig.memory_pct`. Skipped this cycle. |
| G10 | Episode summaries fed back into context | Last N episodes (lifecycle != Archived, non-expired TTL) loaded at session start; injected as system messages; 5% token budget cap |

## Pre-Phase 0 gates (blockers)

| ID | Gate |
|---|---|
| P.1 | Episodes persisted to `.theo/memory/episodes/` (never `.theo/wiki/episodes/`) |
| P.2 | Unicode injection blocked: cyrillic lookalikes + zero-width characters rejected |
| P.3 | `schema_version: u32` field on `EpisodeSummary`, `MemoryLesson`, and builtin `.md` header |
| P.4 | `LessonStatus::Invalidated` variant (renamed from `Retracted`); no `Retracted` refs left |
| P.5 | (Deferred — desktop sidebar outside CAN-modify scope for this loop) |

## Hygiene floor

- Baseline: score=47.272 (L1=63.077, L2=31.467).
- Every commit must **maintain or increase** the score; drops trigger revert.
- `cargo test --workspace` MUST remain green.
- `cargo check --workspace --tests` MUST remain zero-warning.
- No new `unwrap()` / `expect()` in production code paths.

## SOTA Rubric Anchors (per-dimension minimum 2.5)

- **Pattern Fidelity ≥ 2.5**: each commit cites the Hermes/paper pattern it implements (`PLAN_MEMORY_SUPERIORITY.md` task id) and the evidence of conformance (test name + assertion).
- **Architectural Fit ≥ 2.5**: dependency direction respected (`theo-domain → nothing`; `theo-application → theo-infra-memory → theo-domain`). No apps bypass `theo-application`. No new workspace members. No new external deps beyond `unicode-normalization` (P.2).
- **Completeness ≥ 2.5**: every AC of the targeted task has a test or a runtime assertion. `memory_enabled=false` path preserved as no-op with zero overhead.
- **Testability ≥ 2.5**: RED tests precede GREEN code (TDD). Hook sequence tested with `RecordingProvider`. `test_no_dual_memory_injection` exists. Concurrency tests for bg prefetch use `tokio::test`.
- **Simplicity ≥ 2.5**: no new abstractions unless plan explicitly requires them. Max 200 LOC per commit. No speculative generalization.

## Convergence Definition

The cycle converges to "MEMORIA NIVEL SOTA" when:
1. G1-G8 + G10 atingidos (G9 blocked by eval dataset — plan-documented exception).
2. Pre-Phase 0 gates P.1-P.4 all passing (P.5 deferred as scope-excluded).
3. All DoD items in §"DoD Global" satisfied.
4. Hygiene score ≥ 47.272 (baseline preserved).
5. Rubric average ≥ 2.5 across all 5 dimensions.
