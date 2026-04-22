---
id: 20260422-150959
date: 2026-04-22
topic: "Agent Observability Engine para Theo Code runtime"
verdict: REVISED
participants: 16
---

# Reuniao: Agent Observability Engine

## Pauta

### Contexto
O Theo Code runtime (`theo-agent-runtime`) ja possui infraestrutura de observabilidade dispersa: `MetricsCollector` (tokens/cost/timing), `ContextMetrics` (refetch rates, usefulness scores, causal links, failure fingerprints), `StructuredLogListener` (JSONL), `HeuristicReflector` (NoProgressLoop, RepeatedSameError), e `FailurePatternTracker` (cross-session). O que falta e uma camada unificada que componha esses primitivos, capture trajectories estruturadas, compute metricas derivadas, e habilite replay/comparison de runs.

### Questoes a decidir
1. Criar novo crate `theo-observability` ou estender `theo-agent-runtime`?
2. Onde vivem os novos tipos (`TrajectoryStep`, `ObservabilityMetrics`) — `theo-domain` ou crate proprio?
3. Como instrumentar sem impactar o hot path do runtime?
4. Qual a especificacao de loop detection?
5. Escopo de replay/comparison (P1 ou P2)?

### Restricoes conhecidas
- `theo-domain` tem ZERO dependencias (invariante)
- Apps nunca importam engine/infra diretamente
- EventBus e sync com listeners, bounded a 10k eventos (FIFO drop silencioso)
- TDD obrigatorio (RED-GREEN-REFACTOR)
- Benchmark isolation: `theo-benchmark` fica separado

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE c/ CONCERNS | Arquitetura correta (EventListener, sem ciclos). Riscos: domain pollution com tipos observability-specific; sync listener fazendo I/O bloqueia hot path; loop detection sem whitelist tera falsos positivos; replay API vaga demais. Recomenda: tipos ficam em theo-observability (nao theo-domain), usar mpsc channel para I/O async, whitelist de repeticoes esperadas, limitar replay a visualizacao de trajectory (sem replay deterministico). |
| evolution-agent | APPROVE c/ RECS | Infra ja existe dispersa (3 locais com overlap). Proposta consolida. Recomenda: comecar como modulo dentro de theo-agent-runtime, promover a crate so quando houver consumidores externos. Definir exatamente 5 metricas v1. Subsumir StructuredLogListener. Replay/comparison e P2. |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE c/ CONCERNS | Gera 8-12 novas paginas wiki. Risco de domain bloat se tipos observability-specific forem para theo-domain. Recomenda: strict type placement rule, ADR de boundary obs vs benchmark, extend DomainEvent nao duplicar. |
| ontology-manager | CONCERN | TrajectoryStep tem 70%+ overlap semantico com ToolCallRecord. ObservabilityMetrics e god-struct sem boundary. TrajectoryStep implica Trajectory parent que nao existe. Recomenda: definir se TrajectoryStep contem info nao-reconstruivel de DomainEvent+ToolCallRecord; se nao, rejeitar e usar projection layer. Decompor ObservabilityMetrics em TokenBudgetMetrics, ToolExecutionMetrics, RetryMetrics, CostMetrics. |
| data-ingestor | CONCERN | StructuredLogListener ja escreve JSONL. FileSnapshotStore ja tem storage por run. Novo crate duplica infra existente. Risco de dependency violation se importar EventBus de theo-agent-runtime. Recomenda: estender theo-agent-runtime com TrajectoryListener (60 linhas), definir filter set de quais EventTypes constituem trajectory, enriquecer DomainEvent com run_id opcional. |
| wiki-expert | CONCERN | Risco de silo se storage desconectado de DomainEvent. Loop detection deveria ficar no runtime, nao no observability crate (SRP). Requer: extensao DomainEvent primeiro, teste de wiki generation, loop detection scoped ao runtime. |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | CONCERN (0.52) | 3 issues CRITICOS: (1) sync listener bloqueia thread tokio com I/O, (2) FIFO drop silencioso — observability pode perder eventos iniciais sem saber, (3) JSONL crash mid-write sem fsync/atomic rename. Tambem: duplicatas aceitas sem dedup, sem backpressure signal. Recomenda: mpsc channel obrigatorio, sentinel de lag/drop, sequence numbers em JSONL, error propagation (nao `let _ = writeln!`). |
| linter | APPROVE c/ 3 GATES | 3 gates obrigatorios antes de implementar: (1) Domain Contract Review, (2) EventBus Contract Clarification (sync vs async listener), (3) Documentation Alignment (ADR + docs/current). Riscos: circular dep, schema sem versioning, EventBus lossy. |
| retrieval-engineer | APPROVE c/ CONDITIONS | Trajectory data pode melhorar ranking (implicit relevance feedback, loop patterns como negative signal). Requer: campo `outcome` e `files_resolved` no schema, offline aggregation only (zero sync coupling), benchmark gate antes de ativar, dependency via theo-application (nao direta). |
| memory-synthesizer | APPROVE c/ CONDITIONS | Fecha gap do Layer 3 (Operational) do Deep Wiki. Requer: ADR antes de codigo, reuse EventBus, schema_version obrigatorio, labeling protocol (converged_successfully, converged_with_doom_loop, failed_budget, failed_test), dataset generation fica em theo-benchmark. |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | CONCERN | CRITICO: sync I/O em on_event bloqueia tokio worker thread. Pattern correto: mpsc sender em on_event (O(1)), background OS thread drena e escreve. DomainEvent clone com serde_json::Value e deep heap alloc — serializar &DomainEvent para bytes em on_event, enviar bytes pelo channel. Atomics para contadores simples, Mutex so para multi-field consistency. |
| graphctx-expert | APPROVE c/ CONDITIONS | Trajectory data pode informar graph ranking. Requer: TDD gate, TrajectorySignal trait em theo-domain, benchmark extension, schema tight (file-level events apenas). |
| arch-validator | APPROVE | Zero violacoes. Dependency graph limpo: theo-observability → theo-domain only. theo-application wira como EventListener. Sem ciclos. Enforce unidirectional flow (listen only, never publish back). |
| test-runner | CONCERN → CONDITIONAL APPROVE | Aprova Phases 1-3 (serialization, composition, persistence). Bloqueia Phase 4 (loop detection) ate RFC definir loop types. Defer Phase 5 (comparison). TDD plan concreto com 38+ testes, property testing com proptest. |
| frontend-dev | APPROVE c/ CONDITIONS | Timeline/tool graphs viavel com stack atual. Trajectory tree precisa React Flow (@xyflow/react). Requer: discriminated union event stream via Tauri IPC, batch 100ms no Rust side, gerar TS types com ts-rs/specta (nao manter manual), IPC contract document antes de UI code. |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE | 60-70% da infra ja existe. Gaps reais: (1) trajectory com span-level tracing (alinhar com OTel GenAI conventions), (2) result-aware loop detection (Zeroclaw pattern: tool+args+output hash, two-tier escalation N=3 prompt / N=5 hard stop), (3) failure taxonomy (MAST: 14 modos, Theo cobre 2 de 14, adicionar 4 mais relevantes). Terminal-Bench 2.0 usa GPT-5 como juiz. SWE-Bench Pro limita 50 turns/$2. Anthropic tracks p99.9 turn duration. Recomenda: compor existente, estender FailurePattern para 6 modos, alinhar nomes com OTel sem dependencia. |

