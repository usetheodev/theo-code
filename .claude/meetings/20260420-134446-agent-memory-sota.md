---
id: 20260420-134446
date: 2026-04-20
topic: "Sistema de Memoria de Agente nivel SOTA"
verdict: REVISED
participants: 16
base_research: outputs/agent-memory-sota.md
---

# Reuniao: Sistema de Memoria de Agente nivel SOTA

## Pauta

**Contexto.** Research report em `outputs/agent-memory-sota.md` (3800 palavras) mapeou SOTA (Karpathy Wiki + CoALA + MemGPT + Zep + Mem0 + MemoryBank) e produziu gap matrix e roadmap RM0-RM5 + RM6 opcional. Estado atual: `theo-domain` ja tem `memory.rs` (`MemoryProvider` trait), `session_summary.rs`, `working_set.rs`, `episode.rs`. Gaps centrais: coordenador `MemoryEngine` (sem casa), Reflection + Meta-Memory (zero tipos), Karpathy Wiki (nao existe), wiring do `MemoryProvider` no `agent_loop.rs` (infra pronta, nao invocada).

**Questoes a decidir.**
1. O roadmap RM0-RM5 esta corretamente sequenciado?
2. Reflection como novo tipo colide com `Reflection` ja existente no evolution-loop?
3. Karpathy Wiki deve compartilhar diretorio com o Code Wiki existente?
4. MemoryEngine suporta concorrencia de forma segura (writes vs prefetch)?
5. Novo crate `theo-infra-memory` justificavel? Precisa ADR?
6. Quais safeguards obrigatorios antes de ativar self-evolving wiki em producao?
7. Qual surface de UI no `theo-desktop` para recovery de memory poisoning?
8. Como os testes de cada fase sao factiveis em TDD RED-GREEN?

**Restricoes.** `theo-domain → (nothing)` inviolavel; cada fase ≤ 200 LOC; pre-commit sem `--no-verify`; TDD obrigatorio; zero `Co-Authored-By`.

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|---|---|---|
| chief-architect | CONCERN | RM3 e RM5 estao no limite de 200 LOC com spillover admitido → split RM3a/RM3b e RM5a/RM5b. Reordenar RM3a antes de RM2 (builtin nao tem dependencia de infra nova). Adicionar `RM-pre`: feature flag `agent.memory_enabled=false` default. RM4 precisa cycle-break explicito (`source != "reflection"`). RM5 precisa hard limits codificados (`max_llm_calls_per_compile`, `max_cost_usd_per_compile`) + cache_hit_rate como AC. |
| evolution-agent | CONCERN | **Colisao de nome critica**: `theo-domain::evolution::Reflection` ja existe (retry-with-reflection intra-task, 117 LOC + 515 LOC em agent-runtime). Renomear o novo tipo para `MemoryReflection` ou `Lesson`. Ha coexistencia valida: retry-reflection alimenta memory-reflection via `on_session_end`. Feedback loop toxico sem quarentena: two-stage write-gate (`confidence>=0.6` → `_pending/`, promocao exige 2o hit OU aprovacao humana). Provenance mandatory. Kill switch `WIKI_COMPILE_ENABLED=false` default-off em CI. |

### Conhecimento

