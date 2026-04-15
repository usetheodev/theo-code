---
id: 20260409-083445
date: 2026-04-09
topic: "Avaliacao critica: GraphCTX → Code Wiki como context manager para long-running agents"
verdict: REVISED
participants: 16
---

# Reuniao: GraphCTX como Context Manager para Long-Running Agents

## Pauta

**Contexto**: Paulo avaliou o pipeline GraphCTX → Code Wiki como 8/10 em code intelligence mas 6.5/10 como context manager para agentes de longa duracao. Propoe 4 novas camadas: (A) Structural Context Store melhorado, (B) Execution Memory Store, (C) Context Assembler, (D) Checkpoint & Compaction.

**Questoes a decidir**:
1. Concordamos com o diagnostico de 6.5/10?
2. A arquitetura de 4 camadas e a abordagem correta?
3. Quais gaps sao mais criticos?
4. Qual a ordem de implementacao?
5. Devemos criar tipos novos ou estender existentes?

**Restricoes conhecidas**:
- theo-application tem apenas 18 testes (graph_context_service.rs: 4 testes para 1234 LoC)
- 7 violacoes de fronteira arquitetural identificadas
- content_hash usa mtime, nao conteudo real
- TDD obrigatorio

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE c/ PREREQS | Arquitetura de 4 camadas e coerente, mas exige corrigir 7 violacoes de fronteira + aumentar cobertura de testes antes. Execucao: violations → Layer B → Layer A → Layer C → Layer D |
| evolution-agent | CONCERN | Risco de dual ownership entre ContextManager e RunSnapshot. WorkingSet deve ser campo dentro de RunSnapshot, nao componente paralelo. Drift detector precisa de max_corrections para evitar loop infinito |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE condicional | Episode summaries como wiki pages e forte, mas compaction NAO pode acontecer antes da geracao do summary. evidence_refs devem apontar para event_ids retidos |
| ontology-manager | REJECT (como proposto) | ExecutionMemoryEvent duplica DomainEvent. WorkingSet sobrepoe RunSnapshot. Recomenda: estender EventType com variantes cognitivas, nao criar tipo paralelo |
| data-ingestor | APPROVE condicional | JSONL rotation e naive (perde dados). Precisa de WAL para promocoes, archival strategy, backward-compat. Migration function de RuntimeInsight → ExecutionMemoryEvent |
| wiki-expert | APPROVE c/ CONCERN | Episode summaries como wiki pages e perigoso — blurra a linha entre conhecimento e log operacional. Precisa de tier EpisodicCache com TTL hard. Nunca invalidar Deterministic/Enriched por runtime state |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | REJECT | content_hash usa mtime nao conteudo (bug critico). RawCache pode envenenar contexto. RunSnapshot sem schema_version. Bloqueante: corrigir hash antes de qualquer feature |
| linter (tooling) | CONCERN | Pediu clarificacao de escopo (95% rule). Preocupado com explosao de complexidade de 4 camadas novas sobre base com violacoes |
| retrieval-engineer | APPROVE c/ condicoes | Multi-objective scoring viavel mas pesos devem ser configuraveis, nao hardcoded. ScoringContext como struct em reranker.rs |
| memory-synthesizer | CONCERN | Salto grande de KV flat para episodic memory system. invalidates/supersedes cria grafo de dependencias caro. Recomenda: time-windowed compaction primeiro, semantica de invalidacao depois |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer (qa) | REJECT | 18 testes em theo-application e inaceitavel. Nao adicionar features sobre base sem testes. TDD plan concreto fornecido para content hash como P0 |
| graphctx-expert | APPROVE c/ condicoes | Symbol hash OK (+5ms negligivel). Co-change impact predictor OK mas sem side-effects no grafo. Working set assembly pertence a theo-application, NAO a engine |
| arch-validator | REJECT c/ fixes | 7 violacoes de fronteira encontradas. Component placement definido. Corrigir violacoes ANTES de qualquer nova camada |
| test-runner | CONCERN → APPROVE | Fix theo-application primeiro. TDD plan completo: RED-GREEN-REFACTOR para cada P0 item |
| frontend-dev | APPROVE c/ CONCERN | Working Set Viewer (painel colapsavel) e Checkpoint dividers no chat. NAO mostrar raw memory. Episode-to-wiki precisa de tier separado |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | CONCERN | ExecutionMemoryEvent com 16 campos e research-grade, nao shipping-grade. Nenhum agent em producao (SWE-agent, Devin, Claude Code) usa schema tao estruturado. Medir primeiro, projetar depois |

---

## Conflitos

### Conflito 1: Criar tipos novos vs Estender existentes

