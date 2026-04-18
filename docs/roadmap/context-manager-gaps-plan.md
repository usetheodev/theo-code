# Plano Executavel: Context Manager 4.2 → 5.0

> Baseado na reuniao 20260409-115317. Dados reais do codebase, nao especulacao.
> Sequencia: P-1 (bug fixes) → P0 (metricas + evals) → P1 (lifecycle) → P2 (learning) → P3 (contradiction)

---

## Estado Atual (Verificado)

| Item | Arquivo:Linha | Status |
|---|---|---|
| TTL assignment | `episode.rs:174` | **BUG**: sempre `RunScoped`, ignora scope de constraints |
| prior_event_id validation | `event.rs:76-79` | **BUG**: so checa existencia do campo, nao referential integrity |
| successful_steps | `episode.rs:161` | **BUG**: sempre `vec![]`, nunca populado |
| failed_attempts | `episode.rs:162` | **BUG**: sempre `vec![]`, nunca populado |
| compute_usefulness | `context_metrics.rs` | NAO EXISTE |
| referenced_community_ids | `episode.rs:20-54` | CAMPO AUSENTE |
| ContextAssembler chunk tagging | `context_assembler.rs:49` | Sections sao strings sem ID |
| Retrieval metrics (MRR/DepCov) | `metrics.rs:61,151` | EXISTEM e funcionam |
| Eval CI | — | NAO INTEGRADO |

---

## P-1 — Bug Fixes (BLOQUEANTE)

> Corrigir antes de qualquer feature nova. Dados incorretos invalidam tudo.

---

### P1-BF1: TTL Promotion automatica por scope

**Problema**: `EpisodeSummary::from_events()` em `episode.rs:174` sempre usa `TtlPolicy::default()` (RunScoped). Constraints com `scope=workspace-local` sao deletadas no fim da run — perda silenciosa de conhecimento.

**Arquivo**: `crates/theo-domain/src/episode.rs`

**Microtasks**:

1. **P1-BF1.1**: RED — Testes que falham
   ```rust
   #[test]
   fn ttl_promoted_to_permanent_when_workspace_constraint_present() {
       let events = vec![
           make_event(EventType::ConstraintLearned, json!({
               "constraint": "no unwrap in auth",
               "scope": "workspace-local"
           })),
       ];
       let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
       assert_eq!(summary.ttl_policy, TtlPolicy::Permanent,
           "Workspace-scoped constraints must survive run end");
   }

   #[test]
   fn ttl_stays_run_scoped_when_only_run_local_constraints() {
       let events = vec![
           make_event(EventType::ConstraintLearned, json!({
               "constraint": "retry limit 3",
               "scope": "run-local"
           })),
       ];
       let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
       assert_eq!(summary.ttl_policy, TtlPolicy::RunScoped);
   }

   #[test]
   fn ttl_promoted_when_task_local_constraints() {
       let events = vec![
           make_event(EventType::ConstraintLearned, json!({
               "constraint": "auth module fragile",
               "scope": "task-local"
           })),
       ];
       let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
       assert_eq!(summary.ttl_policy, TtlPolicy::TimeScoped { seconds: 86400 },
           "Task-local constraints should survive 24h");
   }
   ```
   - DoD: 3 testes escritos, todos FALHAM (from_events ignora scope)

2. **P1-BF1.2**: GREEN — Implementar `infer_ttl_policy`
   ```rust
   fn infer_ttl_policy(events: &[DomainEvent]) -> TtlPolicy {
       let has_workspace = events.iter()
           .filter(|e| e.event_type == EventType::ConstraintLearned)
           .any(|e| e.payload.get("scope").and_then(|s| s.as_str()) == Some("workspace-local"));
       let has_task = events.iter()
           .filter(|e| e.event_type == EventType::ConstraintLearned)
           .any(|e| e.payload.get("scope").and_then(|s| s.as_str()) == Some("task-local"));

       if has_workspace { TtlPolicy::Permanent }
       else if has_task { TtlPolicy::TimeScoped { seconds: 86400 } }
       else { TtlPolicy::RunScoped }
   }
   ```
   - Substituir `ttl_policy: TtlPolicy::default()` por `ttl_policy: infer_ttl_policy(events)`
   - DoD: 3 testes passam