| Agente | Posicao | Resumo |
|---|---|---|
| knowledge-compiler | APPROVE com 2 blockers | Markdown + SHA-256 incremental correto; YAML frontmatter suficiente. **Blocker 1**: `.theo/` NAO esta gitignored (.gitignore:298 so ignora evolution_log.jsonl). Antes de RM3/RM5: entrada em .gitignore. **Blocker 2**: collision de slugs code/memory wiki → `namespace: memory\|code` obrigatorio no frontmatter; rejeitar sem namespace no `wiki_lint`. Schema proposto: `kind`, `slug`, `schema_version`, `sources[{path,sha256,section}]`, `confidence`, `backlinks[]` (gerados, nao manuais). |
| ontology-manager | CONCERN | **Inconsistencia pre-implementacao**: topic fala em 6 tipos, tabela tem 8. LTM-procedural colapsa em "wiki/procedures/" — nao e tipo, e path de arquivo. Reflection vs Episodic vao fundir sem consumer distinto. Meta-Memory como nome colide com o que engenheiro espera em `AgentConfig`; renomear `MemoryCoordinator`. Usar nomes canonicos Rust: `WorkingSet` (nao "WM"), `EpisodeSummary` (nao "episodic memory"). Nao criar 8 tipos antes de RM0 estar green. |
| data-ingestor | APPROVE com CONCERN | `sync_turn(user, assistant)` cobre 60% — **perde tool outputs** (episodic real e 4-turn: user/tool_call/tool_result/assistant). Precisa `on_tool_result` hook OU `sync_turn(turn: &CompletedTurn)`. Artifacts (patches, plans) em `on_convergence` apos task fechar. Batch ingest e write path separada (nao via sync_turn). **Sem idempotencia = corrupcao em retry**: chave `SHA256(session_id + turn_index + user_hash + assistant_hash)` obrigatoria, semantica upsert. Falhas no ingestor batch DEVEM propagar erro (nao silenciar); so `prefetch` pode ser silencioso. Contrato unificado `source_turn_index, source_session_id, source_checksum, written_at, writer: MemoryWriterKind`. |
| wiki-expert | CONCERN | **Dois mounts com namespaces no disco**: `.theo/wiki/code/` vs `.theo/wiki/memory/`. `WikiRetrievalBackend` hoje aponta para `.theo/wiki/` sem subdir — se escrever no mesmo root, indices BM25 colapsam. Trait separado: `MemoryWikiBackend` ≠ `WikiBackend` (nao reutilizar `WikiInsightInput` que e typed para runtime execution, nao markdown Karpathy). Cross-link UNIDIRECIONAL: memory-wiki pode linkar `[[code:...]]`; code-wiki NUNCA linka memory-wiki (espelha `theo-domain → nada`). `authority_tier` visivel na UI, `authority_tier` obrigatorio em cada resultado. |

### Qualidade

| Agente | Posicao | Resumo |
|---|---|---|
| validator | CONCERN | **4 gates insuficientes** — faltam 3 gates criticos: Gate 5 contradiction scan sincrono (cosine similarity > 0.85 com reflection existente na mesma categoria → rejeitar); Gate 6 provenance hash lock (SHA-256 dos evidence events no momento da escrita; prefetch valida existencia); Gate 7 semantic dedup (fingerprint do `lesson` normalizado). **Quarantine 7 dias** (nao 30): reflections novas prefixadas "unverified reflection"; promovem apos 1+ recall hit sem contradicao. **Tool `wiki_expunge`** explicito (retracao auditavel com propagacao via backlinks). **Integrar com `reflector.rs` ja existente** (FailurePattern::RepeatedSameError eh trigger correto); nao criar deteccao paralela. **Cross-contamination**: `MEMORY.md` por usuario (`.theo/memory/<user-hash>.md`), nao por repo. |
| linter | APPROVE (com conditions) | 6 metricas continuas: (1) compilation wiki-to-recall ratio (alvo >30%), (2) reflections-per-compression (alvo 2-5), (3) reflection-hit-rate 30d (alvo >40%), (4) orphan episodes count (alvo 0), (5) wiki manifest staleness (<2h repos ativos), (6) recall p50 latency (<500ms). **Comando obrigatorio** `theo memory lint --output json|text --severity critical\|warning\|concern\|info`. Integracao: RM0 expose `MemoryProvider::on_metrics()`; RM3 tracks `last_prefetch_hit_time`; RM4 `ReflectionStore::query_stale(30d)`; RM5 stores `last_compile_timestamp` em `.theo/wiki/.metadata.json`. Cadencia: cheap check em session transition + weekly cron. |
| retrieval-engineer | APPROVE com CONCERN | **Namespace logico, nao fisico** — mesma Tantivy instance + campo `source_type: "code"\|"wiki"\|"reflection"\|"episodic"` para filtro em query. Indice separado duplica overhead. **Thresholds por tipo** (nao 0.5 cego): `code: 0.35`, `wiki: 0.50`, `reflection: 0.60`. BM25F weights diferentes por namespace (`title 3x, content 1x` para memoria; nao reusar `filename 5x, symbol 3x`). **Budget 15% do total** reservado para memoria (ex: 3k em 20k de context); memoria nunca canibaliza codigo. **Sem cross-encoder reranker** em memoria (corpus pequeno; corta 80% da latencia). Embedding Jina Code ruim para linguagem natural → confiar em BM25 para reflections. |
| memory-synthesizer | APPROVE | Compile = minha funcao externalizada em tool. Tres decisoes validas: hash-based incremental (zero calls se hash igual), frontmatter obrigatorio (`source_events`, `evidence`, `confidence`, `schema_version`), routing layer `Compaction`. **Risco subestimado no report**: nao-determinismo — temperatura 0 + seed fixa obrigatorios; teste RED que 2 compilacoes identicas produzem output byte-igual. **Paralelismo**: fase extract paralelizavel por arquivo (`tokio::spawn` por source); fase generate sequencial por dependencia. Bottleneck hoje e latencia (20 sources = 40 LLM calls em serie). |

