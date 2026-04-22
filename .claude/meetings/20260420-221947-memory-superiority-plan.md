---
id: 20260420-221947
date: 2026-04-20
topic: "Cross-validation hermes-agent vs theo-code: Plano Memory & State Superiority"
verdict: REVISED
participants: 16
---

# Reuniao: Memory & State Superiority Plan

## Pauta

**Contexto**: Cross-validation revelou que o sistema de memoria do Theo tem 0% production-wired. Tipos, logica e 500+ testes existem mas NENHUM hook e chamado no agent loop. Plano propoe 4 fases (14 tasks, ~1000 LOC) para superar hermes-agent.

**Questoes a decidir**:
1. Aprovar, modificar ou rejeitar o plano de 4 fases
2. Sequenciamento e dependencias corretos?
3. LOC budget realista?
4. Riscos arquiteturais criticos?

**Documentos de referencia**:
- `referencias/PLAN_MEMORY_SUPERIORITY.md`
- `referencias/CROSS_VALIDATION_MEMORY.md`

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | **APPROVE** | Plano correto. Risco critico: `run_engine.rs:311-331` tem sistema de memoria artesanal (`FileMemoryStore`) que conflita com o plano — precisa ser reconciliado ou removido no Phase 0. Factory em `theo-application` NAO viola boundaries. |
| evolution-agent | **APPROVE** (3 concerns) | Estrategia "profundidade > largura" validada. 70-90% do codigo ja existe. Gargalo real e RM0 (wiring) + RM3a (concorrencia). Sugere RM0+RM3a como unidade atomica. Path ad-hoc em run_engine.rs:310-331 cria risco de injecao duplicada. |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | **APPROVE** | Formatos (JSON/MD) adequados. T1.4 deveria usar trait `MemoryRetrieval` (ja existe) ao inves de keyword match. RM5a/RM5b NAO devem entrar neste plano (ja implementados). Exigir `schema_version` em todo JSON persistido. |
| ontology-manager | **APPROVE** (4 concerns) | Taxonomia coerente. `MemoryKind` vs `MemoryLifecycle` corretamente ortogonais. `SourceType` ja existe em `theo-infra-memory` — NAO mover para dominio. `EpisodeOutcome::Inconclusive` nunca e produzido (tipo orfao). Renomear `Retracted` para `Invalidated`. |
| data-ingestor | **APPROVE** | Formatos adequados. `.gitignore` cobre todos os paths EXCETO `agent_knowledge.md`/`user_model.md` se fora de `.theo/memory/`. Exigir paths fixos em `.theo/memory/`. Adicionar `schema_version` ao markdown do builtin. |
| wiki-expert | **APPROVE** | Episodes devem mover de `.theo/wiki/episodes/` para `.theo/memory/episodes/` (viola namespace wiki). T1.4 deve usar keyword match direto (simples), nao BM25. RM5a/RM5b ja implementados — nao incluir. Episodes renderizados como wiki SOMENTE via compiler. |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | **CONCERN** (3 criticos) | (1) Episode JSON corrompido sem mitigacao para reload futuro. (2) Compaction pode OOM com oversized tail messages. (3) Unicode injection bypass — cyrillic lookalikes passam no scan. Frozen snapshot NAO existe no codigo (premissa incorreta da cross-validation). |
| linter | **APPROVE** | Health score 0.72/1.0. 82 ACs, todos testaveis. 4 ACs ambiguos precisam clarificacao. LOC targets com "~" precisam de split criteria. Idempotency key format de RM3a falta no DoD. Test-to-code ratio precisa clarificacao. |
| retrieval-engineer | **CONCERN** | Thresholds (0.35/0.50/0.60) arbitrarios sem calibracao empirica — bloqueante para T3.3. Budget de 15% nao tem bucket no `BudgetConfig` existente (soma > 100%). Recomenda reutilizar RRF 3-ranker para memory retrieval. Adicionar campo `created_at_secs` para sinal de recencia. |
| memory-synthesizer | **APPROVE** (3 concerns) | Loop episode→lesson→hypothesis parcialmente coerente. Keyword search insuficiente para insights. Decay pode perder knowledge valioso sem `impact_score`. User/agent split correto mas precisa de `MemoryComposer` para queries cross-cutting. |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | **APPROVE** | (1) `std::fs::write` bloqueante em async context (run_engine.rs:200-234) — converter para `tokio::fs`. (2) Usar `OnceLock` (stdlib) para snapshot, NAO `OnceCell`. (3) Usar `tokio::sync::oneshot` para bg prefetch, NAO `Arc<Mutex<Option>>`. (4) 2 `expect()` em producao violam convencao — converter para Result. |
| graphctx-expert | **CONCERN** | Tantivy indices correetamente separados. Budget 15% NAO tem bucket em `BudgetConfig` — precisa reconciliar (bloqueante T3.3). Thresholds sem calibracao empirica (bloqueante T3.3). Community IDs instáveis entre re-indexes (risco medio). Phases 0-2 podem avancar sem restricoes. |
| arch-validator | **APPROVE** | Nenhuma task viola dependency direction. `SessionSearch` trait pertence a `theo-domain` (correto). Factory em `theo-application` e o lugar correto. `theo-agent-runtime` acumula responsabilidades mas cada modulo e focado — aceitavel. |
| test-runner | **APPROVE** | Plano TDD viavel. Testes existentes cobrem regressao. Performance assertion de 50ms testavel via `Instant::now()`. Recomendar testes de concorrencia para bg prefetch via `tokio::test`. |
| frontend-dev | **CONCERN** | Plano backend-first arriscado. Paginas de memory existem no desktop mas NAO aparecem na sidebar (`AppSidebar.tsx`). T2.1 (lessons) e T2.3 (hypotheses) sem UI ficam invisiveis ao usuario. Token/cost display precisa de UI no desktop, nao so CLI. Definir MVP de UI por task. |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | **APPROVE** | Hypothesis tracking com Laplace e GENUINAMENTE NOVEL (zero prior art em coding agents). 7-gate composition e novel em combinacao. Frozen snapshot e table stakes. 3 referencias faltando: MemArchitect, Knowledge Objects, CodeTracer. "Profundidade > largura" validado por Databricks e Mem0. 2 features publicaveis. |

