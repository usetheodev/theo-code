# Roadmap: Context Manager para Long-Running Agents

> De code intelligence pipeline (8/10) para context manager robusto.
> Baseado na reuniao 20260409-083445 + review do Paulo.
> Cada task tem: descricao, arquivo(s), criterio de aceite, DoD, e TDD plan.

---

## Estado Atual (Baseline)

| Componente | Arquivo | Estado |
|---|---|---|
| State Machine | `agent_run.rs` | 8 states, validated transitions |
| RunSnapshot | `snapshot.rs:19-32` | 10 campos, checksum, **SEM schema_version** |
| SnapshotStore | `persistence.rs:26-32` | 4 methods (save/load/list/delete) |
| DomainEvent | `event.rs:6-33` | 14 EventType variants |
| EventBus | `event_bus.rs:25-29` | Bounded FIFO (10k), panic-safe |
| BudgetEnforcer | `budget_enforcer.rs:15-23` | tokens + iterations + tool_calls + time |
| Compaction | `compaction.rs` | Message truncation, 80% threshold, idempotent |
| AgentMemoryEntry | `memory/mod.rs:22-28` | 4 campos (key, value, created_at, run_id) |
| RuntimeInsight | `wiki/model.rs:524-537` | 12 campos, append-only JSONL |
| content_hash | `graph_context_service.rs:983-1031` | **USA MTIME, NAO CONTEUDO** |
| AgentState | `state.rs:34-43` | DEPRECATED mas usado em run_engine.rs |

### Violacoes de Fronteira Confirmadas

| # | Crate | Dependencia Ilegal | Arquivo:Linha |
|---|---|---|---|
| 1 | theo-governance | theo-engine-graph | `crates/theo-governance/Cargo.toml:8` |
| 2 | theo-agent-runtime | theo-tooling | `crates/theo-agent-runtime/Cargo.toml:8` |
| 3 | theo-agent-runtime | theo-infra-auth | `crates/theo-agent-runtime/Cargo.toml:16` |
| 4 | theo-cli | theo-tooling, theo-infra-* | `apps/theo-cli/Cargo.toml:10-12` |
| 5 | theo-desktop | theo-infra-*, theo-tooling | `apps/theo-desktop/Cargo.toml:15-17` |

### Cobertura de Testes

| Crate | Testes | Status |
|---|---|---|
| theo-engine-graph | 86 | OK |
| theo-engine-retrieval | 242 | OK |
| theo-agent-runtime | 284 | OK |
| theo-domain | 174 | OK |
| theo-governance | 75 | OK |
| **theo-application** | **18** | **GAP CRITICO** |
| graph_context_service.rs | 10 (1234 LoC) | **INSUFICIENTE** |

---

## Sprint 0 — Fundacoes (BLOQUEANTE)

> Nada avanca ate que Sprint 0 esteja 100% completo.
> Criterio global: `cargo test` passa, `cargo clippy` sem warnings.

---

### S0-T1: blake3 content hash

**Problema**: `compute_project_hash()` em `graph_context_service.rs:983-1031` usa `mtime` para hash. Arquivo tocado sem mudanca de conteudo invalida cache. Arquivo restaurado com mesmo conteudo nao e detectado.

**Arquivos**:
- `crates/theo-application/src/use_cases/graph_context_service.rs` (linhas 983-1031)
- `crates/theo-domain/Cargo.toml` (adicionar blake3)
- `Cargo.toml` raiz (workspace.dependencies)

**Microtasks**:

1. **S0-T1.1**: Adicionar `blake3` ao workspace dependencies
   - Editar `Cargo.toml` raiz: `blake3 = "1"`
   - Editar `crates/theo-domain/Cargo.toml`: `blake3.workspace = true`
   - DoD: `cargo check` passa

2. **S0-T1.2**: RED — Escrever testes que falham
   ```rust
   #[test]
   fn content_hash_stable_when_mtime_changes_but_content_identical() {
       // Arrange: criar arquivo temporario, computar hash
       // Act: touch arquivo (mtime muda, conteudo igual), recomputar
       // Assert: hashes iguais — FALHA hoje (usa mtime)
   }

   #[test]
   fn content_hash_differs_when_content_changes() {
       // Arrange: criar arquivo, computar hash
       // Act: modificar conteudo, recomputar
       // Assert: hashes diferentes
   }

   #[test]
   fn content_hash_uses_mtime_as_fast_precheck() {
       // Arrange: dois arquivos identicos em conteudo mas mtimes diferentes
       // Act: computar hash
       // Assert: hash identico (mtime nao influencia resultado final)
   }
   ```
   - DoD: 3 testes escritos, 2 falham (o terceiro pode passar dependendo da impl)

3. **S0-T1.3**: GREEN — Implementar blake3 content hash
   - Substituir logica de mtime em `compute_project_hash()` por:
     ```
     para cada arquivo:
       se mtime == cached_mtime → usar hash cached (fast path)
       senao → blake3::hash(file_bytes), atualizar cache
     hash final = blake3::hash(sorted entries de (path, content_hash))
     ```
   - DoD: 3 testes passam