### Engineering

| Agente | Posicao | Resumo |
|---|---|---|
| code-reviewer | CONCERN | **3 criticals**: (1) `unwrap()` pre-existente em `run_engine.rs:786` herdado por qualquer MemoryEngine; corrigir antes de RM0. (2) `std::sync::RwLock` em task tokio = deadlock; obrigatorio `tokio::sync::RwLock` em TODAS as `BuiltinMemoryProvider`/`ReflectionStore`. (3) Atomic write (temp + rename) para todos markdown; util compartilhada `theo-infra-memory::fs_util::atomic_write`, nao reimplementar por provider. **Pre-req RM0**: `MemoryError` typed em `theo-domain` com `thiserror` (`StoreFailed`, `CompileFailed`, `RetrieveFailed`). **RM5**: `mpsc::Sender<MemoryError>` injetado no compiler (fire-and-forget nao perde erro). |
| graphctx-expert | APPROVE com CONCERN | **NAO integrar memory-graph como subgrafo do code-graph** — tres razoes: (1) code-graph e read-only/deterministic, memory e write-heavy/conversational; (2) rebuild joint forca reindex desnecessario; (3) RRF 3-ranker calibrado para codigo (Hit@5=0.97) — mixing corrompe weights. **Overlay separado** com cross-ref via stable IDs (`symbol_ref: "crates/.../lib.rs::rrf_fuse"` como campo opaco). Evento `GraphRebuilt { removed_ids }` marca refs como `unverified` (lazy validation, nao blocking). Test RED obrigatorio: `test_memory_cross_ref_marked_unverified_after_symbol_removal`. |
| arch-validator | APPROVE | **CLAUDE.md diz 11 crates**; novo `theo-infra-memory` seria 12a. **Justificativa necessaria em ADR**: simetrica a `theo-infra-llm`/`theo-infra-auth` (adapters para servicos externos — memoria como persistencia/retrieval). Se rejeitar novo crate, fallback: `theo-application::memory` (com risco de bloat da application). Feature gate obrigatorio: `[features] memory-retrieval = ["theo-engine-retrieval"]` default-off. **RM1 precisa teste RED explicito de panic isolation** (comportamento claimed sem prova hoje). Toda fase deve ter teste por AC. |
| test-runner | APPROVE com recomendacoes criticas | **RM0-4 puro RED-GREEN** (zero LLM, zero flakiness). **RM2 depende de fixture** — reutilizar `three_community_fixture()` ja existente em `theo-engine-retrieval/tests/test_search.rs:13-60`. **RM5 split em RM5a (hash/lint, puro, 100 LOC) + RM5b (compiler com MockLLM, 150 LOC)**. **Criar crate `theo-test-memory-fixtures`** (optional, test-only) com `MockCodeGraph`, `MockCompilerLLM`, `MockRetrievalEngine` deterministicos. **42 unit tests + 6 integration tests** distribuidos RM0(6)+RM1(8)+RM2(6)+RM3(8)+RM4(8)+RM5(8). Security scan (RM3) — `security_test_cases.toml` com patterns + test que itera todos. Test-to-code ratio target 0.67+ em theo-infra-memory. |
| frontend-dev | APPROVE com CONCERN | **Plano e cego ao theo-desktop** — palavra "UI" zero ocorrencias. Sem surface: usuario nao ve/corrige/expurga fatos. 3 rotas MVP: **`/memory/episodes`** (timeline EpisodeSummary + MemoryReflection, badge categoria, barra confidence, acao delete/mark-disputed); **`/memory/wiki`** (grid pages compiladas, render markdown com syntax highlight do Code Wiki, [[links]] clicaveis, trigger manual compile/lint); **`/memory/settings`** (3 secoes: Retention sliders 1-90d, Forgetting toggle MemoryBank/TTL-fixo, Privacy checkbox "sync wiki to git" — resolve open question). Tauri commands: `get_episodes`, `dismiss_episode`, `list_wiki_pages`, `run_wiki_lint`, `trigger_wiki_compile`, `get/save_memory_settings`. Sequenciamento: UI comeca **apos RM3** (nao RM5); `/wiki` vira placeholder ate RM5. |