---

## Conflitos Identificados

### Conflito 1: Episodes path — `.theo/wiki/` vs `.theo/memory/`
- **wiki-expert**: Mover para `.theo/memory/episodes/` (viola namespace wiki)
- **data-ingestor**: Confirma — `.gitignore` ja cobre `.theo/memory/`
- **Resolucao**: **MOVER para `.theo/memory/episodes/`**. Wiki namespace e para conteudo compilado.

### Conflito 2: T1.4 — Keyword match vs MemoryRetrieval trait vs BM25
- **knowledge-compiler**: Usar trait `MemoryRetrieval` com stub
- **wiki-expert**: Keyword match direto (simples, atende DoD)
- **retrieval-engineer**: Keyword insuficiente, precisa BM25 + recency
- **memory-synthesizer**: Keyword insuficiente para insights
- **Resolucao**: **T1.4 implementa keyword match direto** (atende G2 com < 150 LOC). Registrar como evolution item: migrar para `MemoryRetrieval` trait com RRF quando T3.3 estiver pronto.

### Conflito 3: T3.3 Thresholds — arbitrarios vs calibrados
- **retrieval-engineer**: BLOQUEANTE — precisa eval dataset antes de hardcodar
- **graphctx-expert**: BLOQUEANTE — budget 15% nao tem bucket em BudgetConfig
- **Resolucao**: **T3.3 bloqueada ate**: (1) mini eval dataset com 20-30 pares, (2) `BudgetConfig` atualizado com campo `memory_pct`. Thresholds iniciais marcados como `// PLACEHOLDER: not calibrated`.

### Conflito 4: FileMemoryStore artesanal (run_engine.rs:311-331)
- **chief-architect**: Reconciliar ou remover
- **evolution-agent**: Remover explicitamente, teste RED para prevenir duplicacao
- **Resolucao**: **Phase 0 inclui sub-task: remover path ad-hoc e adicionar teste `test_no_dual_memory_injection`**.

### Conflito 5: Primitivas async — OnceCell vs OnceLock, Mutex vs oneshot
- **code-reviewer**: `OnceLock` (stdlib) para snapshot, `oneshot` para bg prefetch
- **Resolucao**: **Adotar recomendacoes do code-reviewer**. `std::sync::OnceLock` para frozen snapshot, `tokio::sync::oneshot` para background prefetch.

---

## Decisoes