---

## Conflitos Identificados

### Conflito 1: Novo crate vs modulo dentro do runtime

**A favor de novo crate:** arch-validator, chief-architect, linter, graphctx-expert
**A favor de modulo:** evolution-agent, data-ingestor

**Resolucao:** Comecar como **modulo `observability/`** dentro de `theo-agent-runtime` (evolution-agent + data-ingestor). O runtime ja possui `observability.rs`, `metrics.rs`, `context_metrics.rs` — consolidar la. Promover a crate separado **apenas quando** `theo-application` ou `theo-cli` precisarem consumir diretamente (Rule of 3). Isso respeita YAGNI e evita proliferacao de crates prematura. arch-validator confirma que o modulo interno nao viola boundaries.

### Conflito 2: Tipos em theo-domain vs crate proprio

**A favor de theo-domain:** arch-validator, knowledge-compiler, memory-synthesizer
**Contra theo-domain:** chief-architect, ontology-manager, data-ingestor, wiki-expert

**Resolucao:** **Tipos de dados puros ficam em theo-domain** (apenas identifiers e traits minimos: `TrajectoryId`, trait `TrajectorySignal`). **Tipos computacionais/compostos ficam no modulo observability** dentro do runtime. Decomposicao obrigatoria: nao criar `ObservabilityMetrics` god-struct — usar `ToolExecutionMetrics`, `LoopDetectionResult`, `EfficiencyReport` separados (ontology-manager). `TrajectoryStep` **nao sera criado** — usar projection layer sobre `DomainEvent` + `ToolCallRecord` existentes (ontology-manager + data-ingestor).

### Conflito 3: Loop detection — onde mora?

**No modulo observability:** proposta original, test-runner
**No runtime (theo-agent-runtime):** wiki-expert

**Resolucao:** Loop detection **permanece no runtime** (ja existe em `reflector.rs` e `context_metrics.rs`). O modulo observability **consome e reporta** resultados de loop detection, nao re-implementa. Estender `HeuristicReflector` com result-aware detection (research-agent: hash de tool+args+output, two-tier escalation). Observability captura `LoopDetected` events emitidos pelo runtime.