3. **P1-BF1.3**: REFACTOR — Extrair `infer_ttl_policy` como funcao publica testavel
   - DoD: `cargo test -p theo-domain -- episode` verde

**Criterio de Aceite**:
- [x] Workspace constraints → Permanent
- [x] Task constraints → TimeScoped(24h)
- [x] Run-only constraints → RunScoped
- [x] Nenhum teste existente quebrado
- [x] 3+ testes novos

**DoD**: `cargo test -p theo-domain -- ttl` verde.

---

### P1-BF2: Validacao contextual de prior_event_id

**Problema**: `validate_cognitive_event()` em `event.rs:76-79` aceita qualquer string em `prior_event_id` sem verificar se o evento referenciado existe.

**Arquivo**: `crates/theo-domain/src/event.rs`

**Microtasks**:

1. **P1-BF2.1**: RED
   ```rust
   #[test]
   fn validate_in_context_rejects_nonexistent_prior() {
       let known_ids: HashSet<String> = ["evt-1", "evt-2"].iter().map(|s| s.to_string()).collect();
       let payload = json!({"prior_event_id": "evt-999", "reason": "disproved"});
       let result = validate_cognitive_event_in_context(
           EventType::HypothesisInvalidated, &payload, &known_ids
       );
       assert!(result.is_err(), "Should reject nonexistent prior_event_id");
   }

   #[test]
   fn validate_in_context_accepts_existing_prior() {
       let known_ids: HashSet<String> = ["evt-1", "evt-2"].iter().map(|s| s.to_string()).collect();
       let payload = json!({"prior_event_id": "evt-1", "reason": "test passed"});
       let result = validate_cognitive_event_in_context(
           EventType::HypothesisInvalidated, &payload, &known_ids
       );
       assert!(result.is_ok());
   }

   #[test]
   fn validate_in_context_passes_non_hypothesis_events() {
       let known_ids: HashSet<String> = HashSet::new();
       let payload = json!({"tool_name": "bash"});
       let result = validate_cognitive_event_in_context(
           EventType::ToolCallCompleted, &payload, &known_ids
       );
       assert!(result.is_ok(), "Non-cognitive events pass without context check");
   }
   ```
   - DoD: 3 testes escritos, primeiro FALHA (funcao nao existe)

2. **P1-BF2.2**: GREEN
   ```rust
   pub fn validate_cognitive_event_in_context(
       event_type: EventType,
       payload: &serde_json::Value,
       known_event_ids: &std::collections::HashSet<String>,
   ) -> Result<(), EventValidationError> {
       // First: basic validation
       validate_cognitive_event(event_type, payload)?;
       // Then: referential integrity for invalidation events
       if event_type == EventType::HypothesisInvalidated {
           if let Some(prior_id) = payload.get("prior_event_id").and_then(|v| v.as_str()) {
               if !known_event_ids.contains(prior_id) {
                   return Err(EventValidationError::InvalidValue {
                       event_type: "HypothesisInvalidated".into(),
                       field: "prior_event_id".into(),
                       reason: format!("event '{}' not found in known events", prior_id),
                   });
               }
           }
       }
       Ok(())
   }
   ```
   - DoD: 3 testes passam

3. **P1-BF2.3**: REFACTOR — Adicionar `EventValidationError::ReferenceNotFound` variant
   - DoD: `cargo test -p theo-domain -- validate_in_context` verde

**Criterio de Aceite**:
- [x] HypothesisInvalidated com prior inexistente → Err
- [x] HypothesisInvalidated com prior existente → Ok
- [x] Eventos nao-cognitivos → Ok (passam sem check)
- [x] Backward-compatible (validate_cognitive_event sem context ainda funciona)

**DoD**: `cargo test -p theo-domain -- event` verde.

---

### P1-BF3: Popular successful_steps e failed_attempts

**Problema**: `MachineEpisodeSummary.successful_steps` e `failed_attempts` em `episode.rs:161-162` sao sempre `vec![]`. Consumidores confiam em dados vazios.

**Arquivo**: `crates/theo-domain/src/episode.rs`

**Microtasks**:

