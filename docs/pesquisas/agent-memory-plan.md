# Sistema de Memoria SOTA — Plano Executavel

**Fonte:** `outputs/agent-memory-sota.md` (research, 3800 palavras)
**Ata de aprovacao:** `.claude/meetings/20260420-134446-agent-memory-sota.md` (16 agentes, VEREDITO REVISED com 20 decisoes)
**Data:** 2026-04-20
**Branch alvo:** `evolution/memory-*` (criar por fase)

---

## 0. Goal, success metrics, non-goals

**Goal.** Sistema de memoria multi-camada (STM/WM/LTM-semantic/LTM-episodic/LTM-procedural/Retrieval/MemoryLesson/Meta) com Karpathy LLM Wiki como LTM-semantic, coordenado por `MemoryEngine` de decisao — nao armazenamento. Agente lembra projetos/preferencias entre sessoes com recovery auditavel.

**Success metrics (medidas no `theo-benchmark` + `theo memory lint`):**

| Metrica | Baseline | Target (apos RM5b) |
|---|---|---|
| Wiki-to-recall ratio | N/A | ≥ 30% das pages retornadas em prefetch |
| Reflections-per-compression | N/A | 2-5 (media) |
| Reflection-hit-rate 30d | N/A | ≥ 40% |
| Orphan episodes | N/A | 0 |
| Wiki manifest staleness | N/A | < 2h em repos ativos |
| Recall p50 latency | N/A | < 500ms |
| Memory compromise recovery | impossivel (edita .md manual) | ≤ 3 cliques no theo-desktop |
| Workspace tests | 2788 | ≥ 2788 |
| Harness score | 75.150 | ≥ 75.150 |

**Non-goals explicitos:**

- Fine-tuning continuo (Karpathy end-game): fora do MVP. RM7+ potencial.
- Sharing de memoria entre usuarios/equipes: single-user por usuario/repo. Multi-tenancy futura.
- Integracao com provedores comerciais (Mem0 / Zep hosted): so trait-ready; impl concreta opcional.
- UI avancada (knowledge graph visual estilo Graphiti): 3 rotas MVP suficientes.
- A-MEM Zettelkasten-style (arXiv:2502.12110): opcional pos-MVP (RM7).
- Budget hard enforcement: router reporta custo; enforcement em feature separada.

---

## 1. Global Definition of Done

Cada PR DEVE satisfazer os 10 gates abaixo antes de merge:

1. `cargo test --workspace` exits 0 (atualmente 2788).
2. `cargo check --workspace --tests` emite 0 warnings.
3. Pre-commit hook (`.githooks/pre-commit` + `commit-msg`) passa sem `--no-verify`.
4. **Nenhum trailer** `Co-Authored-By:` ou `Generated-with` (hook enforca).
5. `theo-domain → (nothing)` inviolavel; `theo-infra-memory → theo-domain only`.
6. **TDD**: commit body cita o teste RED primeiro; implementacao vem depois.
7. Cada fase ≤ 200 LOC (test-runner impoe split se estourar).
8. Harness score ≥ 75.150.
9. Zero `unwrap()` em producao; typed errors via `thiserror`.
10. Doc atualizada: ADR/`docs/current/memory-architecture.md` + `.theo/evolution_research.md`.

**Extras inherited da ata:**

- `tokio::sync::RwLock` (nao `std`) em toda concorrencia async.
- Atomic write via `theo-infra-memory::fs_util::atomic_write` (temp + rename) para todos markdown.
- Kill switches + feature flags obrigatorios antes de self-evolving (`agent.memory_enabled`, `WIKI_COMPILE_ENABLED`).
- `.gitignore` tem `.theo/memory/` + `.theo/wiki/memory/` + `.theo/reflections.jsonl` (pessoal).

---

## 2. Pre-requisitos (RM-pre)

Antes de qualquer RM iniciar, 5 itens bloqueantes:

### RM-pre-1 — `.gitignore` protege dados pessoais