4. **S0-T1.4**: REFACTOR — Extrair trait
   - Criar `ContentHasher` trait em `theo-domain`
   - Impl `Blake3ContentHasher` em `theo-application`
   - DoD: testes passam, `cargo clippy` limpo

**Criterio de Aceite**:
- [x] `compute_project_hash()` retorna hash identico para arquivos com conteudo identico independente de mtime
- [x] `compute_project_hash()` retorna hash diferente quando conteudo muda
- [x] mtime serve apenas como pre-filtro (skip re-hash se mtime nao mudou)
- [x] 3+ testes especificos passando
- [x] Nenhum teste existente quebrado

**DoD**: PR com testes + implementacao + review. `cargo test -p theo-application` verde.

---

### S0-T2: schema_version em RunSnapshot

**Problema**: `RunSnapshot` em `snapshot.rs:19-32` nao tem `schema_version`. Binario novo deserializando snapshot antigo pode corromper silenciosamente.

**Arquivos**:
- `crates/theo-agent-runtime/src/snapshot.rs` (struct, linhas 19-32)
- `crates/theo-agent-runtime/src/persistence.rs` (load, linhas 78-84)

**Microtasks**:

1. **S0-T2.1**: RED — Testes de versionamento
   ```rust
   #[test]
   fn snapshot_has_schema_version() {
       let snap = RunSnapshot::new(...);
       assert_eq!(snap.schema_version, RunSnapshot::CURRENT_VERSION);
   }

   #[test]
   fn snapshot_rejects_unknown_schema_version() {
       let mut snap = create_test_snapshot();
       snap.schema_version = 999;
       let json = serde_json::to_string(&snap).unwrap();
       let store = FileSnapshotStore::new(tmp_dir);
       // save raw json, then load
       let result = store.load(&run_id).await;
       assert!(matches!(result, Err(PersistenceError::SchemaVersionMismatch { .. })));
   }

   #[test]
   fn snapshot_v1_deserializes_to_current() {
       // Snapshot sem schema_version (v0 implicito) deve migrar para v1
       let legacy_json = r#"{"run": ..., "checksum": "..."}"#; // sem schema_version
       let snap: RunSnapshot = serde_json::from_str(legacy_json).unwrap();
       assert_eq!(snap.schema_version, 0); // default para legacy
   }
   ```
   - DoD: 3 testes escritos, todos falham

2. **S0-T2.2**: GREEN — Adicionar campo + validacao
   - Adicionar `pub schema_version: u32` a `RunSnapshot` com `#[serde(default)]`
   - Constante `CURRENT_VERSION: u32 = 1`
   - Em `persistence.rs::load()`: apos deserializar, checar `schema_version <= CURRENT_VERSION`
   - Adicionar variante `SchemaVersionMismatch { found: u32, expected: u32 }` a `PersistenceError`
   - Incluir `schema_version` no `SnapshotHashable` para que checksum cubra a versao
   - DoD: 3 testes passam

3. **S0-T2.3**: REFACTOR — Migration path
   - `schema_version == 0` (legacy, sem campo) → migrar automaticamente para v1 (preencher defaults)
   - Documentar: futuras mudancas incrementam CURRENT_VERSION e adicionam migration fn
   - DoD: testes passam, snapshots legados continuam funcionando

**Criterio de Aceite**:
- [x] RunSnapshot serializa/deserializa com schema_version
- [x] Snapshots legados (sem campo) carregam com version=0 e migram
- [x] Version > CURRENT_VERSION retorna erro tipado
- [x] Checksum inclui schema_version
- [x] 3+ testes especificos

**DoD**: `cargo test -p theo-agent-runtime -- snapshot` verde. `cargo test -p theo-agent-runtime -- persistence` verde.

---

### S0-T3: Corrigir violacoes de fronteira

**Problema**: 5 violacoes de dependency rules confirmadas.

**Microtasks**:

1. **S0-T3.1**: theo-governance → remover theo-engine-graph
   - Arquivo: `crates/theo-governance/Cargo.toml:8`
   - Acao: mover os tipos necessarios de engine-graph para theo-domain, ou re-exportar via theo-application
   - DoD: `cargo check -p theo-governance` compila sem theo-engine-graph

2. **S0-T3.2**: theo-agent-runtime → remover theo-tooling
   - Arquivo: `crates/theo-agent-runtime/Cargo.toml:8`
   - Acao: extrair interface (trait) para theo-domain, injetar implementacao via theo-application
   - DoD: `cargo check -p theo-agent-runtime` compila sem theo-tooling

3. **S0-T3.3**: theo-agent-runtime → remover theo-infra-auth
   - Arquivo: `crates/theo-agent-runtime/Cargo.toml:16`
   - Acao: injetar auth via trait em theo-domain
   - DoD: `cargo check -p theo-agent-runtime` compila sem theo-infra-auth

4. **S0-T3.4**: theo-cli → remover imports diretos de infra/tooling
   - Arquivo: `apps/theo-cli/Cargo.toml:10-12`
   - Acao: acessar via theo-application (re-exports ou facade)
   - DoD: `cargo check -p theo-cli` compila apenas com theo-application + theo-api-contracts

5. **S0-T3.5**: theo-desktop → remover imports diretos de infra/tooling
   - Arquivo: `apps/theo-desktop/Cargo.toml:15-17`
   - Acao: mesmo pattern de theo-cli
   - DoD: `cargo check -p theo-desktop` compila apenas com theo-application + theo-api-contracts