**Proposta original**: 4 tipos novos (ExecutionMemoryEvent, WorkingSet, ContextManager, EpisodeSummary)

**Ontology-manager**: REJECT — ExecutionMemoryEvent duplica DomainEvent, WorkingSet sobrepoe RunSnapshot

**Chief-architect**: APPROVE — camadas bem separadas

**Resolucao**: **Ontology-manager vence**. Estender DomainEvent com novos EventType variants (HypothesisFormed, HypothesisInvalidated, ConstraintLearned, DecisionMade) e mais KISS e respeita DRY. WorkingSet vira campo em RunSnapshot (evolution-agent concordou). Unico tipo genuinamente novo: **EpisodeSummary**.

### Conflito 2: Construir agora vs Medir primeiro

**Proposta original**: Implementar P0 imediatamente

**Research-agent**: Medir onde o contexto realmente quebra em runs >20 iteracoes antes de projetar schema

**QA/Validator**: Corrigir fundacoes (hash, testes) antes de qualquer feature

**Resolucao**: **Ambos vencem**. Sequencia: (1) Corrigir fundacoes, (2) Instrumentar runtime para medir context breakdown, (3) Projetar schema com dados reais, (4) Implementar.

### Conflito 3: Episode summaries como wiki pages

**Knowledge-compiler**: Sim, e forte para narrativa

**Wiki-expert**: Perigoso, polui o indice BM25 com ruido operacional

**Resolucao**: **Wiki-expert vence com condicao**. Episode summaries ficam em tier separado (EpisodicCache) com TTL hard, excluido do indice BM25 principal. Sao consultaveis mas nao competem com paginas Deterministic no ranking.

### Conflito 4: Onde vive o Context Assembler

**GraphCTX-expert**: Na application layer, NAO no engine

**Chief-architect**: Concorda — theo-application como orquestrador

**Resolucao**: **Consenso**. ContextAssembler e um use case em theo-application que consome dados de graph + runtime + wiki.

---

## Consensos

1. **UNANIME**: Corrigir violacoes de fronteira e debt de testes ANTES de qualquer feature nova
2. **UNANIME**: content_hash deve usar hash de conteudo real (blake3), nao mtime
3. **FORTE**: Estender DomainEvent com variantes cognitivas em vez de criar ExecutionMemoryEvent paralelo
4. **FORTE**: EpisodeSummary e o unico tipo genuinamente novo necessario
5. **FORTE**: Working set assembly pertence a theo-application
6. **FORTE**: WorkingSet como campo em RunSnapshot, nao componente separado
7. **FORTE**: Pesos de scoring multi-objetivo devem ser configuraveis
8. **MODERADO**: Instrumentar antes de projetar schema completo

---

## Decisoes

1. **DIRECAO APROVADA, ABORDAGEM REVISADA**: O diagnostico de 6.5/10 esta correto. A direcao de 4 camadas e valida, mas a implementacao deve estender tipos existentes em vez de criar paralelos.

2. **PREREQUISITOS BLOQUEANTES** (Sprint 0):
   - Corrigir 7 violacoes de fronteira arquitetural
   - Aumentar testes de theo-application (graph_context_service.rs) para ≥80% cobertura
   - Substituir mtime por blake3 content hash em compute_project_hash()
   - Adicionar schema_version a RunSnapshot

3. **TIPO NOVO APROVADO**: EpisodeSummary em theo-domain

4. **EXTENSAO APROVADA**: DomainEvent ganha variantes: HypothesisFormed, HypothesisInvalidated, ConstraintLearned, DecisionMade

5. **PLACEMENT DEFINIDO**:
   | Componente | Crate |
   |---|---|
   | EpisodeSummary type | theo-domain |
   | Novos EventType variants | theo-domain |
   | WorkingSet (campo em RunSnapshot) | theo-agent-runtime |
   | ContextAssembler use case | theo-application |
   | Symbol/community hash | theo-engine-graph |
   | Impact set computation | theo-engine-graph |
   | ScoringContext (pesos configuraveis) | theo-engine-retrieval |
   | EpisodicCache tier | theo-engine-retrieval/wiki |

6. **DEFERIDO**: invalidates/supersedes semantics, cross-run hypothesis tracking, full multi-field ExecutionMemoryEvent (aguarda instrumentacao e dados reais)

---

## Plano TDD

### Sprint 0: Fundacoes (BLOQUEANTE)