**Files**: `.gitignore`

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| PRE1-AC-1 | `test_gitignore_excludes_theo_memory` | `git check-ignore .theo/memory/test.md` returns 0 |
| PRE1-AC-2 | `test_gitignore_excludes_memory_wiki` | `.theo/wiki/memory/foo.md` ignored |
| PRE1-AC-3 | `test_gitignore_includes_code_wiki` | `.theo/wiki/code/foo.md` NOT ignored |
| PRE1-AC-4 | `test_gitignore_excludes_reflections` | `.theo/reflections.jsonl` ignored |

**DoD**: `.gitignore` commitado antes de qualquer RM write data pessoal. **LOC**: 10. **Risk**: Very low.

### RM-pre-2 — `MemoryError` typed enum

**Files**: `crates/theo-domain/src/memory/error.rs` (novo, `memory.rs` vira `memory/mod.rs`)

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| PRE2-AC-1 | `test_memory_error_variants_serde_roundtrip` | Todas variantes round-trip |
| PRE2-AC-2 | `test_memory_error_carries_source` | `StoreFailed { key, source }` preserva io::Error |
| PRE2-AC-3 | `test_memory_error_implements_std_error` | `Box<dyn std::error::Error>` aceita MemoryError |

**Schema**:
```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MemoryError {
    #[error("store write failed for key `{key}`: {source}")]
    StoreFailed { key: String, #[source] source: std::io::Error },
    #[error("wiki compilation failed: {reason}")]
    CompileFailed { reason: String },
    #[error("recall query failed: {source}")]
    RetrieveFailed { #[source] source: Box<dyn std::error::Error + Send + Sync> },
    #[error("lesson gate rejected: {reason}")]
    GateRejected { reason: String },
}
```

**DoD**: enum em theo-domain, 3 tests, exportado. **LOC**: ~40. **Risk**: Low.

### RM-pre-3 — Fix `unwrap()` em `run_engine.rs:786`

**Files**: `crates/theo-agent-runtime/src/run_engine.rs`

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| PRE3-AC-1 | `test_llm_result_none_returns_typed_error` | `llm_result = None` retorna `LlmError::NoResponse` em vez de panic |

**DoD**: zero `.unwrap()` em hot path do run_engine. **LOC**: ~20. **Risk**: Low.

### RM-pre-4 — ADR 008 `theo-infra-memory` crate

**Files**: `docs/adr/008-theo-infra-memory.md` (novo)

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| PRE4-AC-1 | `test_workspace_cargo_toml_lists_12_crates` | `Cargo.toml` members = 12 |
| PRE4-AC-2 | `test_adr_008_exists` | Arquivo ADR presente e assinado |

**Conteudo do ADR**: justifica 12a crate por simetria com `theo-infra-llm`/`theo-infra-auth` (adapters de servicos externos). Documenta imports permitidos e direcao de dependencia.

**DoD**: ADR revisado por arch-validator; CLAUDE.md atualiza contagem de 11 para 12 crates. **LOC**: ~80 ADR markdown. **Risk**: Zero (doc only).

### RM-pre-5 — Feature flag `agent.memory_enabled`

**Files**: `crates/theo-agent-runtime/src/config.rs`

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| PRE5-AC-1 | `test_memory_enabled_default_false` | `AgentConfig::default().memory_enabled == false` |
| PRE5-AC-2 | `test_memory_enabled_toml_roundtrip` | TOML parse preserva flag |

**DoD**: flag em AgentConfig; default false; documentado em `.theo/config.toml.example`. **LOC**: ~15. **Risk**: Very low.

---

## 3. Roadmap (RM0 → RM5b)

Reordenado conforme decisao C3 da ata: RM3a antes de RM2.

---

### RM0 — Wire MemoryProvider no agent_loop

**Objetivo.** Invocar os 4 hooks (`prefetch`, `sync_turn`, `on_pre_compress`, `on_session_end`) no `agent_loop.rs`, gated por `config.memory_enabled`. Sem provider ativo → NullMemoryProvider (behavior-preserving).