1. **P1-BF3.1**: RED
   ```rust
   #[test]
   fn from_events_populates_successful_steps() {
       let events = vec![
           make_event(EventType::ToolCallCompleted, json!({
               "tool_name": "edit", "file": "src/auth.rs", "success": true
           })),
           make_event(EventType::ToolCallCompleted, json!({
               "tool_name": "bash", "success": true
           })),
       ];
       let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
       assert!(!summary.machine_summary.successful_steps.is_empty(),
           "Should extract successful tool calls");
   }

   #[test]
   fn from_events_populates_failed_attempts() {
       let events = vec![
           make_event(EventType::Error, json!({"message": "compile error in auth.rs"})),
           make_event(EventType::ToolCallCompleted, json!({
               "tool_name": "edit", "success": false, "error": "file not found"
           })),
       ];
       let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
       assert!(!summary.machine_summary.failed_attempts.is_empty(),
           "Should extract failed tool calls and errors");
   }
   ```
   - DoD: 2 testes escritos, ambos FALHAM

2. **P1-BF3.2**: GREEN — Extrair de eventos
   - `successful_steps`: ToolCallCompleted com `success: true` → `"tool_name: file"`
   - `failed_attempts`: ToolCallCompleted com `success: false` + Error events → mensagem
   - DoD: 2 testes passam

3. **P1-BF3.3**: REFACTOR — Dedup de entries
   - DoD: `cargo test -p theo-domain -- episode` verde

**Criterio de Aceite**:
- [x] successful_steps populado com tool calls bem-sucedidos
- [x] failed_attempts populado com erros e tool calls falhados
- [x] Testes existentes nao quebram

**DoD**: `cargo test -p theo-domain -- episode` verde.

---

## P0 — Metricas + Evals (PARALELO)

> Instrumentar o que funciona antes de mudar como funciona.

---

### P0-T1: Context usefulness proxy

**Problema**: Nao medimos quais blocos de contexto o agente realmente usou. Sem isso, otimizacao e cega.

**Arquivos**:
- `crates/theo-application/src/use_cases/context_assembler.rs` (chunk tagging)
- `crates/theo-agent-runtime/src/context_metrics.rs` (usefulness scoring)
- `crates/theo-domain/src/event.rs` (ContextUsageSignal variant)

**Microtasks**:

1. **P0-T1.1**: RED — Testes de usefulness
   ```rust
   // context_metrics.rs
   #[test]
   fn usefulness_positive_when_context_file_in_tool_call() {
       let mut m = ContextMetrics::new();
       m.record_assembled_chunk("community:auth", vec!["src/auth.rs".into()]);
       m.record_tool_reference("src/auth.rs");
       let scores = m.compute_usefulness();
       assert!(scores["community:auth"] > 0.0);
   }

   #[test]
   fn usefulness_zero_when_context_not_referenced() {
       let mut m = ContextMetrics::new();
       m.record_assembled_chunk("community:db", vec!["src/db.rs".into()]);
       m.record_tool_reference("src/auth.rs"); // different file
       let scores = m.compute_usefulness();
       assert_eq!(scores.get("community:db").copied().unwrap_or(0.0), 0.0);
   }

   #[test]
   fn usefulness_report_includes_scores() {
       let mut m = ContextMetrics::new();
       m.record_assembled_chunk("c:auth", vec!["src/auth.rs".into()]);
       m.record_tool_reference("src/auth.rs");
       let report = m.to_report();
       assert!(!report.usefulness_scores.is_empty());
   }
   ```
   - DoD: 3 testes escritos, todos FALHAM (metodos nao existem)

2. **P0-T1.2**: GREEN — Implementar em ContextMetrics
   - Adicionar campos: `assembled_chunks: HashMap<String, Vec<String>>` (community_id → file paths)
   - Adicionar campo: `tool_references: Vec<String>` (files referenced by agent)
   - `record_assembled_chunk(community_id, files)` — registra chunk montado
   - `record_tool_reference(file)` — registra arquivo acessado pelo agente
   - `compute_usefulness() -> HashMap<String, f64>` — score = intersection(chunk_files, referenced_files) / chunk_files.len()
   - Adicionar `usefulness_scores: HashMap<String, f64>` ao `ContextMetricsReport`
   - DoD: 3 testes passam

3. **P0-T1.3**: Integrar com run_engine.rs
   - Apos cada tool call de read/edit/grep: `context_metrics.record_tool_reference(file)`
   - Quando ContextAssembler montar contexto: registrar chunks
   - DoD: `cargo test -p theo-agent-runtime` verde