### Conflito 4: Sync I/O no listener

**Consenso absoluto:** chief-architect, validator, code-reviewer — sync I/O bloqueia tokio, e inaceitavel.

**Resolucao:** **Channel + background OS thread** e OBRIGATORIO. `on_event` faz apenas `sender.try_send(serialized_bytes)` (O(1)). Background thread drena channel e escreve com BufWriter. Pattern ja existe em `BroadcastListener` (event_bus.rs:164-173). Serializar `&DomainEvent` para bytes em on_event (evita clone de serde_json::Value).

---

## Consenso

1. **Conceito aprovado** — observability engine e necessario e estrategicamente valioso (16/16 concordam)
2. **Compor, nao substituir** — consolidar MetricsCollector + ContextMetrics + StructuredLogListener + HeuristicReflector (10/16 explicitos)
3. **Channel + background thread** — zero I/O no hot path (unanime entre os que analisaram)
4. **TDD obrigatorio** — RED primeiro, sem excecao (unanime)
5. **Replay/comparison e P2** — capturar e computar primeiro, replay depois (chief-architect, evolution-agent, test-runner)
6. **ADR obrigatorio antes de codigo** — documenta decisoes de boundary, escopo, storage
7. **Schema versioning desde dia 1** — `schema_version` em todo artefato persistido (memory-synthesizer, linter)
8. **Alinhar nomes com OTel GenAI conventions** — sem dependencia de crate OTel, apenas nomenclatura compativel (research-agent)
9. **Estender failure taxonomy** — de 2 para 6 modos MAST relevantes (research-agent)
10. **ts-rs/specta para gerar tipos TypeScript** — nao manter tipos paralelos (frontend-dev)

---

## Decisoes

1. **Modulo, nao crate.** Criar `crates/theo-agent-runtime/src/observability/` como diretorio de modulos. Consolida `observability.rs`, `metrics.rs`, `context_metrics.rs` existentes + novas funcionalidades.

2. **Tipos minimos em theo-domain.** Apenas: `TrajectoryId(String)` newtype. Nenhum struct composto. Projection layer no modulo observability reconstroi trajectory de DomainEvent + ToolCallRecord.

3. **Channel architecture obrigatoria.** `ObservabilityListener` implementa `EventListener` com `mpsc::SyncSender<Vec<u8>>`. Background OS thread drena e escreve JSONL com sequence numbers e fsync periodico.

4. **Event filter set definido.** Trajectory events: `RunInitialized`, `RunStateChanged`, `TaskCreated`, `TaskStateChanged`, `ToolCallQueued`, `ToolCallDispatched`, `ToolCallCompleted`, `LlmCallStart`, `LlmCallEnd`, `BudgetExceeded`, `Error`, `HypothesisFormed`, `HypothesisInvalidated`, `DecisionMade`, `ConstraintLearned`, `SensorExecuted`, `ContextOverflowRecovery`, `RetrievalExecuted`. Excluidos do trajectory: `ContentDelta`, `ReasoningDelta`, `ToolCallProgress`, `TodoUpdated`.

5. **Loop detection estendido no runtime.** Estender `HeuristicReflector` com result-aware detection: hash(tool_name, args_normalized, output_truncated_hash). Two-tier: inject corrective prompt at N=3, hard stop at N=5. RFC obrigatorio antes de implementar.

6. **Failure taxonomy estendida.** Adicionar 4 modos MAST ao `FailurePattern`: `PrematureTermination`, `WeakVerification`, `TaskDerailment`, `ConversationHistoryLoss`. Cada modo requer sensor concreto antes de ser adicionado.

7. **5 metricas v1 derivadas.**
   - `doom_loop_frequency`: tool calls com hash identico em sliding window / total tool calls
   - `llm_efficiency`: tool calls uteis (success + distintos) / total LLM calls
   - `context_waste_ratio`: ContextOverflowRecovery events / total iterations
   - `hypothesis_churn_rate`: HypothesisInvalidated / HypothesisFormed por run
   - `time_to_first_tool`: delta entre RunInitialized e primeiro ToolCallDispatched

8. **Storage: JSONL por run, com schema_version.** Path: `.theo/trajectories/{run_id}.jsonl`. Cada linha tem `schema_version`, `sequence_number`, `event_type`. Subsumir StructuredLogListener — um unico writer.

9. **Frontend: IPC contract antes de UI code.** Discriminated union via Tauri emit. Batch 100ms no Rust side. Timeline view primeiro (menor complexidade). Trajectory tree com React Flow (P2).

10. **Replay/comparison e P2.** Nao implementar ate ter 50+ trajectories reais para validar design.

---

## Plano TDD

### Phase 1: Data Plumbing (RED tests possiveis HOJE)