**Files**:
- `crates/theo-agent-runtime/src/agent_loop.rs` (wire 4 hooks)
- `crates/theo-agent-runtime/src/memory_lifecycle.rs` (novo helper)
- `crates/theo-domain/src/memory/null.rs` (ja pode existir; garantir NullMemoryProvider)
- `crates/theo-agent-runtime/tests/memory_integration_rm0.rs` (novo)

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| RM0-AC-1 | `test_prefetch_called_before_llm` | `MockProvider` recebe `prefetch` antes da chamada LLM |
| RM0-AC-2 | `test_sync_turn_called_after_llm` | `sync_turn(&CompletedTurn)` chamado apos resposta |
| RM0-AC-3 | `test_on_pre_compress_called` | Hook dispara no callback de compaction |
| RM0-AC-4 | `test_on_session_end_called` | Hook dispara em convergence/abort |
| RM0-AC-5 | `test_memory_disabled_skips_all_hooks` | `memory_enabled=false` → zero invocacoes |
| RM0-AC-6 | `test_null_provider_preserves_behavior` | Regression: todas as 2788 tests passam com NullMemoryProvider |
| RM0-AC-7 (integration) | `test_memory_hooks_invoked_in_order` | Sequence recorded: prefetch → llm → sync_turn → [compaction? on_pre_compress] → on_session_end |

**DoD extras**: `MemoryLifecycle` helper extraido; integration test usa `MockProvider` que grava sequence; feature flag enforcement testado. **LOC target**: ~120. **Risk**: Medium (hot path). **Dependencias**: RM-pre 1-5.

---

### RM1 — MemoryEngine coordinator

**Objetivo.** Criar crate `theo-infra-memory` e `MemoryEngine` em `theo-application`. Fan-out para multiplos providers; error isolation (provider que panics nao derruba loop).

**Files**:
- `crates/theo-infra-memory/Cargo.toml` (novo)
- `crates/theo-infra-memory/src/lib.rs`
- `crates/theo-infra-memory/src/fs_util.rs` (atomic_write util)
- `crates/theo-application/src/memory/engine.rs` (novo)
- `crates/theo-application/tests/memory_engine.rs` (novo)

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| RM1-AC-1 | `test_fanout_prefetch_concatenates_results` | 2 providers → results mesclados via fence XML |
| RM1-AC-2 | `test_panicking_provider_does_not_block_fanout` | Provider A panics → Provider B ainda roda, logs WARN |
| RM1-AC-3 | `test_only_one_external_provider_allowed` | Registrar 2 externos → WARN + reject 2o |
| RM1-AC-4 | `test_providers_dispatched_in_registration_order` | Ordem determinista |
| RM1-AC-5 | `test_fence_wraps_each_provider_output` | `<memory-context source="provider_id">...</memory-context>` |
| RM1-AC-6 | `test_sync_turn_fans_out_to_all` | Write chamado em todos (broadcast) |
| RM1-AC-7 | `test_tokio_rwlock_allows_concurrent_reads` | Dois prefetch paralelos terminam sem deadlock |
| RM1-AC-8 (integration) | `test_engine_end_to_end_with_2_providers` | `MemoryEngine::new(vec![A, B])` + ciclo completo |

**DoD extras**: `fs_util::atomic_write` via temp+rename testado; `tokio::sync::RwLock` (nao std); new crate cita `theo-domain` only. **LOC target**: ~180. **Risk**: Low (novo codigo isolado). **Dependencias**: RM0.

---

### RM3a — BuiltinMemoryProvider + security scan (reordenado: antes de RM2)

**Objetivo.** Impl concreto de `MemoryProvider` backed por `.theo/memory/<user-hash>.md`. Security scan port de `hermes memory_tool.py:65-103`. Idempotency obrigatoria.