**Criterio de Aceite Global**:
- [x] Nenhum crate viola as regras de dependencia documentadas em CLAUDE.md
- [x] `cargo check` do workspace inteiro compila
- [x] `cargo test` do workspace inteiro passa
- [x] Nenhuma dependencia circular introduzida

**DoD**: `cargo test` verde. Dependency graph validado por arch-validator.

---

### S0-T4: Elevar testes de theo-application

**Problema**: `graph_context_service.rs` tem 1234 LoC e 10 testes. Cobertura insuficiente para construir sobre.

**Arquivos**:
- `crates/theo-application/src/use_cases/graph_context_service.rs`
- `crates/theo-application/src/use_cases/pipeline.rs`

**Microtasks**:

1. **S0-T4.1**: Testes de erro e recuperacao
   ```rust
   #[tokio::test]
   async fn initialize_timeout_transitions_to_failed() { ... }

   #[tokio::test]
   async fn query_after_failed_build_returns_error() { ... }

   #[tokio::test]
   async fn double_initialize_concurrent_is_safe() { ... }
   ```
   - DoD: 3 testes de erro escritos e passando

2. **S0-T4.2**: Testes de cache
   ```rust
   #[tokio::test]
   async fn cache_hit_skips_rebuild() { ... }

   #[tokio::test]
   async fn cache_miss_triggers_full_build() { ... }

   #[tokio::test]
   async fn stale_cache_served_during_rebuild() { ... }
   ```
   - DoD: 3 testes de cache escritos e passando

3. **S0-T4.3**: Testes de pipeline end-to-end
   ```rust
   #[tokio::test]
   async fn pipeline_extract_build_cluster_query_roundtrip() { ... }

   #[tokio::test]
   async fn pipeline_incremental_update_preserves_communities() { ... }
   ```
   - DoD: 2 testes E2E escritos e passando

4. **S0-T4.4**: Testes de wiki generation trigger
   ```rust
   #[tokio::test]
   async fn wiki_generated_when_graph_hash_changes() { ... }

   #[tokio::test]
   async fn wiki_skipped_when_graph_hash_unchanged() { ... }
   ```
   - DoD: 2 testes de wiki trigger escritos e passando

**Criterio de Aceite**:
- [x] graph_context_service.rs tem >= 20 testes
- [x] Cenarios cobertos: happy path, erro, timeout, cache hit/miss, concorrencia, wiki trigger
- [x] Todos os metodos publicos tem pelo menos 1 teste

**DoD**: `cargo test -p theo-application` verde com 20+ testes.

---

### S0 — Gate de Saida

| Criterio | Verificacao |
|---|---|
| content_hash usa blake3 | `cargo test -p theo-application -- content_hash` |
| RunSnapshot tem schema_version | `cargo test -p theo-agent-runtime -- schema_version` |
| 0 violacoes de fronteira | `cargo check` + arch-validator |
| theo-application >= 20 testes | `cargo test -p theo-application` |
| Workspace verde | `cargo test` (todos) |
| Clippy limpo | `cargo clippy -- -D warnings` |

---

## Sprint 0.5 — Instrumentacao de Contexto

> Medir antes de projetar. Sem dados, schema e especulacao.

---

### S05-T1: Metricas de context breakdown

**Objetivo**: Instrumentar o agent runtime para coletar dados sobre onde o contexto quebra em runs >20 iteracoes.

**Arquivos**:
- `crates/theo-agent-runtime/src/run_engine.rs`
- `crates/theo-agent-runtime/src/metrics.rs` (novo ou existente)

**Microtasks**:

1. **S05-T1.1**: RED — Testes de coleta de metricas
   ```rust
   #[test]
   fn metrics_tracks_context_size_per_iteration() {
       let metrics = ContextMetrics::new();
       metrics.record_context_size(1, 3500); // iteration 1, 3500 tokens
       metrics.record_context_size(2, 4200);
       assert_eq!(metrics.avg_context_size(), 3850);
   }

   #[test]
   fn metrics_detects_refetch_of_same_artifact() {
       let metrics = ContextMetrics::new();
       metrics.record_artifact_fetch("src/auth.rs", 1);
       metrics.record_artifact_fetch("src/auth.rs", 5); // mesmo arquivo, iteracao 5
       assert_eq!(metrics.refetch_count("src/auth.rs"), 2);
   }

   #[test]
   fn metrics_counts_action_repetitions() {
       let metrics = ContextMetrics::new();
       metrics.record_action("search: auth flow", 1);
       metrics.record_action("search: auth flow", 4); // mesma busca
       assert_eq!(metrics.repeated_actions(), vec!["search: auth flow"]);
   }
   ```
   - DoD: 3 testes escritos e falhando

2. **S05-T1.2**: GREEN — Implementar ContextMetrics
   ```rust
   pub struct ContextMetrics {
       context_sizes: Vec<(usize, usize)>,     // (iteration, tokens)
       artifact_fetches: HashMap<String, Vec<usize>>, // path → [iterations]
       actions: HashMap<String, Vec<usize>>,    // action → [iterations]
       hypothesis_changes: Vec<(usize, String)>, // (iteration, description)
   }
   ```
   - Metodos: `record_context_size`, `record_artifact_fetch`, `record_action`, `record_hypothesis_change`
   - Queries: `avg_context_size`, `refetch_count`, `repeated_actions`, `hypothesis_loss_rate`
   - DoD: todos os testes passam