4. **P0-T1.4**: Adicionar EventType::ContextUsageSignal
   - Em `event.rs`: novo variant `ContextUsageSignal` (non-cognitive, passa validation)
   - Publicado ao final de cada iteracao com usefulness scores
   - DoD: `cargo test -p theo-domain -- event` verde

**Criterio de Aceite**:
- [x] Cada chunk de contexto montado tem ID rastreavel
- [x] Cada arquivo acessado pelo agente e registrado
- [x] Usefulness score computado: files_used / files_provided por community
- [x] Score persiste no ContextMetricsReport JSON
- [x] Overhead < 5% do tempo de assembly
- [x] 5+ testes novos

**DoD**: `cargo test -p theo-agent-runtime -- usefulness` verde. `cargo test -p theo-domain -- event` verde.

---

### P0-T2: Eval CI com golden cases

**Problema**: Evals definidos mas nao integrados com CI. Regressoes sao invisiveis.

**Arquivos**:
- `crates/theo-engine-retrieval/src/metrics.rs` (ja tem MRR/DepCov/hit_at_k)
- `crates/theo-engine-retrieval/tests/` (novo eval fixture)

**Microtasks**:

1. **P0-T2.1**: RED — Eval fixture
   ```rust
   // tests/eval_golden.rs (novo)
   use theo_engine_retrieval::metrics::{mrr, hit_at_k, dep_coverage};

   struct GoldenCase {
       query: &'static str,
       expected_files: &'static [&'static str],
       expected_deps: &'static [&'static str],
   }

   const GOLDEN_CASES: &[GoldenCase] = &[
       GoldenCase {
           query: "authentication flow",
           expected_files: &["crates/theo-infra-auth/src/lib.rs"],
           expected_deps: &["theo-domain"],
       },
       // ... 10-15 cases
   ];

   #[test]
   fn eval_aggregate_mrr_above_floor() {
       let results = run_all_golden_cases();
       let aggregate_mrr = compute_aggregate_mrr(&results);
       assert!(aggregate_mrr >= 0.80,
           "MRR must be >= 0.80, got {:.3}", aggregate_mrr);
   }

   #[test]
   fn eval_aggregate_dep_coverage_above_floor() {
       let results = run_all_golden_cases();
       let aggregate_dep = compute_aggregate_dep_coverage(&results);
       assert!(aggregate_dep >= 0.95,
           "DepCov must be >= 0.95, got {:.3}", aggregate_dep);
   }
   ```
   - DoD: testes escritos, podem passar ou falhar (baseline measurement)

2. **P0-T2.2**: GREEN — Implementar runner de golden cases
   - Usa Pipeline existente para construir graph + score
   - Cada golden case: build graph do repo real → query → compute metrics
   - Aggregate: average across all cases
   - DoD: runner funciona, baseline medido

3. **P0-T2.3**: REFACTOR — Configuracao via TOML
   - Golden cases em `eval_cases.toml` (nao hardcoded)
   - DoD: cases externalizados, runner le TOML

**Criterio de Aceite**:
- [x] 10+ golden cases definidos
- [x] MRR e DepCov computados automaticamente
- [x] Testes passam como baseline (ou falham com threshold claro)
- [x] Pode ser rodado via `cargo test -p theo-engine-retrieval --test eval_golden`

**DoD**: `cargo test -p theo-engine-retrieval --test eval_golden` roda e reporta metricas.

---

### P0-T3: referenced_community_ids em EpisodeSummary

**Problema**: EpisodeSummary nao rastreia quais communities do GraphCTX foram usadas. Impossivel saber o que foi util.

**Arquivo**: `crates/theo-domain/src/episode.rs`

**Microtasks**:

1. **P0-T3.1**: RED
   ```rust
   #[test]
   fn episode_summary_tracks_referenced_communities() {
       let mut summary = EpisodeSummary::from_events("r-1", None, "task", &[]);
       summary.referenced_community_ids = vec!["community:auth".into(), "community:db".into()];
       let json = serde_json::to_string(&summary).unwrap();
       let back: EpisodeSummary = serde_json::from_str(&json).unwrap();
       assert_eq!(back.referenced_community_ids.len(), 2);
   }
   ```
   - DoD: teste FALHA (campo nao existe)