### Pesquisa

| Agente | Posicao | Resumo |
|---|---|---|
| research-agent (self-critique) | CONCERN | Maior blocker nao e gap da matriz — e **falta de auditoria de wiring** de WM/LTM-episodic. `episode.rs` e `working_set.rs` existem; assumi "provavelmente wired", mas sem grep em `agent_loop.rs` por call-sites. Se nao wired, RM0 fica RM0+RM0.5+RM0.75. Omissao: **A-MEM (Zhong et al., arXiv:2502.12110, 2025)** — SOTA em Zettelkasten-style self-organizing memory, diretamente concorrente ao padrao Karpathy Wiki proposto. Subestimei custo de manutencao do wiki compilado (nao e so $; reprocessamento de linkage cruzada em 6 meses vira minutos, nao segundos). **Reordenacao critica**: RM3 ANTES de RM5 — RM3 resolve 80% do valor percebido ("agente lembra meu nome/preferencias") com 10% da complexidade; security scan de RM3 e pre-req de QUALQUER write path, portar depois vira retrabalho. |

---

## Conflitos

### C1. Indice retrieval: fisico separado vs namespace logico

- **wiki-expert**: dois mounts fisicamente separados no disco.
- **retrieval-engineer**: namespace logico na mesma Tantivy instance.
- **Resolucao**: coexistem. Disco fisicamente separado (paths `.theo/wiki/code/` vs `.theo/wiki/memory/`) resolve confusao de provenance e policy de backup. Indice Tantivy logico compartilhado com filter `source_type` resolve latencia. Implementacao: um `WikiRetrievalBackend` aponta para dois mounts, mas escreve no mesmo Tantivy index anotando o `source_type`. Ganha os dois lados.

### C2. Reflection tem consumer?

- **ontology-manager**: Reflection risca virar campo extra em EpisodeSummary se nao tiver reader distinto.
- **test-runner / chief-architect**: assumem consumer, nao nomeiam.
- **validator**: integrar com `reflector.rs::classify_failure()` existente — ai esta o consumer.
- **Resolucao**: consumer concreto e `WikiCompiler` (RM5) que le `reflections.jsonl` durante compilacao, gera paginas em `wiki/memory/reflections/*.md`, e `prefetch` da wiki injeta no system prompt. Segundo consumer: `AgentLoop` consulta reflections via `MemoryEngine::prefetch()` para guidance em turno novo. Reflection tem dois consumers — nao e dead code.

### C3. Ordem RM3 vs RM5 (Wiki antes do Builtin?)

- **Report original**: RM5 apos RM4 (sequencial natural).
- **research-agent (self)**: RM3 ANTES de RM5 — 80/10 value.
- **chief-architect**: reorderar RM3a antes de RM2.
- **Resolucao**: consenso forte para reordenar. **Nova ordem**: RM0 → RM1 → RM3a (Builtin + security) → RM2 (retrieval integration) → RM4 (Reflection + gating) → RM5a (wiki hash+lint puro) → RM5b (compiler MockLLM) → RM3b/RM6 opcional.

### C4. Novo crate vs application::memory

- **arch-validator**: ADR obrigatorio para 12a crate; fallback em application.
- **Sem opositores**, mas test-runner recomenda `theo-test-memory-fixtures` como crate separado (test-only, nao contabiliza como workspace member producao).
- **Resolucao**: criar `theo-infra-memory` COM ADR explicito citando simetria com `theo-infra-llm`/`theo-infra-auth`; criar `theo-test-memory-fixtures` como crate test-only.

### C5. Nome do tipo Reflection

- **evolution-agent**: colide com `theo-domain::evolution::Reflection` existente.
- **ontology-manager**: aceita qualquer nome exceto "Meta-Memory".
- **validator**: neutro.
- **Resolucao**: renomear o novo tipo para `MemoryLesson` (termo usado no research report §5) ou `Lesson`. Arquivo: `theo-domain/src/memory/lesson.rs`. `Reflection` existente fica intacto.

---

## Decisoes

