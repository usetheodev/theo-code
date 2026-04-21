# Plano: Memory & State — Theo Ahead of Hermes

> **Status**: APPROVED (REVISED v2) — Meeting 20260420-221947 + review findings 20260421
> **Data**: 2026-04-20 (rev 2026-04-21)
> **Baseline honesto**: 0% do sistema de memoria esta wired em producao.
> Tipos, logica e testes existem. Nenhum hook e chamado no agent loop.
> **Ata**: `.claude/meetings/20260420-221947-memory-superiority-plan.md`

---

## Diagnostico

```
O que EXISTE (testado, passing):          O que RODA em producao:
─────────────────────────────────         ──────────────────────────
MemoryProvider trait ✓                    EpisodeSummary (write-only, never read back)
MemoryEngine fan-out ✓                   Compaction (sem memory hooks)
BuiltinMemoryProvider (atomic I/O) ✓     System prompt statico
MemoryLesson + 7 gates ✓                 .theo/theo.md context loading
MemoryLifecycleEnforcer (decay) ✓        FileMemoryStore artesanal (run_engine.rs:311-331)
EpisodeSummary + hypothesis ✓              ↑ CONFLITA com MemoryProvider formal
RetrievalBackedMemory (interface) ✓
MemoryLifecycle helpers ✓
Security scanning ✓
SessionTree (types) ✓
```

**A distancia entre "existe" e "roda" e ~100 LOC de wiring em run_engine.rs + remocao do path ad-hoc.**

---

## Criterio de Sucesso Global

| # | Criterio | Hermes hoje | Theo target |
|---|----------|-------------|-------------|
| G1 | Memory prefetch/sync rodando em producao | Sim | Sim |
| G2 | Cross-session search funcional | FTS5 + LLM summary (transcript-level) | Keyword match sobre episode summaries (structured-level). **Nota**: Hermes busca em transcripts completos; Theo busca em summaries estruturados. Sao abordagens diferentes, nao comparaveis diretamente. Theo troca cobertura por precisao (campos machine-readable). Evolution path: RRF + embeddings sobre episodes para fechar o gap de cobertura. |
| G3 | Cost/token tracking per-session | 6 campos + billing | Token tracking + cost estimation |
| G4 | Compaction com memory hooks + oversized protection | LLM summary iterativo | Structured template + pre-compress hook + per-msg cap |
| G5 | Knowledge gating (lessons) | Nenhum | 7-gate + quarantine (ja coded) |
| G6 | Failure learning automatico | Nenhum | Constraint extraction + hypothesis (ja coded) |
| G7 | Memory lifecycle decay | Nenhum | Active→Cooling→Archived (ja coded) |
| G8 | Frozen snapshot (cache stability) | Sim | Sim (OnceLock) |
| G9 | Retrieval com budget packing | Nenhum | 15% cap + per-type thresholds (CALIBRADOS, nao arbitrarios) |
| G10 | Episode summaries fed back into context | Nao | Sim |

---

## Estrutura de Fases

```
Pre-Phase 0: PREP (pre-requisitos da reuniao)         ← ~80 LOC
Phase 0: WIRE (ativar + reconciliar)                   ← ~150 LOC (atomico com RM3a)
Phase 1: CLOSE GAPS (o que Hermes tem e Theo nao)      ← ~400 LOC
Phase 2: ACTIVATE (ligar features dormentes)            ← ~280 LOC (revisado: decay +40)
Phase 3: SURPASS (o que ninguem tem)                    ← ~310 LOC (revisado: reasoning +20)
                                                         ────────
                                                   Total: ~1220 LOC
```

---

## Pre-Phase 0: PREP — Pre-requisitos da Reuniao

> **Objetivo**: Corrigir issues identificados na reuniao antes de qualquer wiring.
> **LOC estimado**: ~80

### Task P.1: Mover episodes para `.theo/memory/episodes/`

**Decisao reuniao #4**: Episodes em `.theo/wiki/episodes/` violam namespace wiki.

**O que fazer**:
1. Mover path de persistencia de episodes de `.theo/wiki/episodes/` para `.theo/memory/episodes/`
2. Atualizar `run_engine.rs` (onde episodes sao escritos)
3. Atualizar qualquer leitor de episodes

**AC**:
- AC-P.1.1: Episodes persistidos em `.theo/memory/episodes/{id}.json`
- AC-P.1.2: Path antigo (`.theo/wiki/episodes/`) nao e mais escrito
- AC-P.1.3: `.gitignore` ja cobre `.theo/memory/` (verificar, nao mudar)