3. **S05-T1.3**: Integrar com run_engine.rs
   - Adicionar `context_metrics: ContextMetrics` ao `AgentRunEngine`
   - Hook em cada iteracao do loop principal:
     - Apos montar contexto: `record_context_size(iteration, tokens)`
     - Apos tool call de read/search: `record_artifact_fetch(path, iteration)`
     - Apos cada acao: `record_action(description, iteration)`
   - DoD: `cargo test -p theo-agent-runtime` verde

4. **S05-T1.4**: Report de metricas ao final da run
   - Ao convergir ou abortar: gerar `ContextMetricsReport`
   - Persistir em `.theo/metrics/{run_id}.json`
   - Campos: avg_context_size, max_context_size, refetch_rate, action_repetition_rate, total_iterations
   - DoD: report gerado em testes de integracao

**Criterio de Aceite**:
- [x] Metricas coletadas automaticamente durante agent runs
- [x] Report JSON gerado ao final de cada run
- [x] 5 metricas minimas: context_size_avg, context_size_max, refetch_rate, action_repetition_rate, hypothesis_changes
- [x] Zero overhead mensuravel em runs < 50 iteracoes (< 1ms por iteracao)

**DoD**: `cargo test -p theo-agent-runtime -- metrics` verde. Report legivel por humanos.

---

### S05 — Gate de Saida

| Criterio | Verificacao |
|---|---|
| ContextMetrics struct testado | `cargo test -- context_metrics` |
| Integrado com run_engine | Rodar 1 agent run, verificar report em .theo/metrics/ |
| Metricas coletadas | Report JSON com 5+ campos preenchidos |

---

## Sprint 1 — Tipos e Memoria Episodica

> Estender tipos existentes. Unico tipo novo: EpisodeSummary.

---

### S1-T1: Novos EventType variants cognitivos

**Problema**: DomainEvent tem 14 variants (event.rs:6-33), todas operacionais. Faltam variants para raciocinio do agente.

**Arquivos**:
- `crates/theo-domain/src/event.rs` (linhas 6-33, enum EventType)

**Microtasks**:

1. **S1-T1.1**: RED — Testes de invariantes causais
   ```rust
   #[test]
   fn hypothesis_formed_requires_rationale() {
       let payload = json!({"hypothesis": "auth bug in jwt.rs"});
       // Sem rationale → deve falhar validacao
       assert!(validate_cognitive_event(EventType::HypothesisFormed, &payload).is_err());

       let valid = json!({"hypothesis": "auth bug in jwt.rs", "rationale": "test_verify fails"});
       assert!(validate_cognitive_event(EventType::HypothesisFormed, &valid).is_ok());
   }

   #[test]
   fn hypothesis_invalidated_must_reference_prior() {
       let payload = json!({"prior_event_id": "evt-123", "reason": "test passed after revert"});
       assert!(validate_cognitive_event(EventType::HypothesisInvalidated, &payload).is_ok());

       let missing_ref = json!({"reason": "test passed"});
       assert!(validate_cognitive_event(EventType::HypothesisInvalidated, &missing_ref).is_err());
   }

   #[test]
   fn decision_made_carries_choice_and_evidence() {
       let payload = json!({
           "choice": "rewrite verify_token",
           "alternatives_considered": ["patch", "rewrite"],
           "evidence_refs": ["evt-100", "evt-102"]
       });
       assert!(validate_cognitive_event(EventType::DecisionMade, &payload).is_ok());
   }

   #[test]
   fn constraint_learned_has_scope() {
       let payload = json!({"constraint": "no unwrap in auth", "scope": "workspace-local"});
       assert!(validate_cognitive_event(EventType::ConstraintLearned, &payload).is_ok());

       let no_scope = json!({"constraint": "no unwrap in auth"});
       assert!(validate_cognitive_event(EventType::ConstraintLearned, &no_scope).is_err());
   }
   ```
   - DoD: 4 testes escritos e falhando

2. **S1-T1.2**: GREEN — Adicionar variants + validacao
   - Adicionar ao enum EventType:
     ```rust
     HypothesisFormed,
     HypothesisInvalidated,
     DecisionMade,
     ConstraintLearned,
     ```
   - Adicionar `ALL_EVENT_TYPES` (atualizar array, linhas 57-72)
   - Criar `pub fn validate_cognitive_event(event_type: EventType, payload: &Value) -> Result<(), EventValidationError>`
   - Invariantes:
     - HypothesisFormed: payload DEVE ter "hypothesis" + "rationale"
     - HypothesisInvalidated: payload DEVE ter "prior_event_id" + "reason"
     - DecisionMade: payload DEVE ter "choice" + "evidence_refs"
     - ConstraintLearned: payload DEVE ter "constraint" + "scope" (run-local | task-local | workspace-local)
   - DoD: 4 testes passam