1. **RED**: `test_trajectory_projection_from_domain_events()` — dada sequencia de DomainEvents, projection layer produz trajectory struct correta
2. **RED**: `test_trajectory_serializes_to_jsonl()` — roundtrip serde
3. **RED**: `test_empty_run_produces_valid_trajectory()` — edge case
4. **GREEN**: Structs com `#[derive(Serialize, Deserialize)]`, projection function
5. **REFACTOR**: Consolidar imports, remover duplicacao com ContextMetrics existente
6. **VERIFY**: `cargo test -p theo-agent-runtime`

### Phase 2: Channel + Background Writer (RED tests com channel)

1. **RED**: `test_observability_listener_does_not_block_event_publish()` — medir latencia de on_event < 1ms
2. **RED**: `test_jsonl_file_created_per_run()` — publish RunInitialized, verificar arquivo
3. **RED**: `test_sequence_numbers_are_monotonic()` — parse JSONL, verificar seq
4. **RED**: `test_schema_version_present_in_every_line()` — parse JSONL
5. **RED**: `test_concurrent_runs_write_to_separate_files()` — 5 threads
6. **GREEN**: ObservabilityListener com mpsc + background thread
7. **REFACTOR**: Subsumir StructuredLogListener
8. **VERIFY**: `cargo test -p theo-agent-runtime`

### Phase 3: Metricas Derivadas (RED tests com formulas)

1. **RED**: `test_doom_loop_frequency_with_known_input()` — 3 tool calls identicos em 10 total = 0.3
2. **RED**: `test_llm_efficiency_perfect_run()` — todas tool calls distintas e success = 1.0
3. **RED**: `test_context_waste_ratio_zero_overflows()` — 0 overflows = 0.0
4. **RED**: `test_hypothesis_churn_rate_no_invalidations()` — 0/5 = 0.0
5. **RED**: `test_time_to_first_tool_with_known_timestamps()` — delta exato
6. **GREEN**: Funcoes de computacao por metrica
7. **REFACTOR**: Extrair MetricsComputer trait
8. **VERIFY**: `cargo test -p theo-agent-runtime`

### Phase 4: Loop Detection Estendido (BLOCKED ate RFC)

1. RFC define: tipos de loop, thresholds, whitelist de repeticoes esperadas
2. **RED**: testes por tipo de loop conforme RFC
3. **GREEN**: estender HeuristicReflector
4. **VERIFY**: `cargo test -p theo-agent-runtime`

### Phase 5: Failure Taxonomy (apos Phase 3)

1. **RED**: `test_premature_termination_detected()` — run converge com 0 edits
2. **RED**: `test_weak_verification_detected()` — edit sem sensor subsequente
3. **GREEN**: novos FailurePattern variants + sensors
4. **VERIFY**: `cargo test -p theo-agent-runtime`

---

## Action Items

- [ ] **Paulo** — Escrever ADR `docs/adr/XXX-agent-observability-engine.md` documentando decisoes desta reuniao — **antes de qualquer codigo**
- [ ] **Paulo** — Criar diretorio `crates/theo-agent-runtime/src/observability/` e migrar `observability.rs`, `metrics.rs`, `context_metrics.rs` como submodulos — **Sprint 1**
- [ ] **Paulo** — Implementar Phase 1 TDD (data plumbing: projection layer + serde) — **Sprint 1**
- [ ] **Paulo** — Implementar Phase 2 TDD (channel + background writer) — **Sprint 1**
- [ ] **Paulo** — Implementar Phase 3 TDD (5 metricas derivadas) — **Sprint 2**
- [ ] **Paulo** — Escrever RFC de Loop Detection (tipos, thresholds, whitelist) — **Sprint 2**
- [ ] **Paulo** — Implementar Phase 4 TDD (loop detection estendido) — **Sprint 3, apos RFC aprovado**
- [ ] **Paulo** — Implementar Phase 5 TDD (failure taxonomy 6 modos MAST) — **Sprint 3**
- [ ] **Paulo** — Definir IPC contract para frontend (`apps/theo-ui/src/features/observability/types.ts`) — **Sprint 3**
- [ ] **Paulo** — Avaliar promocao a crate separado apos 50+ trajectories reais — **Sprint 4+**

---

## Veredito Final

**REVISED**: Conceito aprovado por unanimidade (16/16), mas com modificacoes substanciais:
1. Modulo interno ao runtime (nao crate separado) — YAGNI ate haver consumidor externo
2. Tipos minimos em theo-domain (apenas TrajectoryId) — projection layer em vez de TrajectoryStep
3. Channel + background thread obrigatorio — zero I/O no hot path
4. Loop detection RFC obrigatorio antes de Phase 4
5. Replay/comparison e P2
6. Subsumir StructuredLogListener existente
7. Alinhar com MAST failure taxonomy (6 de 14 modos) e OTel GenAI naming conventions