1. **Reordenacao do roadmap**: RM0 → RM1 → RM3a → RM2 → RM4 → RM5a → RM5b → (RM3b/RM6 opcional).
2. **Pre-requisitos antes de RM0**:
   - (a) `.theo/` adicionado ao `.gitignore` com excecoes explicitas (`!.theo/fixtures/`, `!.theo/wiki/code/`).
   - (b) `MemoryError` typed enum em `theo-domain/src/memory/error.rs`.
   - (c) Corrigir `unwrap()` pre-existente em `run_engine.rs:786`.
   - (d) ADR curto em `docs/adr/` justificando `theo-infra-memory` como 12a crate.
   - (e) Feature flag `agent.memory_enabled=false` default no `AgentConfig`.
3. **Renomear tipo novo**: `MemoryLesson` (ou `Lesson`) em vez de "Reflection". Namespace: `theo-domain::memory::lesson`.
4. **Gates de write em MemoryLesson**: 7 (nao 4). Adicionar contradiction scan, provenance hash lock, semantic dedup. Quarantine 7d (nao 30d). `status: quarantine\|confirmed\|retracted`.
5. **Karpathy Wiki schema** (frontmatter obrigatorio): `namespace: memory\|code`, `kind`, `slug`, `schema_version`, `sources[{path,sha256,section}]`, `confidence`, `created_at`, `updated_at`, `backlinks[]`, `evidence_event_ids[]`.
6. **Dois mounts fisicos, indice logico compartilhado**: `.theo/wiki/code/` e `.theo/wiki/memory/` no disco; Tantivy com `source_type` field.
7. **Cross-link unidirecional**: memory→code permitido (`[[code:symbol]]`); code→memory proibido.
8. **Concorrencia**: `tokio::sync::RwLock` em TODAS as impls que fazem write; atomic write via temp+rename utility compartilhado.
9. **Determinismo no compiler**: temperatura 0 + seed fixa; teste RED de byte-equality entre 2 compilacoes.
10. **Ingest hooks expandidos**: `sync_turn(turn: &CompletedTurn)` com tool_call+tool_result; `on_convergence` para artifacts; `on_tool_result` opcional para episodic fine-grained.
11. **Idempotencia obrigatoria**: SHA256 key em todo write; upsert semantics.
12. **Retrieval**: namespace logico em Tantivy; thresholds por tipo (code 0.35, wiki 0.50, reflection 0.60); memory_token_budget = 15% do total; sem cross-encoder reranker em memoria.
13. **UI theo-desktop**: 3 rotas MVP comecando apos RM3 — `/memory/episodes`, `/memory/wiki`, `/memory/settings`.
14. **Health monitoring**: comando `theo memory lint` com 6 metricas (wiki-to-recall, reflections-per-compression, reflection-hit-rate, orphans, manifest staleness, recall p50).
15. **Test infrastructure**: crate `theo-test-memory-fixtures` (test-only) com `MockCodeGraph`, `MockCompilerLLM`, `MockRetrievalEngine`. 42 unit + 6 integration tests distribuidos por fase.
16. **Integrar com existente**: `reflector.rs::classify_failure()` alimenta `MemoryLesson`; nao criar deteccao paralela. `EpisodeSummary`/`WorkingSet` sao os nomes canonicos Rust (usar em docs/issues/PRs).
17. **Memory graph como overlay separado**: NAO integrar com code-graph; cross-ref via stable IDs opaco; evento `GraphRebuilt` marca refs `unverified`.
18. **Kill switch**: `WIKI_COMPILE_ENABLED=false` default em CI/benchmark; `max_llm_calls_per_compile` e `max_cost_usd_per_compile` como hard limits codificados.
19. **Cross-contamination**: `.theo/memory/<user-hash>.md` em vez de `MEMORY.md` global (multi-user safety).
20. **Tool `wiki_expunge`**: retracao auditavel com propagacao via backlinks.

---

## Action Items

### Pre-requisitos (antes de RM0)

- [ ] **knowledge-compiler** — adicionar `.theo/` ao `.gitignore` com excecoes — antes de qualquer commit de RM
  - **Plano TDD**: RED `test_gitignore_excludes_theo_memory` → GREEN atualizar `.gitignore` → VERIFY `git check-ignore .theo/memory/test.md`
- [ ] **code-reviewer** — adicionar `MemoryError` enum em `theo-domain/src/memory/error.rs` — pre-req de RM0
  - **Plano TDD**: RED `test_memory_error_variants_roundtrip_serde` → GREEN impl thiserror → REFACTOR compartilhar com callers → VERIFY `cargo test -p theo-domain`