3. **S1-T1.3**: REFACTOR — Scope enum tipado
   - Criar `ConstraintScope` enum em vez de string livre
   - Garantir que `serde` roundtrip funciona
   - DoD: testes passam, clippy limpo

**Criterio de Aceite**:
- [x] 4 novos EventType variants
- [x] Validacao obrigatoria para eventos cognitivos (payload checado)
- [x] Backward-compatible (variantes antigas nao afetadas)
- [x] ALL_EVENT_TYPES atualizado
- [x] 4+ testes de invariantes causais

**DoD**: `cargo test -p theo-domain -- event` verde.

---

### S1-T2: EpisodeSummary (unico tipo novo)

**Problema**: Nao existe forma de compactar uma sequencia de eventos em um resumo estruturado reutilizavel.

**Arquivos**:
- `crates/theo-domain/src/episode.rs` (novo)
- `crates/theo-domain/src/lib.rs` (re-export)

**Microtasks**:

1. **S1-T2.1**: RED — Testes de criacao e validacao
   ```rust
   #[test]
   fn episode_summary_created_from_event_window() {
       let events = vec![evt1, evt2, evt3]; // DomainEvents
       let summary = EpisodeSummary::from_events("run-1", &events);
       assert_eq!(summary.run_id, "run-1");
       assert_eq!(summary.evidence_event_ids.len(), 3);
       assert!(summary.schema_version > 0);
   }

   #[test]
   fn episode_summary_machine_part_has_structured_fields() {
       let summary = create_test_summary();
       assert!(!summary.machine_summary.key_actions.is_empty());
       assert!(!summary.machine_summary.outcome.is_empty());
   }

   #[test]
   fn episode_summary_serde_roundtrip() {
       let summary = create_test_summary();
       let json = serde_json::to_string(&summary).unwrap();
       let restored: EpisodeSummary = serde_json::from_str(&json).unwrap();
       assert_eq!(summary.summary_id, restored.summary_id);
       assert_eq!(summary.schema_version, restored.schema_version);
   }
   ```
   - DoD: 3 testes escritos e falhando

2. **S1-T2.2**: GREEN — Implementar structs
   ```rust
   pub struct EpisodeSummary {
       pub summary_id: String,
       pub run_id: String,
       pub task_id: Option<String>,
       pub window_start_event_id: String,
       pub window_end_event_id: String,
       pub machine_summary: MachineEpisodeSummary,
       pub human_summary: Option<String>,
       pub evidence_event_ids: Vec<String>,
       pub affected_files: Vec<String>,
       pub open_questions: Vec<String>,
       pub unresolved_hypotheses: Vec<String>,
       pub supersedes_summary_id: Option<String>,
       pub schema_version: u32,
       pub created_at: u64,
   }

   pub struct MachineEpisodeSummary {
       pub objective: String,
       pub key_actions: Vec<String>,
       pub outcome: String,  // "success" | "failure" | "partial" | "inconclusive"
       pub successful_steps: Vec<String>,
       pub failed_attempts: Vec<String>,
       pub learned_constraints: Vec<String>,
       pub files_touched: Vec<String>,
   }
   ```
   - Constante `CURRENT_SCHEMA_VERSION: u32 = 1`
   - `from_events(run_id, events)` — construtor deterministico
   - DoD: 3 testes passam

3. **S1-T2.3**: REFACTOR — TtlPolicy
   - Criar `TtlPolicy` enum: `RunScoped`, `TimeScoped(Duration)`, `Permanent`
   - Adicionar campo `ttl_policy: TtlPolicy` com default `RunScoped`
   - DoD: testes passam, serde roundtrip funciona

**Criterio de Aceite**:
- [x] EpisodeSummary com separacao machine/human
- [x] schema_version desde o dia 1
- [x] supersedes_summary_id para trilha minima de evolucao
- [x] Construivel a partir de Vec<DomainEvent>
- [x] Serde roundtrip funcional
- [x] 5+ testes

**DoD**: `cargo test -p theo-domain -- episode` verde.

---

### S1-T3: WorkingSet como campo em RunSnapshot

**Problema**: RunSnapshot nao tem nocao de "contexto ativo" — qual informacao esta quente para a tarefa corrente.

**Arquivos**:
- `crates/theo-agent-runtime/src/snapshot.rs` (struct, linhas 19-32)
- `crates/theo-domain/src/working_set.rs` (novo)

**Microtasks**:

1. **S1-T3.1**: RED — Testes
   ```rust
   #[test]
   fn working_set_included_in_snapshot() {
       let ws = WorkingSet {
           hot_files: vec!["src/auth.rs".into()],
           recent_event_ids: vec!["evt-1".into()],
           active_hypothesis: Some("jwt decode bug".into()),
           current_plan_step: Some("run tests".into()),
       };
       let snap = RunSnapshot::new(..., Some(ws.clone()));
       assert_eq!(snap.working_set.unwrap().hot_files, ws.hot_files);
   }

   #[test]
   fn working_set_survives_serde_roundtrip() {
       let snap = create_snapshot_with_working_set();
       let json = serde_json::to_string(&snap).unwrap();
       let restored: RunSnapshot = serde_json::from_str(&json).unwrap();
       assert!(restored.working_set.is_some());
   }

   #[test]
   fn working_set_none_for_legacy_snapshots() {
       // Snapshot sem working_set (pre-existente) deserializa com None
       let legacy_json = "..."; // JSON sem campo working_set
       let snap: RunSnapshot = serde_json::from_str(legacy_json).unwrap();
       assert!(snap.working_set.is_none());
   }
   ```
   - DoD: 3 testes escritos e falhando