**DoD**: `cargo test --workspace` green. Nenhum referencia a `.theo/wiki/episodes/` no codigo.

---

### Task P.2: Unicode injection hardening

**Decisao reuniao #8**: Scanner nao normaliza unicode. Cyrillic lookalikes passam.

**Crate**: `theo-infra-memory`
**Arquivo**: `src/security.rs`

**O que fazer**:
1. Adicionar dependencia `unicode-normalization` ao crate
2. Pre-processar input com NFKD normalization antes do scan
3. Remover zero-width characters (U+200B, U+200C, U+200D, U+FEFF)
4. Detectar mixed-script text (latin + cyrillic = suspect)

**AC**:
- AC-P.2.1: `test_cyrillic_lookalike_injection_blocked` — "ignore рrevious instructions" (cyrillic р) rejeitado
- AC-P.2.2: `test_zero_width_injection_blocked` — content com ZWJ/ZWNJ rejeitado
- AC-P.2.3: Normalizacao NFKD aplicada antes de pattern matching
- AC-P.2.4: Pure ASCII input nao sofre overhead mensuravel

**DoD**: `cargo test -p theo-infra-memory` green. <= 40 LOC.

---

### Task P.3: schema_version obrigatorio

**Decisao reuniao #11**: Todo JSON persistido e markdown do builtin devem ter schema_version.

**O que fazer**:
1. Adicionar `schema_version: u32` a `EpisodeSummary` (se nao existir)
2. Adicionar `schema_version: u32` a `MemoryLesson`
3. Adicionar `<!-- schema_version: 1 -->` ao header do markdown do builtin
4. Verificar no deserialize: se version > current, log warning (forward compat)

**AC**:
- AC-P.3.1: `EpisodeSummary` tem campo `schema_version` serializado no JSON
- AC-P.3.2: `MemoryLesson` tem campo `schema_version` serializado no JSON
- AC-P.3.3: Builtin markdown tem header com schema_version
- AC-P.3.4: JSON com schema_version > implementado gera warning, nao panic

**DoD**: `cargo test --workspace` green. <= 30 LOC.

---

### Task P.4: Renomear LessonStatus::Retracted para Invalidated

**Decisao reuniao #13**: Diferencia de `HypothesisStatus::Superseded`.

**AC**:
- AC-P.4.1: Enum variant renomeada em theo-domain
- AC-P.4.2: Todos os testes atualizados
- AC-P.4.3: Nenhuma referencia a `Retracted` no codebase

**DoD**: `cargo test --workspace` green. <= 10 LOC.

---

### Task P.5: Corrigir sidebar do desktop (Memory group)

**Decisao reuniao #12**: Paginas de memory existem mas nao aparecem na sidebar.

**Crate**: `apps/theo-ui`
**Arquivo**: `src/app/AppSidebar.tsx`

**AC**:
- AC-P.5.1: Grupo "Memory" visivel na sidebar com items: Episodes, Wiki, Settings
- AC-P.5.2: Routes `/memory/episodes`, `/memory/wiki`, `/memory/settings` acessiveis

**DoD**: UI funcional, navegacao testada manualmente. <= 20 LOC.

---

**Pre-Phase 0 DoD**:
- [ ] Episodes em `.theo/memory/episodes/`
- [ ] Unicode injection bloqueada (cyrillic + zero-width)
- [ ] schema_version em todos os JSONs e markdown
- [ ] `LessonStatus::Invalidated` (nao `Retracted`)
- [ ] Sidebar do desktop com Memory group

---

## Phase 0: WIRE — Ativar o Sistema de Memoria

> **Objetivo**: Ligar os 4 hooks no agent loop, instantiar provider, REMOVER path ad-hoc.
> **Resultado**: Agente com prefetch/sync rodando em cada turn.
> **LOC estimado**: ~150
> **NOTA reuniao #3**: Phase 0 e RM3a sao UNIDADE ATOMICA. Sem provider ativo, wiring opera sobre NullMemoryProvider.

### Task 0.1: Wire MemoryLifecycle hooks + Remover FileMemoryStore ad-hoc

**Crate**: `theo-agent-runtime`
**Arquivo**: `src/run_engine.rs`