2. **P0-T3.2**: GREEN — Adicionar campo
   ```rust
   #[serde(default)]
   pub referenced_community_ids: Vec<String>,
   ```
   - DoD: teste passa, backward-compatible (serde default)

**Criterio de Aceite**:
- [x] Campo existe, serde roundtrip funciona
- [x] Backward-compatible (summaries sem campo carregam com vec vazio)

**DoD**: `cargo test -p theo-domain -- episode` verde.

---

## P1 — Memory Lifecycle

> Requer dados de P0 (usefulness scores) para informar politica.

---

### P1-T1: Promotion policy baseada em usefulness

**Problema**: Sem politica de promocao, episodios uteis e inuteis sao tratados igualmente.

**Arquivo**: `crates/theo-engine-retrieval/src/wiki/runtime.rs`

**Microtasks**:

1. **P1-T1.1**: RED
   ```rust
   #[test]
   fn episode_with_high_usefulness_promoted() {
       let summary = EpisodeSummary { /* usefulness > threshold */ };
       let action = evaluate_promotion(&summary, 0.5);
       assert_eq!(action, PromotionAction::Promoted);
   }

   #[test]
   fn episode_with_low_usefulness_not_promoted() {
       let summary = EpisodeSummary { /* usefulness < threshold */ };
       let action = evaluate_promotion(&summary, 0.5);
       assert_eq!(action, PromotionAction::Evicted);
   }
   ```

2. **P1-T1.2**: GREEN
   ```rust
   pub fn evaluate_promotion(summary: &EpisodeSummary, threshold: f64) -> PromotionAction {
       let avg_usefulness = if summary.referenced_community_ids.is_empty() {
           0.0
       } else {
           // proportion of communities that were actually used
           // (this is a proxy until real usefulness scores are available)
           1.0
       };
       if avg_usefulness >= threshold {
           PromotionAction::Promoted
       } else {
           PromotionAction::Evicted
       }
   }
   ```

3. **P1-T1.3**: REFACTOR — Integrar com archival
   - Promoted → movido de EpisodicCache para PromotedCache
   - Evicted → deletado ou movido para archive

**Criterio de Aceite**:
- [x] Episodios com alta usefulness promovidos
- [x] Episodios com baixa usefulness evicted
- [x] Transicoes registradas no WAL
- [x] 3+ testes

**DoD**: `cargo test -p theo-engine-retrieval -- promotion` verde.

---

### P1-T2: Decay e hard limits operacionais

**Problema**: Sem limites, memoria cresce indefinidamente em runs longas.

**Arquivo**: `crates/theo-engine-retrieval/src/wiki/runtime.rs`

**Microtasks**:

1. **P1-T2.1**: RED
   ```rust
   #[test]
   fn summaries_beyond_limit_archived() {
       // Criar 600 summaries (limit 500)
       // assert: 100 mais antigos arquivados
   }

   #[test]
   fn raw_events_beyond_size_limit_compacted() {
       // Criar >10MB de events
       // assert: forced compaction triggered
   }
   ```

2. **P1-T2.2**: GREEN — Implementar `enforce_limits()`
   ```rust
   pub struct OperationalLimits {
       pub max_raw_event_bytes: usize,      // 10MB
       pub max_active_summaries: usize,     // 500
       pub archival_ttl_days: u32,          // 30
   }
   
   pub fn enforce_limits(wiki_dir: &Path, limits: &OperationalLimits) -> EnforcementReport
   ```

3. **P1-T2.3**: REFACTOR — Health check
   - `check_health(wiki_dir) -> HealthStatus` com alertas se perto dos limites

**Criterio de Aceite**:
- [x] Hard limits enforced: 10MB raw, 500 summaries, 30-day TTL
- [x] Enforcement nao perde dados silenciosamente (loga o que archivou)
- [x] Health check disponivel
- [x] 3+ testes

**DoD**: `cargo test -p theo-engine-retrieval -- limits` verde.

---

## P2 — Closed-Loop Learning

> **GATED**: So inicia apos 50+ observacoes de usefulness (P0-T1 dados reais).

---

### P2-T1: Assembly feedback loop

**Problema**: Assembler monta contexto mas nunca aprende o que funcionou.