2. **S1-T3.2**: GREEN — Adicionar WorkingSet
   ```rust
   // theo-domain/src/working_set.rs
   pub struct WorkingSet {
       pub hot_files: Vec<String>,
       pub recent_event_ids: Vec<String>,
       pub active_hypothesis: Option<String>,
       pub current_plan_step: Option<String>,
       pub constraints: Vec<String>,
   }
   ```
   - Adicionar `#[serde(default)] pub working_set: Option<WorkingSet>` ao RunSnapshot
   - Incluir no SnapshotHashable para checksum
   - Incrementar CURRENT_VERSION para 2
   - DoD: 3 testes passam

3. **S1-T3.3**: Integrar com run_engine.rs
   - Ao criar snapshot: popular working_set a partir do estado atual
   - Ao restaurar: carregar working_set de volta
   - DoD: `cargo test -p theo-agent-runtime` verde

**Criterio de Aceite**:
- [x] WorkingSet e campo Optional em RunSnapshot
- [x] Backward-compatible (snapshots sem working_set carregam com None)
- [x] Checksum inclui working_set
- [x] Survives serde roundtrip
- [x] 3+ testes

**DoD**: `cargo test -p theo-agent-runtime -- snapshot` verde. `cargo test -p theo-agent-runtime -- persistence` verde.

---

### S1-T4: supersedes_event_id em DomainEvent

**Problema**: Sem trilha minima de substituicao, eventos cognitivos acumulam contradicao.

**Arquivo**: `crates/theo-domain/src/event.rs`

**Microtasks**:

1. **S1-T4.1**: RED
   ```rust
   #[test]
   fn domain_event_supports_supersedes() {
       let evt = DomainEvent::new_with_supersedes(
           EventType::HypothesisInvalidated,
           "run-1",
           json!({"reason": "test passed"}),
           Some(EventId::from("evt-original")),
       );
       assert_eq!(evt.supersedes_event_id.unwrap().to_string(), "evt-original");
   }

   #[test]
   fn domain_event_supersedes_none_by_default() {
       let evt = DomainEvent::new(EventType::TaskCreated, "run-1", json!({}));
       assert!(evt.supersedes_event_id.is_none());
   }
   ```
   - DoD: 2 testes escritos e falhando

2. **S1-T4.2**: GREEN
   - Adicionar `#[serde(default)] pub supersedes_event_id: Option<EventId>` ao DomainEvent
   - Construtor `new_with_supersedes()` + manter `new()` sem supersedes
   - DoD: 2 testes passam

**Criterio de Aceite**:
- [x] Campo optional, backward-compatible
- [x] Serde roundtrip
- [x] 2+ testes

**DoD**: `cargo test -p theo-domain -- event` verde.

---

### S1 — Gate de Saida

| Criterio | Verificacao |
|---|---|
| 4 novos EventType variants com invariantes | `cargo test -p theo-domain -- event` |
| EpisodeSummary com dual machine/human | `cargo test -p theo-domain -- episode` |
| WorkingSet em RunSnapshot | `cargo test -p theo-agent-runtime -- snapshot` |
| supersedes_event_id | `cargo test -p theo-domain -- supersedes` |
| Workspace verde | `cargo test` |
| Metricas S0.5 coletando dados | Verificar .theo/metrics/ apos agent run |

---

## Sprint 2 — Context Assembler Minimo

> Deterministic. Sem magia. Budget enforced.

---

### S2-T1: ContextAssembler minimo

**Problema**: Nao existe compositor que monte o pacote certo de contexto antes de cada acao do agente.

**Arquivo**: `crates/theo-application/src/use_cases/context_assembler.rs` (novo)

**Microtasks**:

1. **S2-T1.1**: RED — Testes de assembly
   ```rust
   #[test]
   fn assembler_respects_token_budget() {
       let assembler = ContextAssembler::new(4000); // 4k tokens
       let result = assembler.assemble(&task, &working_set, &graph_context, &recent_events);
       assert!(result.total_tokens <= 4000);
   }

   #[test]
   fn assembler_always_includes_task_objective() {
       let result = assembler.assemble(&task, &ws, &gc, &events);
       assert!(result.content.contains(&task.objective));
   }

   #[test]
   fn assembler_always_includes_current_step() {
       let ws = WorkingSet { current_plan_step: Some("run tests".into()), .. };
       let result = assembler.assemble(&task, &ws, &gc, &events);
       assert!(result.content.contains("run tests"));
   }

   #[test]
   fn assembler_includes_recent_evidence() {
       let events = vec![recent_failure_event()];
       let result = assembler.assemble(&task, &ws, &gc, &events);
       assert!(result.content.contains("FAILED")); // evidence included
   }

   #[test]
   fn assembler_fills_remaining_budget_with_structural_context() {
       // Apos task + step + events, resto do budget vai para graph context
       let result = assembler.assemble(&task, &ws, &gc, &events);
       assert!(result.structural_blocks.len() > 0);
   }
   ```
   - DoD: 5 testes escritos e falhando