- [ ] **code-reviewer** — corrigir `unwrap()` em `run_engine.rs:786` — pre-req de RM0
  - **Plano TDD**: RED `test_llm_result_none_returns_typed_error` → GREEN `llm_result.ok_or(...)?` → VERIFY `cargo test -p theo-agent-runtime`
- [ ] **arch-validator** — escrever ADR `docs/adr/008-theo-infra-memory.md` justificando 12a crate — antes de RM1
- [ ] **chief-architect** — adicionar `agent.memory_enabled: bool = false` em `AgentConfig` — antes de RM0
  - **Plano TDD**: RED `test_memory_flag_default_false` → GREEN adicionar campo → VERIFY

### RM0 — Wire MemoryProvider no agent_loop

- [ ] **chief-architect + code-reviewer** — wire 4 hooks (`prefetch` antes LLM, `sync_turn` depois, `on_pre_compress` no callback, `on_session_end` ao fechar) — 1 sprint
  - **Plano TDD**: RED integration test `test_memory_hooks_invoked_in_order` com `MockProvider` que grava sequence → GREEN adicionar chamadas em `agent_loop.rs` gated por feature flag → REFACTOR extrair `MemoryLifecycle` helper → VERIFY `cargo test -p theo-agent-runtime --test memory_integration_rm0`
  - **ACs**: 6 unit + 1 integration (vide test-runner §4)

### RM1 — MemoryEngine coordinator

- [ ] **arch-validator + code-reviewer** — criar `theo-infra-memory` crate; `MemoryEngine` em `theo-application` orquestrando providers
  - **Plano TDD**: RED `test_panicking_provider_does_not_block_fanout` + `test_only_one_external_provider_allowed` → GREEN port `hermes memory_manager.py:97-121` → VERIFY
  - **ACs**: 8 unit + 1 integration com panic isolation

### RM3a — BuiltinMemoryProvider + security (reordenado: antes de RM2)

- [ ] **code-reviewer + validator** — `.theo/memory/<user-hash>.md` com `tokio::sync::RwLock` + atomic write; security scan port de `memory_tool.py:65-103`
  - **Plano TDD**: RED 4 injection patterns + concurrent write + snapshot stable → GREEN impl → VERIFY
  - **ACs**: 8 unit + 1 integration (MEMORY.md written + recalled across sessions)
- [ ] **data-ingestor** — `sync_turn(turn: &CompletedTurn)` com idempotency key SHA256 + source metadata contract
  - **Plano TDD**: RED `test_retry_does_not_duplicate_entry` → GREEN upsert semantics → VERIFY

### RM2 — RetrievalBackedMemory

- [ ] **retrieval-engineer** — namespace `source_type` em FileTantivyIndex; thresholds por tipo; memory_token_budget 15%
  - **Plano TDD**: RED usando `three_community_fixture()` existente de `test_search.rs` → GREEN adapter `RetrievalBackedMemory` → VERIFY
  - **ACs**: 6 unit + 1 integration

### RM4 — MemoryLesson (renomeado de Reflection) + gates

- [ ] **validator + memory-synthesizer** — 7 gates (confidence bounds, evidence count, contradiction scan, provenance hash, semantic dedup, quarantine 7d, aging decay); `status: quarantine\|confirmed\|retracted`
  - **Plano TDD**: RED 3 puros (confidence 0.99 rejected, single evidence rejected, contradiction rejected) + 2 gate tests → GREEN impl `LessonGate` → VERIFY
  - **ACs**: 8 unit + 1 integration (convergence hook → `reflections.jsonl` com gates aplicados)
- [ ] **validator** — integrar com `reflector.rs::classify_failure()` existente (consume `FailurePattern::RepeatedSameError` como trigger)
  - **Plano TDD**: RED `test_repeated_error_pattern_generates_lesson` → GREEN bridge → VERIFY

### RM5a — Wiki hash + lint (puro)

- [ ] **knowledge-compiler + memory-synthesizer** — hash-based incremental trigger; `wiki_lint` com schema check (namespace obrigatorio) + broken-link detection + cross-namespace check
  - **Plano TDD**: RED `test_unchanged_source_skip_llm` + `test_broken_link_flagged` → GREEN hash manifest + lint regex → VERIFY
  - **ACs**: 4 unit + 0 integration