**Files**:
- `crates/theo-infra-memory/src/builtin.rs`
- `crates/theo-infra-memory/src/security.rs`
- `crates/theo-infra-memory/src/security_test_cases.toml` (fixture)
- `crates/theo-infra-memory/tests/builtin.rs` (novo)

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| RM3a-AC-1 | `test_injection_pattern_prompt_rejected` | "ignore previous instructions" bloqueado |
| RM3a-AC-2 | `test_injection_pattern_exfil_rejected` | "curl.*\\$API_KEY" bloqueado |
| RM3a-AC-3 | `test_injection_pattern_shell_escape_rejected` | "; rm -rf" bloqueado |
| RM3a-AC-4 | `test_all_security_patterns_covered` | Iterar `security_test_cases.toml` e assertar rejeicao |
| RM3a-AC-5 | `test_concurrent_writes_serialize` | 10 tokio::spawn escrevendo → no race, file integro |
| RM3a-AC-6 | `test_snapshot_stable_mid_session` | Frozen snapshot preserva estado durante prefetch |
| RM3a-AC-7 | `test_idempotent_upsert_by_sha256_key` | Mesmo turn 2x → 1 entry |
| RM3a-AC-8 | `test_user_hash_isolation` | user_a nao ve memoria de user_b |
| RM3a-AC-9 (integration) | `test_memory_md_written_and_recalled_across_sessions` | Session 1 escreve; Session 2 prefetch recupera |

**DoD extras**: `.theo/memory/<user-hash>.md` path resolution; SHA256 key obrigatoria; test-coverage security scan ≥ 85%; `tokio::fs` apenas (nao std::fs). **LOC target**: ~200 (split se estourar). **Risk**: Medium-High (security). **Dependencias**: RM1.

---

### RM2 — RetrievalBackedMemory

**Objetivo.** Provider que delega `prefetch` para `theo-engine-retrieval`. Namespace `source_type` na Tantivy; thresholds por tipo; memory_token_budget 15% do total.

**Files**:
- `crates/theo-infra-memory/src/retrieval.rs`
- `crates/theo-engine-retrieval/src/tantivy_search.rs` (add `source_type` field — cirurgico)
- `crates/theo-engine-retrieval/src/assembly.rs` (add `memory_token_budget`)
- `crates/theo-infra-memory/tests/retrieval.rs` (novo)

**Feature flag**:
```toml
[features]
memory-retrieval = ["theo-engine-retrieval"]
# default-off; habilitado explicitamente por tools/app
```

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| RM2-AC-1 | `test_prefetch_queries_retrieval` | MockRetrievalEngine → entries retornados |
| RM2-AC-2 | `test_source_type_filter_in_tantivy` | Query com `source_type="memory"` ignora docs de code |
| RM2-AC-3 | `test_threshold_per_type_calibrated` | code:0.35, wiki:0.50, reflection:0.60 |
| RM2-AC-4 | `test_memory_budget_15_percent` | Total 20k tokens → memory cap 3k |
| RM2-AC-5 | `test_code_ranking_not_cannibalized` | Memory results nao empurram code para fora do topk |
| RM2-AC-6 | `test_no_cross_encoder_for_memory` | Memory path pula reranker (latency) |
| RM2-AC-7 (integration) | `test_end_to_end_prefetch_returns_scored_entries` | Usa `three_community_fixture()` existente |

**DoD extras**: nenhum novo Tantivy index fisico; filter em query time; test reusa fixture existente; feature gate testado compile-sem. **LOC target**: ~150. **Risk**: Medium (toca retrieval). **Dependencias**: RM3a (Builtin deve estar estavel).

---

### RM4 — MemoryLesson (renomeado) + 7 gates

**Objetivo.** Novo tipo `MemoryLesson` (nao "Reflection" — colide com `theo-domain::evolution::Reflection`). 7 gates de write-path: confidence bounds, evidence count, contradiction scan, provenance hash lock, semantic dedup, quarantine 7d, aging decay.

**Files**:
- `crates/theo-domain/src/memory/lesson.rs` (novo)
- `crates/theo-domain/src/memory/gates.rs` (novo — traits)
- `crates/theo-infra-memory/src/lesson_store.rs` (impl concreta)
- `crates/theo-agent-runtime/src/lesson_bridge.rs` (integra com reflector.rs existente)
- `crates/theo-agent-runtime/tests/lesson.rs` (novo)