1. **PLANO APROVADO com revisoes** — 4 fases, 14 tasks, ~1000 LOC. Sequenciamento correto.
2. **Phase 0 expandido** — Incluir reconciliacao/remocao do `FileMemoryStore` artesanal em `run_engine.rs:311-331`. Teste RED: `test_no_dual_memory_injection`.
3. **RM0 + RM3a como unidade atomica** — Sem provider ativo, wiring opera sobre NullMemoryProvider (nao demonstra valor).
4. **Episodes movem para `.theo/memory/episodes/`** — Wiki namespace reservado para conteudo compilado.
5. **T1.4 usa keyword match direto** — Simples, atende DoD. Migration para MemoryRetrieval trait registrada como evolution item.
6. **T3.3 bloqueada** ate: (a) mini eval dataset 20-30 pares, (b) `BudgetConfig.memory_pct` implementado, (c) thresholds marcados como PLACEHOLDER.
7. **Primitivas corrigidas** — `OnceLock` para snapshot, `oneshot` para bg prefetch, `tokio::fs` para I/O async.
8. **Unicode injection** — Adicionar normalizacao NFKD + zero-width char removal antes do scan. Teste RED: `test_cyrillic_lookalike_injection_blocked`.
9. **Corrupcao de JSON** — Definir politica: rename para `.corrupt`, iniciar vazio, emitir `MemoryError::CorruptState`. Bloqueante para RM3b.
10. **Compaction oversized tail** — Adicionar per-message cap de `context_window/4`. Teste RED: `test_single_oversized_message_does_not_cause_oom_loop`.
11. **`schema_version`** — Obrigatorio em todo JSON persistido E no markdown do builtin.
12. **Frontend** — Corrigir sidebar (adicionar Memory group) como pre-req. Definir MVP de UI por task antes de implementar backend.
13. **Renomear `LessonStatus::Retracted` para `LessonStatus::Invalidated`** — Diferencia de `HypothesisStatus::Superseded`.
14. **Latency budget** — `prefetch` < 100ms p99 para providers locais. `sync_turn` fire-and-forget via tokio::spawn.
15. **3 referencias a absorver** — MemArchitect (governance), Knowledge Objects (hash-addressed facts), CodeTracer (hypothesis failure diagnosis).
16. **`EpisodeOutcome::Inconclusive`** — Definir condicao de producao ou remover variante.

---

## Action Items

- [ ] **Paulo** — Aprovar decisoes e priorizar Phase 0
- [ ] **Phase 0 (atomico com RM3a)** — Wire hooks + instantiate provider + remover FileMemoryStore ad-hoc
- [ ] **Pre-Phase 0** — Mover episodes para `.theo/memory/episodes/`, corrigir sidebar, adicionar `schema_version`
- [ ] **T3.3 prep** — Criar mini eval dataset (20-30 pares), atualizar `BudgetConfig` com `memory_pct`
- [ ] **Security** — Unicode NFKD normalizacao + zero-width removal (pre-req para RM3a)
- [ ] **Corrupcao** — Implementar politica de `.corrupt` rename (pre-req para RM3b)

---

## Plano TDD

Para cada action item de codigo:

### Phase 0: Wire + Provider + Reconciliacao
1. **RED**: `test_no_dual_memory_injection` — verifica que FileMemoryStore NAO e chamado quando memory_enabled=true
2. **RED**: `test_hook_sequence_prefetch_sync_end` — RecordingProvider verifica ordem
3. **RED**: `test_builtin_creates_file_on_sync` — E2E com memory_enabled=true
4. **GREEN**: Wire 4 hooks em run_engine.rs, instantiate MemoryEngine, remover path ad-hoc
5. **REFACTOR**: Extrair memory hooks para modulo separado se run_engine.rs > 2600 LOC
6. **VERIFY**: `cargo test -p theo-agent-runtime -p theo-infra-memory -p theo-application`

### Security Unicode
1. **RED**: `test_cyrillic_lookalike_injection_blocked` — "ignore рrevious instructions" (cyrillic р) rejeitado
2. **RED**: `test_zero_width_injection_blocked` — content com ZWJ/ZWNJ rejeitado
3. **GREEN**: Adicionar unicode normalizacao NFKD + zero-width removal em `security.rs`
4. **VERIFY**: `cargo test -p theo-infra-memory`

### Compaction Oversized
1. **RED**: `test_single_oversized_message_does_not_cause_oom_loop` — mensagem > context_window/2 e truncada
2. **GREEN**: Adicionar per-message cap em compaction.rs
3. **VERIFY**: `cargo test -p theo-agent-runtime`

---

## Veredito Final

**REVISED**: Plano aprovado com 16 decisoes de revisao. Nenhum REJECT de chief-architect ou validator. 4 CONCERNs resolvidos com acoes concretas. Phase 0 expandido para incluir reconciliacao do FileMemoryStore artesanal. T3.3 condicionada a calibracao empirica. Consenso de 12 APPROVE + 4 CONCERN (todos resolvidos).