- [ ] **wiki-expert** — `MemoryWikiBackend` trait separado em theo-domain; `.theo/wiki/memory/` mount separado
  - **Plano TDD**: RED `test_code_wiki_and_memory_wiki_resolve_to_different_paths` → GREEN impl → VERIFY

### RM5b — Wiki compiler (com MockLLM)

- [ ] **memory-synthesizer + test-runner** — compilation fase extract paralelizada por source + fase generate sequencial; temperatura 0 + seed fixa
  - **Plano TDD**: RED `test_two_compilations_produce_identical_output` (byte-equality) + `test_parallel_extract_under_budget` → GREEN impl com MockLLM → VERIFY
  - **ACs**: 4 unit + 1 integration (compile → manifest em system prompt)

### UI (paralelo, comeca apos RM3a)

- [ ] **frontend-dev** — 3 rotas theo-desktop: `/memory/episodes`, `/memory/wiki`, `/memory/settings` + Tauri commands
  - **Plano TDD**: RED vitest para rota + integration test Tauri command `get_episodes` → GREEN impl React + Tauri → VERIFY `npm test`

### Test infra

- [ ] **test-runner** — criar `crates/theo-test-memory-fixtures/` com `MockCodeGraph`, `MockCompilerLLM`, `MockRetrievalEngine`
  - **Plano TDD**: N/A (test crate por si mesmo). Garante zero flakiness em RM2/RM5.

### Health monitoring

- [ ] **linter** — comando `theo memory lint` com 6 metricas + severity levels — apos RM5
  - **Plano TDD**: RED fixture com stale wiki + zero-hit reflection → GREEN impl scanner → VERIFY

### Observabilidade

- [ ] **evolution-agent** — kill switch `WIKI_COMPILE_ENABLED=false` em CI + metrics event `memory.lesson.{stored|rejected|promoted}` + `memory.wiki.{compiled|skipped}`
  - **Plano TDD**: RED `test_kill_switch_blocks_compile` → GREEN env var check → VERIFY

### Documentacao

- [ ] **arch-validator** — ADR `docs/adr/008-theo-infra-memory.md` — antes de RM1
- [ ] **knowledge-compiler** — `docs/current/memory-architecture.md` (schema de pagina, namespace convention, backlink policy) — apos RM5a

---

## Plano TDD Consolidado

Todos action items de codigo incluem RED/GREEN/REFACTOR acima. Resumo test-runner:

| Fase | Unit | Integration | RED factivel puro? | LLM mock necessario? |
|---|---|---|---|---|
| Pre-req | 3 | 0 | ✅ | nao |
| RM0 | 6 | 1 | ✅ | nao |
| RM1 | 8 | 1 | ✅ | nao |
| RM3a | 8 | 1 | ✅ | nao |
| RM2 | 6 | 1 | ⚠️ (fixture) | nao (reusar three_community) |
| RM4 | 8 | 1 | ✅ | nao |
| RM5a | 4 | 0 | ✅ | nao |
| RM5b | 4 | 1 | ⚠️ | sim (MockCompilerLLM) |
| UI | 3+ | 2 | ✅ | nao |
| Lint | 2 | 0 | ✅ | nao |
| **TOTAL** | **52** | **8** | **8/10 puros** | **1 mock** |

Test-to-code ratio target: ≥ 0.67 em `theo-infra-memory`, ≥ 0.75 em `theo-application::memory`.

---

## Veredito Final

**REVISED**: proposta e arquiteturalmente solida mas precisa de 5 pre-requisitos (`.gitignore`, `MemoryError`, fix unwrap, ADR 12a crate, feature flag) + reordenacao (RM3a antes de RM2) + rename (`Reflection` → `MemoryLesson`) + 7 gates de write (nao 4) + namespace na wiki + UI desde RM3 + crate test-fixtures separada. Sem essas revisoes o roadmap avanca com collisions, concorrencia insegura, e sem surface de recovery.

Action items tem 14 itens com planos TDD completos; 8/10 fases sao puras (zero LLM real nos testes). Kill switches e feature flags obrigatorios antes de self-evolving em producao. Concerns de `validator` e `code-reviewer` sao bloqueantes ate serem implementados como pre-reqs; demais concerns viram ACs explicitos nas fases.

Decisoes 1-20 acima consolidam o consenso. Plano executavel em `outputs/agent-memory-plan.md` materializa essas decisoes em RM-pre, RM0-RM5b + UI, com ACs e DoDs por fase.