**Schema**:
```rust
pub struct MemoryLesson {
    pub id: Ulid,
    pub lesson: String,
    pub trigger: String,
    pub confidence: f32,                         // 0.6..0.95
    pub evidence_event_ids: Vec<String>,         // len >= 2
    pub evidence_hashes: Vec<[u8; 32]>,          // SHA256 no write
    pub category: LessonCategory,                // Semantic/Procedural/Meta
    pub status: LessonStatus,                    // Quarantine/Confirmed/Retracted
    pub created_at: DateTime<Utc>,
    pub promoted_at: Option<DateTime<Utc>>,
    pub last_hit_at: Option<DateTime<Utc>>,
    pub hit_count: u32,
}
```

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| RM4-AC-1 | `test_confidence_099_rejected` | confidence >= 0.95 → GateRejected |
| RM4-AC-2 | `test_confidence_below_06_rejected` | confidence < 0.6 → GateRejected |
| RM4-AC-3 | `test_single_evidence_rejected` | evidence_event_ids.len() < 2 → GateRejected |
| RM4-AC-4 | `test_contradiction_scan_rejects_cosine_0_85` | Dup semantica contra store existente → reject |
| RM4-AC-5 | `test_provenance_hash_locked_on_write` | evidence_hashes gravados; prefetch valida existencia |
| RM4-AC-6 | `test_semantic_dedup_by_normalized_lesson` | Fingerprint `hash(normalize(lesson))` previne dup |
| RM4-AC-7 | `test_new_lesson_starts_quarantine` | status = Quarantine |
| RM4-AC-8 | `test_promote_after_7d_and_1_hit` | Quarantine → Confirmed apos periodo + recall |
| RM4-AC-9 | `test_jsonl_roundtrip` | serde preserva |
| RM4-AC-10 (integration) | `test_repeated_error_pattern_generates_lesson` | `FailurePattern::RepeatedSameError` (reflector.rs) → Lesson criada com source=repeated_error |

**DoD extras**: integra com `reflector.rs::classify_failure()` existente (nao detector paralelo); quarantine prefix "unverified lesson:" no prefetch; tool `wiki_expunge` para retracao auditavel. **LOC target**: ~200 (split se estourar). **Risk**: Medium. **Dependencias**: RM3a + RM2.

---

### RM5a — Wiki hash + lint (puro, zero LLM)

**Objetivo.** Hash-based incremental trigger + `wiki_lint` com schema check (namespace obrigatorio) + broken-link + cross-namespace check. Zero LLM call.

**Files**:
- `crates/theo-infra-memory/src/wiki/hash.rs`
- `crates/theo-infra-memory/src/wiki/lint.rs`
- `crates/theo-domain/src/memory/wiki_backend.rs` (trait `MemoryWikiBackend`)
- `crates/theo-infra-memory/tests/wiki_lint.rs` (novo)

**Files structure em disco**:
```
.theo/wiki/memory/
  .hashes.json            # sha256 map por source
  .metadata.json          # last_compile_timestamp
  concepts/*.md           # compiled pages (namespace: memory)
  reflections/*.md        # compiled from lessons
  journal/*.jsonl         # raw events
```

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| RM5a-AC-1 | `test_unchanged_source_skip_recompile` | Hash manifest detecta dirty; unchanged = zero trabalho |
| RM5a-AC-2 | `test_dirty_source_marks_for_recompile` | SHA muda → marker em `.hashes.json` |
| RM5a-AC-3 | `test_lint_rejects_page_without_namespace` | Frontmatter sem `namespace:` → LintError |
| RM5a-AC-4 | `test_lint_detects_broken_link` | `[[nonexistent]]` → LintError |
| RM5a-AC-5 | `test_lint_cross_namespace_link_resolves` | `[[code:theo-domain]]` OK se code wiki existe |
| RM5a-AC-6 | `test_memory_wiki_mount_isolated_from_code_wiki` | Dois paths fisicamente separados |