**Item 1: Content Hash**
```
RED:   test_content_hash_stable_when_mtime_changes_but_content_identical()
       test_content_hash_changes_when_content_changes_same_mtime()
GREEN: blake3::hash(file_bytes) em compute_project_hash(). mtime como pre-filtro.
REFACTOR: Extrair ContentHasher trait em theo-domain.
VERIFY: cargo test -p theo-application -p theo-engine-graph
```

**Item 2: RunSnapshot schema_version**
```
RED:   test_snapshot_rejects_unknown_schema_version()
       test_snapshot_deserialize_v1_to_current()
GREEN: Adicionar schema_version: u32 ao RunSnapshot. Err em version mismatch.
REFACTOR: Migration trait para versoes futuras.
VERIFY: cargo test -p theo-agent-runtime -- snapshot
```

**Item 3: theo-application test coverage**
```
RED:   20-30 testes para GraphContextService (init, query, cache hit/miss, timeout, error recovery)
GREEN: Implementar testes com mocks de GraphContextProvider
REFACTOR: Extrair test helpers reutilizaveis
VERIFY: cargo test -p theo-application
```

### Sprint 1: Extensoes de Tipos

**Item 4: DomainEvent variants**
```
RED:   test_hypothesis_formed_event_carries_rationale()
       test_hypothesis_invalidated_links_to_original()
GREEN: Novos variantes em EventType enum.
REFACTOR: Garantir que EventBus propaga corretamente.
VERIFY: cargo test -p theo-domain -p theo-agent-runtime
```

**Item 5: EpisodeSummary**
```
RED:   test_episode_summary_generated_from_events_in_time_window()
       test_episode_summary_preserves_evidence_refs()
GREEN: EpisodeSummary struct + compact() method.
REFACTOR: Integrar com persistence layer.
VERIFY: cargo test -p theo-domain -p theo-agent-runtime
```

### Sprint 2: Context Assembly

**Item 6: WorkingSet em RunSnapshot**
```
RED:   test_working_set_included_in_snapshot_serialization()
       test_working_set_restored_on_checkpoint_load()
GREEN: Campo working_set: WorkingSet em RunSnapshot.
REFACTOR: WorkingSet builder com hot files + recent events.
VERIFY: cargo test -p theo-agent-runtime -- snapshot
```

**Item 7: ContextAssembler**
```
RED:   test_assembler_respects_token_budget()
       test_assembler_prioritizes_task_relevant_context()
GREEN: ContextAssembler::assemble(budget, task_context) em theo-application.
REFACTOR: Extrair TokenCounter trait. Pesos configuraveis.
VERIFY: cargo test -p theo-application -- assembler
```

---

## Action Items

- [ ] **arch-validator** — Corrigir 7 violacoes de fronteira — Sprint 0
- [ ] **QA** — Implementar content hash com blake3 (TDD) — Sprint 0
- [ ] **QA** — Adicionar schema_version a RunSnapshot (TDD) — Sprint 0
- [ ] **test-runner** — Escrever 20-30 testes para graph_context_service.rs — Sprint 0
- [ ] **ontology-manager** — ADR: EventType variants vs ExecutionMemoryEvent — Sprint 0
- [ ] **chief-architect** — ADR: EpisodeSummary schema e lifecycle — Sprint 1
- [ ] **graphctx-expert** — Implementar symbol-level hashing em persist.rs — Sprint 1
- [ ] **retrieval-engineer** — ScoringContext com pesos configuraveis — Sprint 2
- [ ] **wiki-expert** — Definir EpisodicCache tier com TTL — Sprint 2
- [ ] **frontend-dev** — Prototipo Working Set Viewer (colapsavel) — Sprint 2
- [ ] **runtime-engineer** — WorkingSet como campo em RunSnapshot — Sprint 2
- [ ] **research-agent** — Instrumentar runtime para medir context breakdown em runs >20 iteracoes — Sprint 1
- [ ] **infra** — WAL para promotion ledger + archival strategy — Sprint 2

---

## Veredito Final

**REVISED**: A direcao e unanimemente aprovada — o gap de 6.5/10 em context management e real e precisa ser corrigido. Porem, a abordagem foi revisada significativamente:

1. **Nao criar 4 tipos paralelos** — estender DomainEvent e RunSnapshot existentes. Unico tipo novo: EpisodeSummary.
2. **Fundacoes primeiro** — corrigir violacoes, testes e content hash antes de qualquer feature.
3. **Medir antes de projetar** — instrumentar o runtime para coletar dados de onde o contexto realmente quebra.
4. **Sequenciamento disciplinado** — Sprint 0 (fundacoes) → Sprint 1 (tipos + instrumentacao) → Sprint 2 (assembly + UI).

Frase-resumo: **"Concordamos com o diagnostico, discordamos da implementacao. Estender, nao duplicar. Medir, nao especular. Fundacoes antes de features."**

