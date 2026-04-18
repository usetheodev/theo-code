---
id: 20260416-121943
date: 2026-04-16
topic: "Harness Engineering: 5 Gaps da Pesquisa para o Theo Code"
verdict: REVISED
participants: 16
---

# Reuniao: Harness Engineering — 5 Gaps Identificados na Pesquisa

## Pauta

### Contexto
Leitura de 9 documentos de pesquisa (5 papers academicos + 4 artigos de industria) sobre harness engineering, agent optimization e benchmarking. Identificados 5 gaps entre estado da arte e implementacao atual do Theo Code.

### Questoes a Decidir
1. Prioridade relativa dos 5 gaps (P0-P3)
2. Sequencia de implementacao
3. Quais sao YAGNI para o momento atual
4. Quais podem ser incrementais vs big-bang

### Restricoes Conhecidas
- 12 crates Rust, 1879+ testes passando
- theo-application com build quebrado (SymbolKind/SymbolKindDto)
- TDD obrigatorio
- Bounded contexts devem ser respeitados
- Pivot recente: Theo Code = standalone AI coding assistant

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE com restricoes | Gaps 1, 2 e 4 sao viaveis e incrementais. Gap 3 e YAGNI — ja temos CLAUDE.md + skills + hooks que funcionam como proto-NLAHs. Gap 5 e P3 — exige benchmark maturo primeiro. Sequencia: corrigir theo-application ANTES de qualquer gap. Depois: Gap 4 (menor risco, maior ROI imediato) → Gap 2 → Gap 1 → Gap 3 → Gap 5. |
| evolution-agent | APPROVE | Os 5 gaps representam evolucao natural do sistema. Gap 1 (self-evolution) e o mais transformador — CorrectionEngine + HeuristicReflector ja existem como Phase 1, falta o loop estruturado com reflection. Gap 5 (auto-optimization) e o holy grail mas prematuro sem metricas solidas. Recomenda: instrumentar primeiro (Gap 4), depois evoluir (Gap 1). |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | Gap 2 (file-backed state) beneficia diretamente a Code Wiki — episodios persistidos viram fontes de conhecimento. SessionTree ja esta definida mas nao integrada. Priorizar integracao sobre criacao de novos tipos. |
| ontology-manager | CONCERN | Gap 3 quer formalizar contracts/gates/failure taxonomy como tipos. Risco de duplicacao: theo-domain ja tem CapabilitySet, PermissionRule, RunState, ToolCallState, TransitionError. Nao criar tipos paralelos — estender os existentes (regra: extend not duplicate). |
| data-ingestor | ABSTAIN | Gaps nao impactam diretamente ingestao de dados. Apenas observo que file-backed state (Gap 2) deve usar formatos estaveis (JSON/JSONL, nao binarios) para facilitar ingestao futura. |
| wiki-expert | APPROVE | Gap 2 viabiliza Deep Wiki com runtime insights persistidos. Gap 4 (sensors) pode alimentar wiki com metricas de qualidade automaticas. Ambos alinham com a visao Deep Wiki aprovada anteriormente. |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | CONCERN | Gap 1 (self-evolution loop) introduz risco de loops infinitos se reflection gerar hipoteses divergentes. Circuit breaker do Pilot (3 loops sem progresso) mitiga parcialmente, mas o cap de 5 tentativas do NLAH paper deve ser enforced como invariante hard. Gap 4 e o mais seguro — sensors computacionais sao deterministicos. |
| linter | APPROVE | Gap 4 (computational sensors) e o que mais impacta saude do codebase. Hoje hooks existem mas nao estao integrados no AgentRunEngine. Integrar hook runner no tool execution path e a acao de menor risco e maior impacto. Clippy + cargo test do modulo afetado pos-edit = deteccao precoce de regressoes. |
| retrieval-engineer | APPROVE | Gap 2 (file-backed state) complementa o retrieval pipeline. Episodios persistidos podem ser indexados pelo RRF ranker para context assembly mais inteligente. Sugiro que file-backed state use o mesmo formato de ContextMetrics (.theo/metrics/) para consistencia. |
| memory-synthesizer | APPROVE | Gap 2 e pre-requisito para sintese avancada. Sem estado persistido, nao ha material para sintetizar. EpisodeSummary ja tem MachineEpisodeSummary + MemoryLifecycle — falta apenas a persistencia real (write to disk, read back). |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | APPROVE com restricoes | Infraestrutura ja existe para 3 dos 5 gaps: HookRunner (Gap 4), SessionTree + FileSnapshotStore (Gap 2), CorrectionEngine + HeuristicReflector (Gap 1). O trabalho e integracao, nao criacao. Risco principal: AgentRunEngine ja e complexo (run_engine.rs) — cada integracao deve ser incremental com testes. |
| graphctx-expert | ABSTAIN | Gaps nao impactam GRAPHCTX diretamente. Observo que Gap 4 poderia usar graph analysis para detectar quais testes rodar pos-edit (dependency-aware test selection), mas isso e otimizacao futura. |
| arch-validator | CONCERN | Bounded contexts: Gap 1 e 2 vivem em theo-agent-runtime (OK). Gap 4 cruza theo-agent-runtime ↔ theo-tooling ↔ theo-governance — cuidado com acoplamento. Sensors devem ser orquestrados pelo runtime, nao hardcoded nas tools. Gap 3 e Gap 5 tocam theo-domain — qualquer novo tipo precisa revisao. |
| test-runner | APPROVE | Todos os gaps sao testaveis. Gap 4 e o mais facil de testar (deterministico). Gap 1 requer testes com mocks de LLM para reflection. Gap 2 requer testes de persistencia (filesystem). Plano TDD viavel para todos. Recomendo comecar pelo Gap 4 — menor superficie de teste, maior confianca. |
| frontend-dev | ABSTAIN | Gaps sao backend/runtime. Impacto na UI e indireto: Gap 2 poderia expor estado persistido na timeline do desktop app, mas isso e P2+. |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE | Evidencia academica forte para Gaps 1, 2, 4. Gap 1: NLAH paper mostra self-evolution como melhor modulo individual (+4.8 pontos em SWE-bench). Gap 2: file-backed state melhora auditability e trace quality sem ganho direto em score. Gap 4: Bockeler e OpenAI convergem — sensors computacionais sao o fundamento. Gap 3: NLAH propoe mas resultados sao mistos (mais estrutura ≠ melhor score). Gap 5: VeRO mostra gains task-dependent e optimizers defaultam para prompt edits — ROI incerto. |