**DoD extras**: `MemoryWikiBackend` trait separado de `WikiBackend` existente (nao reutilizar `WikiInsightInput`); 0 LLM calls em RM5a. **LOC target**: ~120. **Risk**: Low (puro). **Dependencias**: RM4 (lessons alimentam reflections page gen em 5b).

---

### RM5b — Wiki compiler com MockLLM (determinismo obrigatorio)

**Objetivo.** Compiler 2-phase (extract paralelo + generate sequencial) usando routing layer `Compaction` (temperatura 0 + seed fixa). Hard limits: `max_llm_calls_per_compile`, `max_cost_usd_per_compile`.

**Files**:
- `crates/theo-infra-memory/src/wiki/compiler.rs`
- `crates/theo-infra-memory/tests/wiki_compiler.rs` (com MockCompilerLLM)
- `crates/theo-test-memory-fixtures/src/mock_llm.rs` (novo crate)

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| RM5b-AC-1 | `test_two_compilations_produce_byte_identical_output` | Determinismo: temp=0 + seed → identico |
| RM5b-AC-2 | `test_parallel_extract_phase_within_budget` | N sources paralelo via `tokio::spawn`; generate sequencial |
| RM5b-AC-3 | `test_max_llm_calls_hard_limit_enforced` | Estoura limite → `CompileFailed { reason: "budget" }` |
| RM5b-AC-4 | `test_max_cost_hard_limit_enforced` | Excede $ cap → abort com typed error |
| RM5b-AC-5 | `test_frontmatter_contract_enforced` | Output tem `source_events`, `evidence`, `confidence`, `schema_version` |
| RM5b-AC-6 | `test_kill_switch_blocks_compile` | `WIKI_COMPILE_ENABLED=false` → early return sem calls |
| RM5b-AC-7 | `test_cache_hit_rate_above_80_percent_on_second_compile` | Rerun sem mudanca → >= 80% hash-skip |
| RM5b-AC-8 (integration) | `test_wiki_compile_tool_injects_manifest_in_system_prompt` | Tool invocado → next turn ve manifest no system prompt |

**DoD extras**: routing layer `Compaction` role; `mpsc::Sender<MemoryError>` para logging nao-bloqueante; ADR de determinismo; integracao com `theo-test-memory-fixtures`. **LOC target**: ~200. **Risk**: High (LLM, custo, determinismo). **Dependencias**: RM5a + `theo-test-memory-fixtures` crate.

---

### UI — theo-desktop memory routes (paralelo com RM3a+)

**Objetivo.** 3 rotas MVP em `theo-desktop` para recovery de memory poisoning.

**Files**:
- `apps/theo-desktop/src/commands/memory.rs` (Tauri commands)
- `apps/theo-ui/src/routes/memory/episodes.tsx` (novo)
- `apps/theo-ui/src/routes/memory/wiki.tsx` (novo)
- `apps/theo-ui/src/routes/memory/settings.tsx` (novo)

**Tauri commands**:
- `get_episodes(limit, offset) -> Vec<EpisodeSummary>`
- `dismiss_episode(id) -> Result<(), MemoryError>`
- `list_wiki_pages() -> Vec<WikiPageMeta>`
- `get_wiki_page(slug) -> String`
- `run_wiki_lint() -> Vec<LintIssue>`
- `trigger_wiki_compile() -> Result<(), MemoryError>`
- `get_memory_settings() -> MemorySettings`
- `save_memory_settings(settings)`

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| UI-AC-1 | `episodes.spec.tsx` | Timeline render com cards ordenados por data |
| UI-AC-2 | `episodes.spec.tsx` | Botao "Dismiss" chama `dismiss_episode` Tauri command |
| UI-AC-3 | `wiki.spec.tsx` | Grid de pages com markdown syntax highlight |
| UI-AC-4 | `wiki.spec.tsx` | `[[links]]` clicaveis navegam entre pages |
| UI-AC-5 | `settings.spec.tsx` | 3 secoes: Retention, Forgetting, Privacy |
| UI-AC-6 | `settings.spec.tsx` | Save persiste via `save_memory_settings` |
| UI-AC-7 (Tauri) | `test_get_episodes_command` | Rust side retorna `Vec<EpisodeSummary>` valido |
| UI-AC-8 (Tauri) | `test_trigger_wiki_compile_respects_kill_switch` | Kill switch honrado na UI tambem |

