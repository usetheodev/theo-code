# SOTA Criteria — SOTA Agent Memory

**Version:** 5.0 (memory cycle)
**Date:** 2026-04-20
**Plan:** `outputs/agent-memory-plan.md`
**Meeting:** `.claude/meetings/20260420-134446-agent-memory-sota.md`

## Completion promise decoder

`TODAS TASKS, E DODS CONCLUIDOS E VALIDADOS`:
- **TODAS TASKS**: 5 pre-reqs + RM0 + RM1 + RM3a + RM2 + RM4 + RM5a + RM5b + UI(3 rotas) + test-fixtures crate + lint tool = 13 deliverables.
- **E DODS**: cada uma satisfaz os 10 Global DoDs + extras (Rust safety: `tokio::sync::RwLock`, atomic write, zero unwrap; feature flags; kill switches).
- **CONCLUIDOS E VALIDADOS**: 61 ACs totais como named `#[test]` passando (8/10 fases puras; 1 fase RM5b usa MockCompilerLLM). Harness ≥ 73.300.

Promise so emitido no Phase 5 quando TODOS os 3 clauses sao verdadeiros.

## Rubric (5 dim, 0-3, converge a ≥ 2.5 por ciclo)

### 1. Pattern Fidelity
- **3**: codigo tracea referencia (in-code comment "ref: hermes memory_provider.py:42-120").
- **2**: pattern aplicado com adaptacao idiomatica Rust.
- **1**: inspiracao solta.
- **0**: ad-hoc.

### 2. Architectural Fit
- **3**: `theo-domain → nothing` preservado; consumers via trait; no circular imports; `tokio::sync` (nao `std::sync`); thiserror typed errors.
- **2**: friccao menor.
- **1**: duplicacao cross-crate.
- **0**: violacao de boundary ou `unwrap()` em producao.

### 3. Completeness (per-phase)
- **3**: todos ACs da fase passam; DoDs extras presentes; regression test enforca invariante futuro.
- **2**: ACs passam, extras parciais.
- **1**: so happy path.
- **0**: scaffolding.

### 4. Testability
- **3**: cada AC e um named `#[test]` com AAA; integration test exercita pipeline; fases RM5b+ usam MockLLM deterministico; property tests onde aplicavel.
- **2**: unit + happy + 1 failure.
- **1**: smoke only.
- **0**: nenhum.

### 5. Simplicity
- **3**: ≤ 200 LOC, sem abstracoes especulativas, cada novo tipo tem ≥ 2 consumers.
- **2**: 1 fase > 200 LOC justificado.
- **1**: abstracao para uso hipotetico.
- **0**: refactor sprawl.

## Global DoD (inherited do plano §1)

1. `cargo test --workspace` 0 failures
2. `cargo check --workspace --tests` 0 warnings
3. Pre-commit hook sem `--no-verify`
4. Zero `Co-Authored-By` / `Generated-with`
5. `theo-domain → nothing`
6. TDD order documentado
7. ≤ 200 LOC/fase
8. Harness ≥ 73.300
9. Zero `unwrap()` em producao
10. Doc atualizada

**Extras da ata:**
- `tokio::sync::RwLock` em toda concorrencia async
- Atomic write via `theo-infra-memory::fs_util::atomic_write`
- Feature flag `agent.memory_enabled=false` default
- Kill switch `WIKI_COMPILE_ENABLED=false` default-off em CI

## Per-phase completeness checkpoints

| Fase | AC count | Must land |
|---|---:|---|
| RM-pre-1 | 4 | `.gitignore` atualizado |
| RM-pre-2 | 3 | `MemoryError` enum em theo-domain |
| RM-pre-3 | 1 | `unwrap()` em run_engine.rs:786 corrigido |
| RM-pre-4 | 2 | ADR 008 commitado |
| RM-pre-5 | 2 | `memory_enabled` field; default false |
| RM0 | 7 | 4 hooks wired; integration test em order |
| RM1 | 8 | MemoryEngine fan-out + isolation + atomic_write util |
| RM3a | 9 | BuiltinMemoryProvider + security scan + idempotency |
| RM2 | 7 | Retrieval-backed + source_type filter + thresholds |
| RM4 | 10 | MemoryLesson + 7 gates + quarantine 7d + integra reflector.rs |
| RM5a | 6 | Hash manifest + wiki_lint + namespace check |
| RM5b | 8 | MockCompilerLLM + determinism byte-equal + hard limits |
| UI | 8 | 3 rotas MVP + Tauri commands |
| Lint | 6 | `theo memory lint` com 6 metricas |
| **Total** | **81** | (61 ACs + ~20 extras em DoDs) |

## Guardrails desta rodada

- **Baseline 73.300** (nao 75.150) — score atual pos-merge do develop com dead-code regression. Meta: nao regredir.
- **Rename obrigatorio**: novo tipo = `MemoryLesson`, NAO `Reflection` (colide com `theo-domain::evolution::Reflection`).
- **`.gitignore` deve entrar antes** de qualquer write data pessoal (RM-pre-1).
- **Dois mounts fisicos** (`.theo/wiki/code/` vs `.theo/wiki/memory/`) + um Tantivy index logico com `source_type`.
- **Cross-link unidirecional**: memory → code permitido; code → memory proibido.
- **8/10 fases devem ser RED-GREEN puros** (zero LLM real nos tests). Apenas RM5b usa MockCompilerLLM.

## Convergence gate (Phase 5)

- [ ] 13+ commits `evolution:` landed
- [ ] 61+ named `#[test]` passando
- [ ] `cargo test --workspace` 0 failures
- [ ] `cargo check --workspace --tests` 0 warnings
- [ ] Harness ≥ 73.300
- [ ] Feature flag `agent.memory_enabled` default false validado
- [ ] Kill switch `WIKI_COMPILE_ENABLED` validado
- [ ] 3 rotas UI acessiveis
- [ ] `theo memory lint` funcional
- [ ] ADR 008 aprovado
- [ ] `.gitignore` cobre `.theo/memory/`, `.theo/wiki/memory/`, `.theo/reflections.jsonl`

Qualquer item unchecked → volta IMPLEMENT.