---

## Conflitos

### Conflito 1: Gap 3 (Harness Portavel) — YAGNI ou Fundacional?
- **evolution-agent**: Quer formalizar para permitir harness como search space (otimizacao futura)
- **ontology-manager**: Risco de duplicacao com tipos existentes
- **chief-architect**: YAGNI — CLAUDE.md + skills + hooks ja funcionam
- **research-agent**: Resultados mistos no paper
- **Resolucao**: DEFERRED para P3. Monitorar se demanda surge organicamente. Nao criar tipos novos — usar os existentes.

### Conflito 2: Gap 1 vs Gap 4 — Qual primeiro?
- **evolution-agent**: Gap 1 e mais transformador
- **linter + test-runner**: Gap 4 e mais seguro e facil de testar
- **validator**: Gap 4 e deterministico, Gap 1 introduz risco de loops
- **Resolucao**: Gap 4 primeiro (fundamento), Gap 1 depois (construido sobre sensors).

### Conflito 3: Gap 5 (Auto-optimization) — Prematuro?
- **evolution-agent**: Holy grail, mas precisa de metricas
- **research-agent**: VeRO mostra ROI incerto
- **chief-architect**: P3, exige benchmark maturo
- **Resolucao**: P3. Prerequisitos: Gap 4 (sensors) + benchmark profissional. Nao implementar agora.

---

## Decisoes

### D1: Corrigir theo-application ANTES de qualquer gap (P0)
- Build quebrado e bloqueante. 26 erros de SymbolKind/SymbolKindDto.
- **Unanime**: 16/16

### D2: Gap 4 (Computational Sensors) = P0.5
- Menor risco, maior ROI imediato
- Infraestrutura ja existe (HookRunner, EventBus)
- Trabalho: integrar hooks no tool execution path do AgentRunEngine
- Sensors iniciais: cargo clippy pos-edit, cargo test do modulo afetado
- **Aprovado**: 14/16 (2 ABSTAIN: data-ingestor, frontend-dev)

### D3: Gap 2 (File-Backed State) = P1
- SessionTree definida mas nao integrada
- EpisodeSummary + MachineEpisodeSummary ja existem
- Trabalho: implementar persistencia real (write/read) no AgentRunEngine
- Formato: JSONL em .theo/state/{run_id}/
- **Aprovado**: 13/16 (3 ABSTAIN)

### D4: Gap 1 (Self-Evolution Loop) = P1.5
- Depende de Gap 4 (sensors fornecem signals para reflection)
- CorrectionEngine + HeuristicReflector sao Phase 1
- Trabalho: adicionar structured reflection entre tentativas (Phase 2)
- Cap hard de 5 tentativas como invariante
- **Aprovado**: 12/16 (2 CONCERN resolvidos com cap, 2 ABSTAIN)

### D5: Gap 3 (Harness Portavel) = DEFERRED (P3)
- YAGNI para momento atual
- Proto-NLAHs ja existem (CLAUDE.md, skills, hooks, governance)
- Revisitar quando demanda surgir organicamente
- **Deferred**: 11/16 concordam