2. **S2-T1.2**: GREEN — Implementar assembler deterministic
   ```rust
   pub struct ContextAssembler {
       token_budget: usize,
   }

   impl ContextAssembler {
       pub fn assemble(
           &self,
           task: &Task,
           working_set: &WorkingSet,
           graph_context: &GraphContextResult,
           recent_events: &[DomainEvent],
       ) -> AssembledContext {
           let mut budget_remaining = self.token_budget;
           let mut sections = Vec::new();

           // 1. SEMPRE: task objective (hard rule)
           sections.push(format_task_objective(task));
           budget_remaining -= estimate_tokens(&sections.last().unwrap());

           // 2. SEMPRE: current step (hard rule)
           if let Some(step) = &working_set.current_plan_step {
               sections.push(format_current_step(step));
               budget_remaining -= estimate_tokens(&sections.last().unwrap());
           }

           // 3. SEMPRE: evidencias recentes (hard rule, limit N)
           let evidence = format_recent_events(recent_events, 8);
           sections.push(evidence);
           budget_remaining -= estimate_tokens(&sections.last().unwrap());

           // 4. Hot files do working set (limit K)
           for f in working_set.hot_files.iter().take(5) {
               // ... include if budget allows
           }

           // 5. Structural context (fill remaining)
           for block in &graph_context.blocks {
               if budget_remaining < block.token_count { break; }
               sections.push(block.content.clone());
               budget_remaining -= block.token_count;
           }

           AssembledContext { sections, total_tokens: self.token_budget - budget_remaining }
       }
   }
   ```
   - DoD: 5 testes passam

3. **S2-T1.3**: REFACTOR — TokenEstimator trait
   - Extrair estimativa de tokens para trait injetavel
   - Default: `chars / 4` (heuristica simples)
   - DoD: testes passam, trait limpo

**Criterio de Aceite**:
- [x] 4 regras hard respeitadas (budget, objective, step, evidence)
- [x] Structural context preenche budget restante
- [x] Nunca excede token budget
- [x] Deterministico (mesmo input → mesmo output)
- [x] 5+ testes

**DoD**: `cargo test -p theo-application -- assembler` verde.

---

### S2-T2: EpisodicCache tier na wiki

**Problema**: Episode summaries nao devem poluir o indice BM25 principal nem competir com paginas Deterministic.

**Arquivos**:
- `crates/theo-engine-retrieval/src/wiki/model.rs` (AuthorityTier enum)
- `crates/theo-engine-retrieval/src/wiki/lookup.rs` (exclusao do BM25)
- `crates/theo-engine-retrieval/src/wiki/persistence.rs` (storage separado)

**Microtasks**:

1. **S2-T2.1**: RED
   ```rust
   #[test]
   fn episodic_cache_tier_exists() {
       let tier = AuthorityTier::EpisodicCache;
       assert_eq!(tier.weight(), 0.4);
   }

   #[test]
   fn episodic_pages_excluded_from_main_bm25() {
       // Criar wiki com pagina Deterministic e EpisodicCache
       // Buscar por termo presente em ambas
       // Assert: apenas Deterministic retorna
       let results = lookup(wiki_dir, "auth", 10);
       assert!(results.iter().all(|r| r.authority_tier != AuthorityTier::EpisodicCache));
   }

   #[test]
   fn episodic_pages_queryable_with_explicit_flag() {
       let results = lookup_with_episodic(wiki_dir, "auth", 10);
       assert!(results.iter().any(|r| r.authority_tier == AuthorityTier::EpisodicCache));
   }
   ```
   - DoD: 3 testes escritos e falhando

2. **S2-T2.2**: GREEN
   - Adicionar `EpisodicCache` a AuthorityTier com weight 0.4
   - Em lookup.rs: filtrar EpisodicCache do resultado default
   - Novo fn `lookup_with_episodic()` que inclui
   - Storage: `.theo/wiki/episodes/` (separado de modules/ e cache/)
   - DoD: 3 testes passam

3. **S2-T2.3**: TTL enforcement
   - EpisodicCache pages tem TTL baseado em TtlPolicy do EpisodeSummary
   - `RunScoped` → deletar quando run finaliza
   - `TimeScoped(duration)` → deletar apos duration
   - GC roda em `persistence::gc_episodic_cache()`
   - DoD: teste de GC passando

**Criterio de Aceite**:
- [x] Novo tier EpisodicCache (weight 0.4)
- [x] Excluido do BM25 principal por default
- [x] Queryable com flag explicito
- [x] TTL enforced
- [x] Storage separado (.theo/wiki/episodes/)
- [x] 4+ testes

**DoD**: `cargo test -p theo-engine-retrieval -- wiki` verde.

---

### S2-T3: Integracao ContextAssembler com run_engine

**Arquivo**: `crates/theo-agent-runtime/src/run_engine.rs`

**Microtasks**:

1. **S2-T3.1**: Injetar ContextAssembler no AgentRunEngine
   - Adicionar campo `context_assembler: Option<Arc<ContextAssembler>>`
   - Usar em cada iteracao para montar contexto do prompt
   - Fallback: se None, comportamento atual (query direto)