**O que fazer**:
1. Antes de cada LLM call: chamar `MemoryLifecycle::prefetch(config, &user_query)` e injetar resultado como system message fenced
2. Apos cada LLM response completa: chamar `MemoryLifecycle::sync_turn(config, &user_msg, &assistant_msg)`
3. Antes de `compact_if_needed()` em `compaction.rs`: chamar `MemoryLifecycle::on_pre_compress(config, &messages_text)`
4. No exit do run (convergence/abort): chamar `MemoryLifecycle::on_session_end(config)`
5. **REMOVER** o path ad-hoc em `run_engine.rs:311-331` (`FileMemoryStore::for_project`) quando `memory_enabled=true`
6. Converter `record_session_exit` para `async fn` com `tokio::fs` (decisao #7)

**Acceptance Criteria**:
- AC-0.1.1: `prefetch()` chamado antes de cada `call_llm()`, resultado como system message fenced
- AC-0.1.2: `sync_turn()` chamado apos cada response completa do LLM (nao em tool calls intermediarios)
- AC-0.1.3: `on_pre_compress()` chamado antes de `compact_if_needed()`, resultado merged no summary
- AC-0.1.4: `on_session_end()` chamado em TODOS os paths de saida (converged, aborted, panic via Drop)
- AC-0.1.5: Quando `memory_enabled=false`, todos os hooks sao no-op (zero overhead)
- AC-0.1.6: **`FileMemoryStore::for_project` NAO e chamado quando `memory_enabled=true`** (teste RED: `test_no_dual_memory_injection`)
- AC-0.1.7: Teste de integracao com `RecordingProvider` verifica sequencia: prefetch→llm→sync→end
- AC-0.1.8: `record_session_exit` usa `tokio::fs`, nao `std::fs`

**Latency budget** (decisao #14):
- `prefetch` < 100ms p99 para providers locais
- `sync_turn` chamado inline (NAO fire-and-forget) — durabilidade > latencia. Se sync_turn fosse fire-and-forget via tokio::spawn, on_session_end poderia rodar antes do spawn completar em abort/panic, perdendo a ultima escrita. sync_turn e o write path e DEVE completar antes de prosseguir. Latencia de sync_turn e dominada por I/O atomico (~1ms local) — aceitavel inline.

**DoD**:
- [ ] `cargo test -p theo-agent-runtime` green
- [ ] `cargo check --workspace --tests` zero warnings
- [ ] `test_no_dual_memory_injection` passing
- [ ] Hook sequence validada com RecordingProvider
- [ ] Nenhum `unwrap()` ou `expect()` adicionado
- [ ] `std::fs` removido do hot path async

---

### Task 0.2: Instantiate MemoryEngine com BuiltinMemoryProvider

**Crate**: `theo-application`
**Arquivo**: novo factory em `src/memory.rs`

**O que fazer**:
1. Criar funcao `build_memory_engine(config: &AgentConfig) -> Option<Arc<dyn MemoryProvider>>`
2. Se `memory_enabled=true`: criar `MemoryEngine`, registrar `BuiltinMemoryProvider`, retornar Arc
3. Se `memory_enabled=false`: retornar None
4. Injetar resultado em `AgentConfig.memory_provider`

**Acceptance Criteria**:
- AC-0.2.1: `BuiltinMemoryProvider` registrado no `MemoryEngine` quando `memory_enabled=true`
- AC-0.2.2: `memory_provider` e `None` quando flag desligada
- AC-0.2.3: Arquivo `.theo/memory/{user_hash}.md` criado apos primeiro `sync_turn`
- AC-0.2.4: Security scan rejeita content com injection patterns (inclusive unicode — Task P.2)
- AC-0.2.5: Dependency direction: `theo-application → theo-infra-memory → theo-domain`
- AC-0.2.6: **Corrupcao handling para episode JSONs** (nao para builtin .md): episode JSON corrompido em `.theo/memory/episodes/` → rename para `.corrupt`, skip, emitir log warning. O `BuiltinMemoryProvider` usa .md, nao JSON — corrupcao de .md e tratada como "estado vazio" (provider inicia com Vec vazio se parse falhar em RM3b reload).

**DoD**:
- [ ] `cargo test --workspace` green
- [ ] E2E: `memory_enabled=true` → .md criado
- [ ] E2E: `memory_enabled=false` → nenhum arquivo
- [ ] E2E: JSON corrompido → `.corrupt` rename + estado vazio
- [ ] <= 60 LOC

---

### Task 0.3: Feed EpisodeSummaries back into context

**Crate**: `theo-agent-runtime`
**Arquivo**: `src/run_engine.rs` (session start)

**O que fazer**:
1. No inicio de cada run, carregar ultimos N episodes de `.theo/memory/episodes/` (path corrigido — decisao #4)
2. Filtrar por `lifecycle != Archived` e `ttl_policy` nao expirada
3. Formatar `machine_summary` como system message (objective + learned_constraints + failed_attempts)
4. Injetar como contexto antes do primeiro user message

**Acceptance Criteria**:
- AC-0.3.1: Ate 5 episode summaries mais recentes carregados
- AC-0.3.2: Summaries com `lifecycle=Archived` excluidos
- AC-0.3.3: Summaries com TTL expirado excluidos
- AC-0.3.4: `learned_constraints` aparecem no system prompt como warnings
- AC-0.3.5: `failed_attempts` de episodes anteriores visiveis ao LLM
- AC-0.3.6: Token budget: episode context limitado a 5% do context window
- AC-0.3.7: Sem episodes → nenhum system message (no-op)

**DoD**:
- [ ] `cargo test -p theo-agent-runtime` green
- [ ] Teste: 3 episodes no disco → LLM recebe constraints
- [ ] Teste: episode archived → nao aparece
- [ ] <= 80 LOC

---

**Phase 0 Definition of Done**:
- [ ] Tasks 0.1, 0.2, 0.3 todas completas
- [ ] Agent roda com `memory_enabled=true`:
  - Prefetch executa a cada turn
  - Sync persiste a cada turn (fire-and-forget)
  - .md existe no disco
  - Episodes anteriores aparecem no context
  - FileMemoryStore ad-hoc NAO ativo
- [ ] Agent roda com `memory_enabled=false` sem regressao
- [ ] **G1 atingido**: Memory prefetch/sync rodando
- [ ] **G10 atingido**: Episode summaries fed back

---

## Phase 1: CLOSE GAPS — O que Hermes Tem e Theo Nao

> **LOC estimado**: ~400
> **Depende de**: Phase 0 completa

### Task 1.1: Cost/Token Tracking Per-Session

**Crate**: `theo-domain` (types) + `theo-agent-runtime` (tracking)

**O que fazer**:
1. Adicionar `TokenUsage` struct em `theo-domain`:
   ```
   input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
   reasoning_tokens, estimated_cost_usd
   ```
2. Acumular tokens em cada LLM response no runtime
3. Persistir no episode summary (campo `token_usage: Option<TokenUsage>`)
4. CLI: exibir ao final do run: "Tokens: X in / Y out | Cost: ~$Z.ZZ"
5. Desktop: footer badge no `AgentView` (decisao #12)

**AC**:
- AC-1.1.1: `TokenUsage` struct com 6 campos em theo-domain
- AC-1.1.2: Cada LLM response acumula tokens
- AC-1.1.3: Cost estimation baseada em pricing table hardcoded por provider/model
- AC-1.1.4: CLI exibe summary ao final do run
- AC-1.1.5: TokenUsage serializado no EpisodeSummary
- AC-1.1.6: Provider sem token info → campos 0 (graceful)

**DoD**: `cargo test --workspace` green. <= 120 LOC. **G3 atingido**.

---

### Task 1.2: Frozen Snapshot Pattern

**Crate**: `theo-infra-memory`
**Arquivo**: `src/builtin.rs`

**Tradeoff explicito**: O builtin provider HOJE retorna estado in-memory fresh em cada `prefetch()` (ve writes da mesma sessao). Frozen snapshot TROCA visibilidade intra-sessao por estabilidade de prefix cache. O LLM nao vera memorias escritas mid-session ate a proxima sessao. Isso e o mesmo tradeoff que Hermes faz — deliberado, nao acidental.

**Implementacao**: Capturar snapshot no PRIMEIRO `prefetch()` da sessao. Snapshot = estado in-memory naquele momento (que inclui entries de sync_turns anteriores na mesma sessao, se houver). Writes subsequentes persistem no disco E no state in-memory, mas `prefetch()` retorna o snapshot congelado.

**Primitiva**: `std::sync::OnceLock` (decisao #7 — NAO `OnceCell`)

**O que fazer**:
1. Adicionar `snapshot: OnceLock<String>` ao `BuiltinMemoryProvider`
2. No `prefetch()`: `snapshot.get_or_init(|| self.state.read().entries.join("\n"))`
3. `sync_turn()` continua escrevendo no state + disco normalmente
4. Nova sessao (novo provider instance) → OnceLock vazio → snapshot fresh

**AC**:
- AC-1.2.1: Primeiro `prefetch()` captura estado in-memory e congela em OnceLock
- AC-1.2.2: Segundo+ `prefetch()` retorna snapshot sem ler state
- AC-1.2.3: `sync_turn()` continua persistindo (disco + state) — writes nao se perdem, apenas nao sao visiveis no prefetch da sessao corrente
- AC-1.2.4: Nova sessao → novo provider → OnceLock vazio → snapshot atualizado
- AC-1.2.5: Thread-safe via `OnceLock` (stdlib)

**DoD**: `cargo test -p theo-infra-memory` green. <= 30 LOC. **G8 atingido**.

---

### Task 1.3: Compaction com Memory Hooks + Oversized Protection

**Crate**: `theo-agent-runtime`
**Arquivo**: `src/compaction.rs`

**O que fazer**:
1. Antes de truncar, chamar `on_pre_compress()` e preservar output no summary
2. Mudar tail protection de count fixo (6) para token budget (20% do threshold)
3. Anti-thrashing: skip se < 3 turns desde ultima compaction
4. **Per-message cap de `context_window/4`** (decisao #10) — truncar mensagens oversized no tail
5. Summary template: objective + resolved questions + pending + active task

**AC**:
- AC-1.3.1: `on_pre_compress()` chamado antes de truncation
- AC-1.3.2: Output do hook incluido no compaction summary
- AC-1.3.3: Tail protection por token budget (20%), nao count fixo
- AC-1.3.4: Anti-thrashing: skip se < 3 turns desde ultima
- AC-1.3.5: **Per-message cap `context_window/4`**: mensagem individual > cap e truncada mesmo no tail
- AC-1.3.6: Teste RED: `test_single_oversized_message_does_not_cause_oom_loop`
- AC-1.3.7: Compaction idempotente

**DoD**: `cargo test -p theo-agent-runtime` green. <= 120 LOC. **G4 atingido**.

---

### Task 1.4: Cross-Session Search

**Crate**: `theo-agent-runtime` (interface) + `theo-infra-memory` (implementation)

**Decisao reuniao #5**: Keyword match direto. Migration para `MemoryRetrieval` trait como evolution item.

**O que fazer**:
1. Criar trait `SessionSearch` em theo-domain
2. Implementar com keyword match sobre episode JSONs em `.theo/memory/episodes/`:
   - Keyword match em `objective`, `key_actions`, `learned_constraints`
   - Rank por `(keyword_overlap * 0.6 + recency * 0.4)`
3. Expor como tool do agente: `search_sessions(query)` retorna top 3

**AC**:
- AC-1.4.1: `SessionSearch` trait em theo-domain
- AC-1.4.2: Implementacao le episode JSONs de `.theo/memory/episodes/`
- AC-1.4.3: Busca por keywords em objective + key_actions + learned_constraints
- AC-1.4.4: Rank por `(keyword_overlap * 0.6 + recency * 0.4)`
- AC-1.4.5: Max 3 resultados, formatados como texto estruturado
- AC-1.4.6: Performance < 50ms com 100 episodes
- AC-1.4.7: Zero resultados → "Nenhuma sessao anterior relevante"

**Evolution item**: Migrar para `MemoryRetrieval` trait com RRF quando T3.3 estiver pronto.

**DoD**: `cargo test --workspace` green. <= 150 LOC. **G2 atingido**.

---

**Phase 1 DoD**: G2, G3, G4, G8 atingidos.

---

## Phase 2: ACTIVATE — Ligar Features Dormentes

> **LOC estimado**: ~200
> **Depende de**: Phase 0 completa. PARALELA com Phase 1.

### Task 2.1: Wire Lesson Gates no Runtime

**Crate**: `theo-agent-runtime`

**O que fazer**:
1. Apos cada run com `outcome=Failure|Partial`, extrair lessons candidatas dos events
2. Passar por `apply_gates()` (ja implementado — usa `LessonStatus::Invalidated`, decisao #13)
3. Lessons aprovadas: persistir em `.theo/memory/lessons/{id}.json` (com `schema_version`)
4. No prefetch: carregar lessons `Confirmed` e injetar como constraints

**AC**:
- AC-2.1.1: Lessons candidatas geradas de `ConstraintLearned` events
- AC-2.1.2: `apply_gates()` filtra (7 gates)
- AC-2.1.3: Persistidas em `.theo/memory/lessons/{id}.json` com `schema_version`
- AC-2.1.4: `Quarantine` NAO aparece no prefetch
- AC-2.1.5: `Confirmed` aparece como constraints
- AC-2.1.6: `promote_if_ready()` chamado no prefetch (side effect)
- AC-2.1.7: Lesson com contradicao → `Invalidated`, removida do prefetch

**DoD**: `cargo test --workspace` green. <= 100 LOC. **G5 atingido**.

---

### Task 2.2: Wire Decay Enforcer no Prefetch

**Crate**: `theo-infra-memory`

**Realidade do provider atual**: `BuiltinMemoryProvider` armazena `Vec<String>` sem metadata per-entry. NAO tem `created_at`, `hit_count`, `lifecycle`. Implementar decay exige uma das seguintes abordagens:

**Opcao A (sidecar metadata)** — ~100 LOC:
- Criar arquivo `.theo/memory/{user_hash}.meta.json` com metadata per-entry (indexed por dedup_key)
- Provider carrega sidecar no startup, atualiza em cada prefetch/sync
- Entries no .md continuam sem metadata (human-readable preservado)

**Opcao B (structured entries)** — ~150 LOC:
- Mudar formato de `## Turn\n**user:**...\n**assistant:**...` para bloco com YAML frontmatter per-entry
- Breaking change no formato on-disk (exige migracao)

**Decisao**: Opcao A (sidecar). Menos invasiva, preserva formato .md, custo real ~100 LOC (NAO 60 como estimado antes).

**O que fazer**:
1. Criar struct `EntryMetadata { created_at: u64, last_hit_at: Option<u64>, hit_count: u32, lifecycle: MemoryLifecycle }`
2. Persistir como `.meta.json` sidecar (HashMap<[u8;32], EntryMetadata>)
3. No `prefetch()`: carregar sidecar, calcular `tick()` para cada entry, filtrar por lifecycle
4. Incrementar hit_count quando entry incluida no resultado

**AC**:
- AC-2.2.1: Sidecar `.meta.json` persistido ao lado do .md
- AC-2.2.2: Cada entry mapeada por dedup_key a `EntryMetadata`
- AC-2.2.3: `tick()` chamado no prefetch para cada entry com metadata
- AC-2.2.4: Entries sem metadata (legacy) tratadas como Active com created_at=now (graceful migration)
- AC-2.2.5: Active retorna, Cooling retorna se usefulness > 0.30, Archived nao retorna
- AC-2.2.6: Hit count incrementado e sidecar re-persistido

**DoD**: `cargo test -p theo-infra-memory` green. <= 100 LOC (revisado de 60). **G7 atingido**.

---

### Task 2.3: Wire Hypothesis Feedback Loop

**Crate**: `theo-agent-runtime`

**O que fazer**:
1. `unresolved_hypotheses` de episodes → persistir em `.theo/memory/hypotheses/{id}.json` (com `schema_version`)
2. Proximo run: carregar hypotheses Active, injetar como "Working hypotheses"
3. Atualizar confidence via Laplace smoothing
4. Auto-prune: `evidence_against > evidence_for * 2` → Superseded

**AC**:
- AC-2.3.1: Persistidas em `.theo/memory/hypotheses/{id}.json` com `schema_version`
- AC-2.3.2: Hypotheses Active injetadas no context
- AC-2.3.3: Confidence atualizada (Laplace)
- AC-2.3.4: against > for * 2 → Superseded, removida
- AC-2.3.5: Stale (> 7 dias sem update) marcada

**DoD**: `cargo test -p theo-agent-runtime` green. <= 80 LOC. **G6 atingido**.

---

**Phase 2 DoD**: G5, G6, G7 atingidos.

---

## Phase 3: SURPASS — O que Ninguem Tem

> **LOC estimado**: ~300
> **Depende de**: Phases 0 + 1 completas (Phase 2 pode ser paralela)

### Task 3.1: Reasoning Preservation no SessionTree

**Crate**: `theo-agent-runtime`

**Scope warning**: O runtime atual empurra reasoning para texto visivel do assistant, nao preserva thinking blocks como estado oculto reutilizavel. Adicionar `reasoning: Option<String>` e re-injetar NAO e uma extensao local do SessionTree — abre questoes de model-policy (quais providers requerem/aceitam reasoning replay?), prompt-shape (reasoning blocks consumem tokens do context window), e compatibilidade cross-provider (Anthropic sim, OpenAI depende do modelo). Esta task exige um mini-ADR antes de implementar.

**O que fazer**:
1. Escrever mini-ADR documentando: quais providers suportam reasoning replay, custo em tokens, impacto no prompt shape
2. Extender `SessionEntry::Message` com `reasoning: Option<String>` (serde skip_serializing_if None)
3. Capturar thinking blocks do streaming response quando provider suporta
4. Re-inject CONDICIONAL: apenas quando mesmo provider e usado no rebuild E provider aceita reasoning replay

**AC**:
- AC-3.1.1: Mini-ADR escrito e linkado
- AC-3.1.2: Campo `reasoning: Option<String>` em Message (skip_serializing_if None)
- AC-3.1.3: Thinking blocks capturados apenas de providers que os emitem
- AC-3.1.4: Re-inject APENAS quando provider de rebuild == provider de captura
- AC-3.1.5: Providers sem support → campo None, zero impacto
- AC-3.1.6: Backward-compatible: JSONL antigo → `None`

**DoD**: `cargo test -p theo-agent-runtime` green. Mini-ADR escrito. <= 80 LOC (revisado de 60).

---

### Task 3.2: Background Prefetch Queue

**Crate**: `theo-agent-runtime`

**Primitiva**: `tokio::sync::oneshot` (decisao #7 — NAO `Arc<Mutex<Option>>`)

**O que fazer**:
1. Apos `sync_turn()`, spawn background task para pre-computar proximo prefetch
2. Na proxima `prefetch()`, `try_recv()` no oneshot receiver
3. Se Ok → usar cached. Se Err → prefetch sincrono normal

**AC**:
- AC-3.2.1: Background prefetch disparado via `tokio::spawn` apos sync_turn
- AC-3.2.2: Resultado via `oneshot::Sender<String>`
- AC-3.2.3: `try_recv()` no proximo prefetch — hit usa cache
- AC-3.2.4: Background failure → `Err(RecvError)` → prefetch normal (distingue de "sem memoria")
- AC-3.2.5: Shutdown graceful: drop do sender cancela
- AC-3.2.6: Latency budget: prefetch com cache hit < 1ms

**DoD**: `cargo test -p theo-agent-runtime` green. <= 50 LOC.

---

### Task 3.3: Retrieval Budget Packing Ativo

**Crate**: `theo-infra-memory` + `theo-engine-retrieval`

**BLOQUEADA** (decisao #6) ate:
1. Mini eval dataset com 20-30 pares (query, hit esperado por tipo)
2. `BudgetConfig` atualizado com campo `memory_pct` (reconciliado com totais existentes)
3. Thresholds marcados como `// PLACEHOLDER: not calibrated` ate calibracao

**O que fazer (quando desbloqueada)**:
1. Adicionar `memory_pct: f64` ao `BudgetConfig` com validacao `assert!(total == 1.0)`
2. Ativar feature `tantivy-backend`
3. Wire `RetrievalBackedMemory` como 2o provider no MemoryEngine
4. Thresholds per source type: calibrados via eval dataset
5. Budget packing: limitar injection ao `memory_pct` do context window

**AC**:
- AC-3.3.1: `BudgetConfig` tem campo `memory_pct` e soma = 1.0
- AC-3.3.2: `RetrievalBackedMemory` registrado como 2o provider
- AC-3.3.3: Prefetch retorna hits filtrados por threshold calibrado
- AC-3.3.4: Total tokens injetados <= `memory_pct` do context
- AC-3.3.5: Thresholds definidos a partir do eval dataset (P10 dos positivos)
- AC-3.3.6: Zero hits → string vazia

**Pre-requisitos**:
- [ ] `eval_memory_dataset.json` com 20-30 pares criado
- [ ] `BudgetConfig.memory_pct` implementado e benchmarks re-executados
- [ ] Score distribution medida e thresholds definidos empiricamente

**DoD**: `cargo test --workspace` green. Benchmarks MRR/Hit@5 nao degradam. <= 100 LOC. **G9 atingido**.

---

### Task 3.4: User/Agent Memory Split

**Crate**: `theo-infra-memory`
**Arquivo**: `src/builtin.rs`

**Paths**: `.theo/memory/{user_hash}_agent.md` e `.theo/memory/{user_hash}_user.md`
(preserva per-user isolation via user_hash — NAO usa nomes fixos compartilhados)

**O que fazer**:
1. Split em dois arquivos PER USER: `{user_hash}_agent.md` + `{user_hash}_user.md`
2. Prefetch concatena ambos com labels: `[agent-knowledge]` e `[user-model]`
3. Classificacao basica: keywords "user", "prefer", "style" → user_model; resto → agent_knowledge
4. Decay diferencial via sidecar metadata (T2.2): user_model 30 dias, agent_knowledge 7 dias
5. Migracao: arquivo unico `{user_hash}.md` existente → renomear para `{user_hash}_agent.md`

**AC**:
- AC-3.4.1: Dois arquivos per-user em `.theo/memory/` (cobertos pelo .gitignore)
- AC-3.4.2: Prefetch com labels distintos
- AC-3.4.3: Classificacao por keywords
- AC-3.4.4: Decay diferencial (30d user vs 7d agent) via sidecar de T2.2
- AC-3.4.5: Migracao: `{hash}.md` → `{hash}_agent.md` (rename, nao copy)
- AC-3.4.6: Per-user isolation PRESERVADA (user_hash no nome do arquivo)

**DoD**: `cargo test -p theo-infra-memory` green. <= 80 LOC.

---

**Phase 3 DoD**: G9 atingido (quando T3.3 desbloqueada). Reasoning + bg prefetch + user/agent split funcionais.

---

## Sequenciamento e Dependencias

```
Pre-Phase 0 (PREP)
  ├─ P.1: Mover episodes path
  ├─ P.2: Unicode hardening
  ├─ P.3: schema_version
  ├─ P.4: Rename Invalidated
  └─ P.5: Sidebar fix
          │
          ▼
Phase 0 (WIRE — atomico)
  ├─ T0.1: Wire hooks + remover FileMemoryStore ad-hoc
  ├─ T0.2: Instantiate MemoryEngine + BuiltinProvider
  └─ T0.3: Feed episodes back
                    │
            ┌───────┴───────┐
            ▼               ▼
     Phase 1 (GAPS)   Phase 2 (ACTIVATE)
       ├─ T1.1 ──┐      ├─ T2.1 (lessons)
       ├─ T1.2 ──┤      ├─ T2.2 (decay)
       ├─ T1.3 ──┤      └─ T2.3 (hypotheses)
       └─ T1.4 ──┘
            │               │
            └───────┬───────┘
                    ▼
             Phase 3 (SURPASS)
               ├─ T3.1 (reasoning)
               ├─ T3.2 (bg prefetch)
               ├─ T3.3 (retrieval — BLOQUEADA ate eval)
               └─ T3.4 (user/agent split)
```

**Dependencias explicitas**:
- Pre-Phase 0 BLOQUEIA Phase 0
- T0.1 BLOQUEIA tudo (sem hooks, nada funciona)
- T0.2 BLOQUEIA T1.2, T2.2, T3.4 (precisam de provider ativo)
- T1.3 depende de T0.1 (compaction hooks)
- T2.1 depende de T0.3 (episodes precisam existir)
- T3.3 BLOQUEADA ate eval dataset + BudgetConfig update
- Phase 1 e Phase 2 sao PARALELAS

---

## DoD Global

Todos true para declarar "Memory & State: Theo > Hermes":

- [ ] **G1**: Memory prefetch/sync rodando
- [ ] **G2**: Cross-session search < 50ms
- [ ] **G3**: Token/cost tracking ao final do run
- [ ] **G4**: Compaction com hooks + token-based tail + anti-thrashing + oversized protection
- [ ] **G5**: Lessons com 7 gates + quarantine + promotion
- [ ] **G6**: Hypotheses tracked + Laplace + auto-prune
- [ ] **G7**: Decay Active→Cooling→Archived no prefetch
- [ ] **G8**: Frozen snapshot (OnceLock) previne re-reads
- [ ] **G9**: Retrieval com thresholds CALIBRADOS e budget cap (bloqueada ate eval)
- [ ] **G10**: Episode summaries injetados no context
- [ ] `cargo test --workspace` green
- [ ] `cargo check --workspace --tests` zero warnings
- [ ] Zero `unwrap()` / `expect()` em producao
- [ ] Cada task <= 200 LOC
- [ ] FileMemoryStore ad-hoc removido
- [ ] Unicode injection bloqueada
- [ ] schema_version em todos os artefatos persistidos
- [ ] Sidebar do desktop com Memory group funcional

---

## Referencias a Absorver (decisao #15)

| Referencia | Relevancia | Onde aplicar |
|------------|-----------|--------------|
| MemArchitect (arXiv:2603.18330) | Governance policies, "Triage & Bid" | Gate 6 (contradiction) quando NLI for adicionado |
| Knowledge Objects (arXiv:2603.17781) | Hash-addressed immutable facts | Lesson promotion: confirmed → hash-keyed KO |
| CodeTracer (arXiv:2604.11641) | Hypothesis failure diagnosis | Hypothesis feedback loop (T2.3) |