---

## Addendum: Review do Paulo (pos-reuniao)

**Data**: 2026-04-09
**Status**: Aceito pelo time. Refinamentos incorporados ao plano.

### Refinamento 1: Contrato de EpisodeSummary

O time aprovou EpisodeSummary mas nao definiu o que ele e semanticamente. Paulo identifica 3 usos potencialmente conflitantes: (1) resumo narrativo para humanos, (2) artefato de retomada para agente, (3) item consultavel de memoria operacional. Sao coisas diferentes.

**Decisao**: Separar desde o inicio com struct dual:

```rust
pub struct EpisodeSummary {
    pub summary_id: String,
    pub run_id: String,
    pub window_start_event_id: String,
    pub window_end_event_id: String,

    pub machine_summary: MachineEpisodeSummary,
    pub human_summary: Option<HumanReadableSummary>,

    pub evidence_event_ids: Vec<String>,
    pub affected_files: Vec<String>,
    pub open_questions: Vec<String>,
    pub unresolved_hypotheses: Vec<String>,
    pub schema_version: u32,
    pub ttl_policy: TtlPolicy,
}
```

### Refinamento 2: Invariantes causais para EventType cognitivos

Sem regras explicitas, eventos cognitivos viram log glorificado. Invariantes obrigatorias:

- `HypothesisFormed` DEVE ter `rationale`
- `HypothesisInvalidated` DEVE apontar para hipotese anterior
- `DecisionMade` DEVE carregar opcao escolhida + evidencias
- `ConstraintLearned` DEVE indicar escopo: run-local | task-local | workspace-local

### Refinamento 3: supersedes_event_id minimo (nao deferido completamente)

Grafo de invalidacao completo continua deferido. Mas campo simples opcional:

```rust
supersedes_event_id: Option<EventId>
```

E barato e preserva trilha minima para evolucao futura. Adicionado ao Sprint 1.

### Refinamento 4: Sprint 0.5 — Observabilidade de contexto

Fase explicita entre Sprint 0 e Sprint 1:

- Contar tamanho medio do contexto por step
- Medir frequencia de re-fetch dos mesmos artefatos
- Medir repeticao de acoes
- Medir quantas vezes o agente perde hipotese/plano
- Medir quantas retomadas repetem trabalho

### Refinamento 5: schema_version em tudo que persiste

Nao so RunSnapshot. Tambem em:
- EpisodeSummary
- Payload de DomainEvent cognitivo
- Qualquer persistencia de EpisodicCache

Essa area vai mudar rapido. Sem versionamento desde o inicio, retrocompatibilidade vira caos em dois sprints.

### Refinamento 6: ContextAssembler nasce minimo

Versao inicial deterministica:

```
assemble() =
  task_context
  + current_plan
  + recent_events(limit N)
  + hot_files(limit K)
  + top_structural_matches(limit M)
```

4 regras hard:
1. Respeitar token budget
2. Sempre incluir task objective
3. Sempre incluir current step
4. Sempre incluir evidencias recentes relevantes

Pesos configuraveis e multiobjective reranking sao Sprint 2+.

### Riscos de disciplina identificados

| # | Risco | Mitigacao |
|---|---|---|
| 1 | DomainEvent vira dumping ground de semantica cognitiva mal definida | Invariantes causais obrigatorias (Refinamento 2) |
| 2 | EpisodeSummary vira mini-wiki ou mini-log ao mesmo tempo | Struct dual machine/human (Refinamento 1) |
| 3 | ContextAssembler sofisticado demais antes de provar valor | Versao minima deterministica (Refinamento 6) |
| 4 | Cobertura alta mas sem testes de falha/recuperacao | TDD deve incluir cenarios de erro, nao so happy path |

### Sequenciamento Revisado Final

```
P0:    blake3 content hash + schema_version + violacoes de fronteira + testes de falha
P0.5:  Instrumentacao de breakdown de contexto (metricas de repeticao, perda, retrabalho)
P1:    DomainEvent variants (com invariantes) + EpisodeSummary (dual) + WorkingSet em RunSnapshot + supersedes_event_id
P2:    ContextAssembler minimo + EpisodicCache + UI so depois que backend provar valor
P3:    Symbol/community hash + impact set + scoring configuravel + promotion WAL + refinamentos
```

### Avaliacao do Paulo

> "O diagnostico continua certo; agora a implementacao comecou a ficar adulta."

Conflitos resolvidos corretamente. Ontology-manager, QA+Research, Wiki-expert e GraphCTX-expert venceram nos pontos certos.