**DoD extras**: 3 rotas testadas com vitest; 2 Tauri commands testados em Rust; privacy checkbox respeita `.gitignore` policy. **LOC target**: ~200 (React) + ~80 (Rust Tauri). **Risk**: Medium (UX). **Dependencias**: RM3a (pode comecar). `/wiki` fica com placeholder ate RM5b.

---

### Test infra — `theo-test-memory-fixtures`

**Objetivo.** Crate test-only com mocks deterministicos para RM2/RM5.

**Files**:
- `crates/theo-test-memory-fixtures/Cargo.toml` (novo)
- `crates/theo-test-memory-fixtures/src/lib.rs`
- Exports: `MockCodeGraph::three_communities()`, `MockCompilerLLM::with_response(json)`, `MockRetrievalEngine::scored(entries)`

**DoD**: crate em `[dev-dependencies]` de RM2 e RM5b; nao entra em producao. **LOC target**: ~150. **Risk**: Very low. **Dependencias**: nenhuma.

---

### Health monitoring — `theo memory lint`

**Objetivo.** Comando CLI que reporta saude da memoria. 6 metricas continuas.

**Files**:
- `apps/theo-cli/src/commands/memory_lint.rs`
- `crates/theo-infra-memory/src/lint.rs` (core logic)

**Acceptance criteria**:
| ID | Teste | Comportamento |
|---|---|---|
| LINT-AC-1 | `test_wiki_staleness_detected` | Fixture com manifest > 2h → warning |
| LINT-AC-2 | `test_reflection_zero_hit_flagged` | Reflection criada 30d+ com 0 hits → concern |
| LINT-AC-3 | `test_orphan_episode_reported` | Episode sem link/reflection → info |
| LINT-AC-4 | `test_broken_link_in_wiki_page_flagged` | `[[nonexistent]]` → warning |
| LINT-AC-5 | `test_recall_p50_exceeds_500ms_flagged` | Simula latencia alta → warning |
| LINT-AC-6 | `test_json_output_parseable_by_jq` | `theo memory lint --output json \| jq .` passa |

**DoD extras**: 4 severity levels (critical/warning/concern/info); integra com `MemoryProvider::on_metrics()` hook (RM0); weekly cron opcional. **LOC target**: ~180. **Risk**: Low. **Dependencias**: RM5b.

---

## 4. Dependency graph

```
RM-pre (5 itens, paralelo)
   |
   v
RM0 (wiring) ─────────────┐
   |                       |
   v                       |
RM1 (MemoryEngine) ────────┤
   |                       |
   v                       v
RM3a (Builtin) ────────> UI (paralelo, /wiki placeholder)
   |                       
   v                       
RM2 (retrieval) ──────┐    
   |                  |    
   v                  v    
RM4 (MemoryLesson) ──┤    
   |                  |    
   v                  v    
RM5a (hash + lint) ──┤    
   |                  |    
   v                  v    
RM5b (compiler) ─────┴───> Lint tool (health)
```

**Paralelizaveis**:
- RM-pre (5 itens entre si)
- UI comeca apos RM3a (sem esperar RM5)
- `theo-test-memory-fixtures` antes de RM2

---

## 5. Rollout timeline

Assumindo 1 fase ≈ 1 sprint (1 semana de foco, pre-reqs menor):

```
Sprint 0 (2-3 dias)   RM-pre (todos 5 em paralelo)
Sprint 1              RM0 (wiring)
Sprint 2              RM1 (MemoryEngine)
Sprint 3              RM3a (Builtin + security) + inicio UI
Sprint 4              RM2 (retrieval) + UI (/episodes /settings prontos)
Sprint 5              RM4 (MemoryLesson + 7 gates)
Sprint 6              RM5a (hash + lint)
Sprint 7              RM5b (compiler com MockLLM) + UI /wiki real
Sprint 8              Lint tool (health monitoring) + final docs
```