2. **S2-T3.2**: Atualizar WorkingSet a cada iteracao
   - Apos cada tool call: atualizar hot_files, recent_event_ids
   - Apos cada hipotese: atualizar active_hypothesis
   - DoD: working set reflete estado corrente

3. **S2-T3.3**: Teste de integracao
   ```rust
   #[tokio::test]
   async fn run_with_assembler_respects_budget() {
       let engine = create_test_engine_with_assembler(4000);
       engine.execute().await.unwrap();
       // Verificar que contexto nunca excedeu budget
   }
   ```

**Criterio de Aceite**:
- [x] ContextAssembler integrado como Optional no run_engine
- [x] WorkingSet atualizado a cada iteracao
- [x] Budget respeitado end-to-end
- [x] Backward-compatible (sem assembler = comportamento atual)

**DoD**: `cargo test -p theo-agent-runtime` verde. `cargo test -p theo-application` verde.

---

### S2 — Gate de Saida

| Criterio | Verificacao |
|---|---|
| ContextAssembler minimo funcional | `cargo test -p theo-application -- assembler` |
| EpisodicCache tier isolado | `cargo test -p theo-engine-retrieval -- episodic` |
| Integrado com run_engine | `cargo test -p theo-agent-runtime` |
| 4 hard rules enforced | Verificar testes especificos |
| Workspace verde | `cargo test` |

---

## Sprint 3 — Refinamentos

> So comecar apos S2 provado em uso real.

---

### S3-T1: Symbol-level hashing em theo-engine-graph

- Hash de assinatura + corpo normalizado por simbolo
- Dirty-flag por community (evitar re-cluster desnecessario)
- DoD: invalidacao granular funcional, testes passando

### S3-T2: Impact set computation via co-change

- `compute_impact_set(changed: &[NodeId]) -> ImpactSet`
- Sem side-effects no grafo (read-only)
- Alimenta working_set.hot_files automaticamente apos edits
- DoD: impact set retorna top-K arquivos correlatos

### S3-T3: ScoringContext com pesos configuraveis

- Struct em theo-engine-retrieval com pesos injetaveis
- Default: pesos atuais hardcoded
- Override: via configuracao ou por tarefa
- DoD: pesos ajustaveis sem recompilacao

### S3-T4: Promotion WAL e archival

- Write-ahead ledger para promocoes em `.theo/wiki/runtime/promotions.jsonl`
- Archival: rotate insights para `.theo/wiki/runtime/archive/YYYY-MM-DD.jsonl.gz` apos 24h ou 50k linhas
- Crash recovery: validar WAL + ledger no startup
- DoD: sem perda de dados em crash durante GC

### S3-T5: Episode generation automatica

- Ao final de cada run (convergence ou abort): gerar EpisodeSummary
- Compactar eventos da janela em summary
- Persistir em `.theo/wiki/episodes/`
- Reter evidence_event_ids ate summary ser gerado (regra do knowledge-compiler)
- DoD: episode gerado automaticamente, evidence preservada

---

### S3 — Gate de Saida

| Criterio | Verificacao |
|---|---|
| Symbol hash funcional | `cargo test -p theo-engine-graph -- hash` |
| Impact set read-only | `cargo test -p theo-engine-graph -- impact` |
| Scoring configuravel | `cargo test -p theo-engine-retrieval -- scoring` |
| WAL + archival | `cargo test -p theo-engine-retrieval -- wal` |
| Episode auto-gerado | Rodar agent run, verificar .theo/wiki/episodes/ |

---

## Evals (Medem Progresso Real)

> Sem eval, estamos medindo o motor errado.

### E1: Context Recall@K
- Dado 10 tasks reais: os arquivos/simbolos necessarios estao no top-K do assembler?
- Target: >= 85% recall@10

### E2: Context Precision
- Quanto do contexto montado e realmente usado pelo agente?
- Target: >= 60% dos tokens montados sao referenciados na resposta

### E3: Resume Success Rate
- Apos checkpoint + restore: agente retoma sem repetir trabalho?
- Target: >= 90% das retomadas nao repetem acoes dos ultimos 3 steps

### E4: Drift Detection Accuracy
- O sistema detecta quando contexto ficou invalido?
- Target: >= 95% de mudancas relevantes detectadas

### E5: Cost-to-Solve (antes vs depois)
- Tokens, tool calls, iteracoes para resolver mesma task
- Target: reducao de >= 15% em token usage medio

---

## Invariantes Globais

Aplicam-se a TODAS as tasks de TODOS os sprints:

1. **TDD obrigatorio**: RED → GREEN → REFACTOR. Sem excecao.
2. **schema_version em tudo que persiste**: RunSnapshot, EpisodeSummary, cognitive events, EpisodicCache.
3. **Backward-compatible**: Tipos novos com `#[serde(default)]`. Dados antigos DEVEM carregar.
4. **Zero dependency violations**: arch-validator valida a cada PR.
5. **Clippy clean**: `cargo clippy -- -D warnings` em cada gate.
6. **Cognitive events validados**: payload checado por `validate_cognitive_event()`.
7. **Evidence antes de compaction**: EpisodeSummary gerado ANTES de event eviction.