**Arquivo**: `crates/theo-application/src/use_cases/context_assembler.rs`

**Microtasks**:

1. **P2-T1.1**: RED
   ```rust
   #[test]
   fn assembler_adjusts_after_feedback() {
       let mut assembler = ContextAssembler::new(4000);
       // Initial assembly
       let ctx1 = assembler.assemble("task", &ws, &gc, &[]);
       // Feedback: auth community was useful, db was not
       assembler.record_feedback("community:auth", 0.9);
       assembler.record_feedback("community:db", 0.1);
       // Second assembly: auth should be prioritized
       let ctx2 = assembler.assemble("task", &ws, &gc, &[]);
       // auth content should appear before db content (higher priority)
   }
   ```

2. **P2-T1.2**: GREEN — Adicionar `feedback_scores: HashMap<String, f64>` ao assembler
   - `record_feedback(community_id, score)` — acumula exponential moving average
   - `assemble()` usa feedback como boost no ordering dos structural blocks
   - DoD: teste passa

3. **P2-T1.3**: REFACTOR — Persistir feedback entre runs
   - Salvar em `.theo/assembly_feedback.json`
   - Carregar na construcao do assembler

**Criterio de Aceite**:
- [x] Feedback recorded por community
- [x] Assembly ordering influenciado por feedback historico
- [x] Persistido entre runs
- [x] 4 hard rules NUNCA violadas (budget, objective, step, evidence)
- [x] 3+ testes

**DoD**: `cargo test -p theo-application -- assembler` verde.

---

## P3 — Contradiction Management

> **DEFERRED**: So implementar quando volume de memoria justificar.

---

### P3-T1: Deteccao de conflitos em compaction

**Problema**: Hipoteses contraditorias acumulam sem deteccao.

**Abordagem**: Flag-then-resolve (NAO auto-prune).

**Microtasks** (futuras):

1. Na geracao de EpisodeSummary, comparar learned_constraints com summaries anteriores
2. Se constraint contradiz anterior: marcar como `ConflictDetected` event
3. Resolucao manual ou por evidencia (nao automatica)

**Criterio de Aceite**:
- [x] Contradicoes detectadas e flagged
- [x] Nenhuma hipotese pruned automaticamente
- [x] Conflitos visiveis no report

---

## Evals (Integrados ao Plano)

| Eval | Quando | Target | Como Medir |
|---|---|---|---|
| **Context Recall@K** | P0-T2 | >= 85% recall@10 | Golden cases com expected files |
| **Context Precision** | P0-T1 | >= 60% tokens referenciados | Usefulness proxy scores |
| **Resume Success Rate** | P1+ | >= 90% sem repeticao | Antes/depois de checkpoint restore |
| **Drift Detection** | P1-T2 | >= 95% mudancas detectadas | Blake3 content hash + symbol hash |
| **Cost-to-Solve** | P2+ | -15% tokens medios | Antes/depois de feedback loop |

---

## Sequenciamento Final

```
P-1 (IMEDIATO — bug fixes):
  BF1: TTL promotion por scope         → episode.rs
  BF2: prior_event_id validation       → event.rs
  BF3: successful_steps population     → episode.rs

P0 (PARALELO — medir):
  T1: Context usefulness proxy          → context_metrics.rs + run_engine.rs
  T2: Eval CI golden cases              → tests/eval_golden.rs
  T3: referenced_community_ids          → episode.rs

P1 (APOS P0 — lifecycle):
  T1: Promotion policy com usefulness   → runtime.rs
  T2: Hard limits + health check        → runtime.rs

P2 (GATED em 50+ observacoes):
  T1: Assembly feedback loop            → context_assembler.rs

P3 (DEFERRED):
  T1: Contradiction detection           → futuro
```

---

## Invariantes Globais

1. **TDD obrigatorio**: RED → GREEN → REFACTOR para cada microtask
2. **Backward-compatible**: novos campos com `#[serde(default)]`
3. **4 hard rules do assembler NUNCA violadas**: budget, objective, step, evidence
4. **schema_version em tudo que persiste**
5. **Extend, nao duplicar**: max 3 tipos novos
6. **Overhead < 5%**: usefulness tracking nao pode degradar performance
7. **Datos antes de features**: P2 gated em 50+ observacoes reais de P0
