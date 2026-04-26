# theo-infra-memory — Revisao

> **Contexto**: Infrastructure layer do agent memory subsystem. Fan-out sobre multiplos `MemoryProvider`s.
>
> **Dependencias permitidas** (ADR-011): `theo-domain`, `theo-engine-retrieval` (opcional, feature-gated `tantivy-backend`).
>
> **Status global**: deep-review concluido em 2026-04-25. Cargo.toml verificado: deps respeitam ADR-011 (theo-domain workspace + theo-engine-retrieval optional via `tantivy-backend` feature). 100 tests passando, 0 falhas. `cargo clippy -p theo-infra-memory --lib --tests` silent (zero warnings).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `builtin` | `BuiltinMemoryProvider` — provider padrao embutido. | Revisado |
| 2 | `engine` | `MemoryEngine` + `EngineStats` — coordenador fan-out. | Revisado |
| 3 | `fs_util::atomic_write` | Escrita atomica temp-file-plus-rename (evita torn files). | Revisado |
| 4 | `lint` | `run_lint`, `LessonMetric`, `LintInputs`, `LintIssue`, `LintThresholds`, `Severity`, `render_json`. | Revisado |
| 5 | `retrieval::tantivy_adapter` | Adaptador tantivy para retrieval de memoria (feature-gated). | Revisado |
| 6 | `retrieval (core)` | `MemoryRetrieval`, `RetrievalBackedMemory`, `ScoredMemory`, `SourceType`, `ThresholdConfig`, `pack_within_budget`. | Revisado |
| 7 | `security::scan` | `scan` + `InjectionReason` — deteccao de injection em memorias. | Revisado |
| 8 | `session_search_fs` | `FsSessionSearch` + `render_hits` — busca em sessoes via filesystem. | Revisado |
| 9 | `wiki::compiler` | Compilador wiki: codigo → paginas. | Revisado |
| 10 | `wiki::hash` | `HashManifest`, `SourceHash` — controle de mudanca. | Revisado |
| 11 | `wiki::lint` | `lint_pages` — lint de paginas wiki. | Revisado |
| 12 | `wiki::parse_page` | Parser de paginas wiki. | Revisado |

---

## Notas de Deep-Review por Dominio

> Auditoria orientada a: (1) atomicidade (fs operations), (2) tantivy feature-gate, (3) deps respeitando ADR-011, (4) injection scanning robustness, (5) hygiene.

### 1. builtin (142 LOC)
`BuiltinMemoryProvider` — provider padrao. Implementa `theo_domain::memory::MemoryProvider`. `user_hash(user_id)` deterministic SHA256 para per-user namespace. Wrappa o BuiltinMemoryProvider em `attach_memory_to_config` (theo-application::memory_factory). Fan-out target principal do `MemoryEngine`.

### 2. engine (147 LOC)
`MemoryEngine` coordena fan-out: register multiplos providers, fan_out async para todos em paralelo via `futures::future::join_all`. `EngineStats` captura `(provider_id, success, error)`. Errors isolados por provider (um falha → outros continuam). Aderencia ao contrato `MemoryProvider::on_*` no theo-domain.

### 3. fs_util::atomic_write (45 LOC)
`atomic_write(path, content)` — pattern temp-file-plus-rename. Cria `<path>.tmp.<pid>.<rand>`, escreve, fsync, rename. Garantia: outros processos nao veem arquivo parcial. Used by builtin, wiki::compiler, hash manifests.

### 4. lint (137 LOC)
`run_lint(LintInputs) -> Vec<LintIssue>`. `LessonMetric` (count, last_seen), `LintThresholds` (max_lessons_per_topic), `Severity::{Info, Warning, Error}`, `render_json`. Cobertura via theo-test-memory-fixtures.

### 5. retrieval::tantivy_adapter (83 LOC, feature-gated)
Adaptador tantivy → `MemoryRetrieval`. Apenas compilado quando `tantivy-backend` feature ativo. Bridge entre infra-memory e theo-engine-retrieval.

### 6. retrieval/mod.rs core (158 LOC)
`MemoryRetrieval` trait, `RetrievalBackedMemory` (wrapper), `ScoredMemory { score, content, source }`, `SourceType::{Lesson, Episode, Skill}`, `ThresholdConfig`, `pack_within_budget(scored, budget_tokens)` — orcamento por tokens, ordena por score desc, picks ate budget.

### 7. security::scan (526 LOC)
**Maior modulo do crate** — proporcional a importancia. `scan(content) -> Option<InjectionReason>`. Detecta tentativas de prompt injection em memorias antes de injetar no context. `InjectionReason::{InstructionInjection, JailbreakAttempt, ToolCallInjection, ProviderTokens, OtherSuspicious}`. Test corpus extenso baseado em prompts adversariais conhecidos. Defensa em profundidade: combina com prompt_sanitizer no theo-domain (T1.2).

### 8. session_search_fs (106 LOC)
`FsSessionSearch` — busca em sessoes salvas via filesystem (sem indice). Usado quando tantivy nao esta wired. `render_hits(hits)` formatador. Fallback simples para o full-text via tantivy.

### 9. wiki::compiler (226 LOC)
Compilador `codigo → paginas wiki` deterministico. Source-of-truth para `.theo/wiki/code/`. Le SourceHash manifest, identifica mudancas, recompila apenas afetados. Atomic-write das paginas. Integracao com theo-engine-retrieval para enrich.

### 10. wiki::hash (87 LOC)
`HashManifest` (file_path → SourceHash). `SourceHash::compute(content)` SHA256. Permits: detect "this file changed since last compile" para incremental builds.

### 11. wiki::lint (131 LOC)
`lint_pages(pages) -> Vec<Issue>` + `parse_page(slug_hint, raw)`. Front-matter validation, link integrity, metadata schema.

### 12. wiki::parse_page (re-export of wiki::lint::parse_page)
Re-exportado em `wiki::mod.rs:13`: `pub use lint::{lint_pages, parse_page}`. Decisao de design: parser e lint vivem juntos (mesmo arquivo, mesmas validation rules), exportados separadamente como API publica.

---

## Conclusao

Todos os 12 dominios listados revisitados e marcados **Revisado**.

**Invariantes verificados:**
- ADR-011 respeitada: Cargo.toml lista `theo-domain` + `theo-engine-retrieval` (optional via `tantivy-backend` feature). External deps: tokio, async-trait, serde, serde_json, thiserror, futures, sha2. ✓
- Atomic-write pattern (`fs_util::atomic_write`) usado consistentemente para evitar torn files. ✓
- Security scan pre-context-injection (526 LOC dedicados) — defesa em profundidade contra prompt injection. ✓
- Errors per-provider isolados no fan-out (`MemoryEngine::EngineStats`). ✓
- Tantivy feature-gated corretamente (apenas compila quando `tantivy-backend` ativo). ✓

**Hygiene:**
- 100 tests passando, 0 falhas
- `cargo clippy -p theo-infra-memory --lib --tests` silent (zero warnings — sem fixes nesta auditoria, ja estava limpo)
- Maior modulo: security.rs (526 LOC) — proporcional a importancia de injection detection
- Test fixtures via `theo-test-memory-fixtures` workspace dev-dep

Sem follow-ups bloqueadores. O fan-out + atomic-write + security-scan formam tres invariantes de robustez bem definidas para a memoria do agent.