**Gate entre sprints**: ao final de cada, `cargo test --workspace` verde + score harness >= 75.150. Sem upgrade → back para IMPLEMENT.

---

## 6. O que este plano NAO cobre

- **Fine-tuning continuo** (Karpathy end-game). Requer pipeline de training, dataset curation, model deployment — fora de escopo MVP.
- **Multi-user/multi-tenant sharing** de memoria. Single-user assumed. Sharing requer policy engine + consent model separado.
- **A-MEM Zettelkasten** (arXiv:2502.12110). Research-agent sinalizou como omissao. Opcional RM7.
- **Graphiti temporal KG integration**. Overlay tem cross-ref basico (stable IDs); full temporal graph fica para RM7+.
- **Learned classifier para quando compilar wiki**. Hoje: hash-based dirty flag. Future: ML-based trigger.
- **Budget hard enforcement global**. Router reporta; enforcement por feature dedicada.
- **Memory export / backup automatico**. Usuario pode `git commit .theo/wiki/memory/` se quiser; tooling dedicado fica para 2a onda.
- **Encryption at rest** para memoria. Privacy scope MVP e filesystem + gitignore. Encryption = RM8+.

---

## 7. Ready-to-execute checklist

Antes de comecar **RM-pre**:

- [ ] Ata da reuniao `.claude/meetings/20260420-134446-agent-memory-sota.md` merge-ready.
- [ ] Research `outputs/agent-memory-sota.md` referenciavel.
- [ ] Branch `evolution/memory-pre` criado de `develop`.
- [ ] Baseline snapshotted: harness score, test count, warnings count.
- [ ] Secrets scan confirmado (nenhuma key em `.theo/memory/` historica).

Antes de comecar **RM0**:

- [ ] Todos 5 pre-reqs merged em `develop`.
- [ ] ADR 008 aprovado por arch-validator.
- [ ] `theo-test-memory-fixtures` crate scaffolded.

Antes de ativar **`memory_enabled=true` em producao**:

- [ ] RM5b terminado.
- [ ] Lint tool funcional com 6 metricas.
- [ ] UI 3 rotas disponiveis.
- [ ] Quarantine period 7d validado em fixture.
- [ ] Kill switches testados (WIKI_COMPILE_ENABLED=false bloqueia).
- [ ] Release notes com privacy disclosure.

---

## 8. Traceability — fase → decisao da ata

| Fase | Decisoes ata cobertas |
|---|---|
| RM-pre-1 (.gitignore) | #1, #2, blocker knowledge-compiler |
| RM-pre-2 (MemoryError) | #8, concern code-reviewer |
| RM-pre-3 (fix unwrap) | #8, concern code-reviewer |
| RM-pre-4 (ADR) | #1, #2, concern arch-validator |
| RM-pre-5 (feature flag) | #1, #2 |
| RM0 | #10, #16 (sync_turn hooks expandidos) |
| RM1 | #8, #16, concern code-reviewer (RwLock, atomic write) |
| RM3a | #8, #11, #18, #19 (user-hash path) |
| RM2 | #12, concern retrieval-engineer (threshold por tipo) |
| RM4 | #3 (rename), #4 (7 gates), #16 (integrar reflector.rs), #20 (wiki_expunge) |
| RM5a | #5 (schema), #6 (dois mounts), #7 (unidirecional) |
| RM5b | #9 (determinismo), #18 (kill switch + hard limits), concern memory-synthesizer (paralelizar extract) |
| UI | #13 (3 rotas MVP apos RM3a) |
| Lint tool | #14 (6 metricas + `theo memory lint`) |
| `theo-test-memory-fixtures` | #15 (test infra) |

Cada decisao da ata tem pelo menos uma fase que a implementa. Cada fase tem pelo menos uma decisao que a justifica.