### D6: Gap 5 (Agent Optimization) = DEFERRED (P3)
- Prerequisitos: Gap 4 + benchmark profissional + metricas solidas
- ROI incerto (VeRO paper: gains task-dependent)
- **Deferred**: 14/16 concordam

---

## Sequencia de Implementacao

```
P0:   Corrigir theo-application (build verde)
P0.5: Gap 4 — Computational Sensors (hooks integration)
P1:   Gap 2 — File-Backed State (SessionTree integration)
P1.5: Gap 1 — Self-Evolution Loop (structured reflection)
P3:   Gap 3 — Harness Portavel (DEFERRED)
P3:   Gap 5 — Agent Optimization (DEFERRED)
```

---

## Plano TDD

### P0: Corrigir theo-application
1. RED: `cargo test -p theo-application` deve compilar (atualmente falha com 26 erros)
2. GREEN: Resolver imports de SymbolKind/SymbolKindDto/ReferenceKind/ReferenceKindDto
3. REFACTOR: Verificar se tipos foram movidos ou renomeados; atualizar imports
4. VERIFY: `cargo test --workspace` — todos os 1879+ testes passando

### P0.5: Gap 4 — Computational Sensors
1. RED: Escrever teste `test_post_edit_hook_triggers_clippy` — apos edit tool, hook deve executar sensor
2. RED: Escrever teste `test_sensor_result_injected_as_feedback` — resultado do sensor deve virar mensagem no contexto
3. GREEN: Integrar HookRunner.run_post_hook() no ToolCallManager apos Succeeded state
4. GREEN: Criar SensorResult type que converte output de hook em Message para injection
5. REFACTOR: Extrair sensor orchestration para modulo dedicado (sensors.rs)
6. VERIFY: `cargo test -p theo-agent-runtime`

### P1: Gap 2 — File-Backed State
1. RED: Escrever teste `test_session_tree_persist_and_reload` — write JSONL, read back, verify entries
2. RED: Escrever teste `test_episode_summary_survives_compaction` — compactar mensagens, episode summary deve ser recuperavel de disco
3. GREEN: Implementar SessionTree::persist() e SessionTree::load() com JSONL em .theo/state/{run_id}/
4. GREEN: Integrar persist no AgentRunEngine.record_session_exit()
5. REFACTOR: Unificar FileSnapshotStore e SessionTree em um unico persistence layer
6. VERIFY: `cargo test -p theo-agent-runtime`

### P1.5: Gap 1 — Self-Evolution Loop
1. RED: Escrever teste `test_reflection_generates_revised_strategy` — apos falha, reflection deve produzir strategy diferente da tentativa anterior
2. RED: Escrever teste `test_evolution_cap_enforced` — apos 5 tentativas, loop deve parar independente de resultado
3. RED: Escrever teste `test_evolution_tracks_attempt_lineage` — cada tentativa deve referenciar a anterior
4. GREEN: Criar EvolutionLoop struct com attempt tracking, reflection hook, cap enforcement
5. GREEN: Integrar com CorrectionEngine (estrategia Replan) e HeuristicReflector
6. REFACTOR: Extrair reflection como trait para permitir Phase 2 (heuristic) e Phase 4 (LLM-based)
7. VERIFY: `cargo test -p theo-agent-runtime`

---

## Action Items

- [ ] **code-reviewer** — Corrigir theo-application build (SymbolKind imports) — P0, imediato
- [ ] **code-reviewer** — Integrar HookRunner no ToolCallManager execution path — P0.5, apos P0
- [ ] **code-reviewer** — Criar sensor orchestration module em theo-agent-runtime/src/sensors.rs — P0.5
- [ ] **code-reviewer** — Implementar SessionTree persistence (JSONL) — P1, apos P0.5
- [ ] **code-reviewer** — Integrar SessionTree.persist() no AgentRunEngine — P1
- [ ] **code-reviewer** — Criar EvolutionLoop struct com cap=5 e reflection hook — P1.5, apos P1
- [ ] **arch-validator** — Validar que sensors nao violam bounded contexts — durante P0.5
- [ ] **test-runner** — Validar plano TDD e viavel para cada fase — antes de cada fase
- [ ] **ontology-manager** — Revisar se novos tipos sao necessarios ou se existentes bastam — durante cada fase
- [ ] **research-agent** — Monitorar papers novos sobre harness engineering — contínuo

---

## Veredito Final

**REVISED**: Os 5 gaps sao validos mas devem ser priorizados rigorosamente. Implementar na sequencia P0→P0.5→P1→P1.5, com Gaps 3 e 5 DEFERRED para P3. Prerequisito absoluto: corrigir build de theo-application. Infraestrutura existente (HookRunner, SessionTree, CorrectionEngine, HeuristicReflector) deve ser integrada antes de criar novos componentes. Estender tipos existentes, nao duplicar.
