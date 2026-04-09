# Plano Executavel: Context Manager 4.7 → 5.0

> 5 gaps finais. Nivel refinamento, nao arquitetural.
> Baseado nas reunioes 20260409-134738 + addendum do Paulo.
> Cada gap tem: versao minima viavel, politica operacional, TDD, e DoD.

---

## Estado Atual (Verificado)

| Item | Arquivo | Status |
|---|---|---|
| EpisodeSummary.lifecycle | `episode.rs` | **MISSING** — usa TtlPolicy, nao lifecycle |
| WorkingSet.agent_id | `working_set.rs` | **MISSING** |
| WorkingSet.merge() | `working_set.rs` | **MISSING** |
| ContextBlock.block_id | `graph_context.rs` | **MISSING** — usa source_id |
| Hypothesis struct | — | **MISSING** — so eventos |
| HypothesisStatus enum | — | **MISSING** |
| FailurePattern (domain) | `failure_tracker.rs` (runtime) | Existe como PatternEntry no runtime, nao no domain |
| ContextAssembler lifecycle filter | `context_assembler.rs` | **MISSING** — nao filtra por lifecycle |
| ContextMetrics.compute_usefulness | `context_metrics.rs:132` | EXISTE |

---

## P0.5 — Memory Typing com Politica Real

> Nao so classificacao — comportamento por tier.

---

### P05-T1: MemoryLifecycle enum com politica

**Problema**: EpisodeSummary nao tem lifecycle. Sem isso, sistema nao diferencia memoria ativa de historica.

**Arquivo**: `crates/theo-domain/src/episode.rs`

**Microtasks**:

1. **P05-T1.1**: RED
   ```rust
   #[test]
   fn lifecycle_defaults_to_active() {
       let summary = EpisodeSummary::from_events("r-1", None, "task", &[]);
       assert_eq!(summary.lifecycle, MemoryLifecycle::Active);
   }

   #[test]
   fn lifecycle_serde_roundtrip_all_variants() {
       for lc in &[MemoryLifecycle::Active, MemoryLifecycle::Cooling, MemoryLifecycle::Archived] {
           let json = serde_json::to_string(lc).unwrap();
           let back: MemoryLifecycle = serde_json::from_str(&json).unwrap();
           assert_eq!(*lc, back);
       }
   }

   #[test]
   fn lifecycle_backward_compat_defaults_to_active() {
       // Legacy summaries without lifecycle field
       let mut val = serde_json::to_value(&EpisodeSummary::from_events("r-1", None, "t", &[])).unwrap();
       val.as_object_mut().unwrap().remove("lifecycle");
       let back: EpisodeSummary = serde_json::from_value(val).unwrap();
       assert_eq!(back.lifecycle, MemoryLifecycle::Active);
   }
   ```
   - DoD: 3 testes escritos, todos FALHAM

2. **P05-T1.2**: GREEN
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   pub enum MemoryLifecycle {
       Active,   // runtime: always in assembler, high priority
       Cooling,  // post-episode: conditional (usefulness > 0.3), 24h TTL
       Archived, // long-term: lookup-only, 30d or Permanent
   }

   impl Default for MemoryLifecycle {
       fn default() -> Self { MemoryLifecycle::Active }
   }
   ```
   - Adicionar `#[serde(default)] pub lifecycle: MemoryLifecycle` ao EpisodeSummary
   - Inicializar como `Active` em from_events()
   - DoD: 3 testes passam

3. **P05-T1.3**: Politica de elegibilidade
   ```rust
   impl MemoryLifecycle {
       /// Whether this lifecycle tier is eligible for context assembly.
       pub fn eligible_for_assembly(&self) -> bool {
           matches!(self, MemoryLifecycle::Active)
       }

       /// Whether this tier requires a minimum usefulness score to be assembled.
       pub fn requires_usefulness_gate(&self) -> bool {
           matches!(self, MemoryLifecycle::Cooling)
       }

       /// Usefulness threshold for Cooling tier (0.3).
       pub fn usefulness_threshold(&self) -> f64 {
           match self {
               MemoryLifecycle::Active => 0.0,
               MemoryLifecycle::Cooling => 0.3,
               MemoryLifecycle::Archived => 1.0, // effectively never
           }
       }
   }
   ```
   - Testes: `eligible_for_assembly`, `cooling_requires_gate`
   - DoD: 5+ testes no episode.rs

4. **P05-T1.4**: Transicoes de lifecycle
   ```rust
   pub fn transition_lifecycle(events: &[DomainEvent], current: MemoryLifecycle) -> MemoryLifecycle {
       match current {
           Active => Cooling,    // episode boundary
           Cooling => Archived,  // TTL expired or enforce_limits
           Archived => Archived, // terminal (promotion handled separately)
       }
   }

   pub fn should_promote_to_permanent(reuse_count: usize) -> bool {
       reuse_count >= 2
   }
   ```
   - DoD: testes de transicao

**Criterio de Aceite**:
- [x] MemoryLifecycle enum com 3 variants
- [x] Politica por tier (elegibilidade, threshold, TTL)
- [x] Transicoes definidas e testadas
- [x] Backward-compatible (#[serde(default)])
- [x] 7+ testes

**DoD**: `cargo test -p theo-domain -- lifecycle` verde.

---

### P05-T2: Integrar lifecycle com ContextAssembler

**Problema**: Assembler nao filtra por lifecycle. Memoria Archived entra no contexto.

**Arquivo**: `crates/theo-application/src/use_cases/context_assembler.rs`

**Microtasks**:

1. **P05-T2.1**: RED
   ```rust
   #[test]
   fn assembler_excludes_archived_episodes() {
       // EpisodeSummary com lifecycle=Archived nao deve entrar no contexto
   }

   #[test]
   fn assembler_includes_active_episodes() {
       // EpisodeSummary com lifecycle=Active entra normalmente
   }

   #[test]
   fn assembler_gates_cooling_by_usefulness() {
       // Cooling com usefulness < 0.3 excluido
       // Cooling com usefulness >= 0.3 incluido
   }
   ```

2. **P05-T2.2**: GREEN — Adicionar filtro no assemble()
   - Antes de incluir episodes no contexto, checar `lifecycle.eligible_for_assembly()`
   - Para Cooling: checar `usefulness_score >= lifecycle.usefulness_threshold()`
   - DoD: 3 testes passam

**Criterio de Aceite**:
- [x] Active: sempre elegivel
- [x] Cooling: gated por usefulness >= 0.3
- [x] Archived: nunca elegivel (so lookup explicito)
- [x] 3+ testes

**DoD**: `cargo test -p theo-application -- assembler` verde.

---

## P1 — Failure Learning Loop

> Erro recorrente → constraint automatica com lifecycle binding.

---

### P1-T1: FailurePattern no domain + auto-constraint

**Problema**: Erros recorrentes nao geram constraints automaticamente. PatternEntry existe no runtime mas nao no domain.

**Arquivos**:
- `crates/theo-domain/src/episode.rs` (integrar com from_events)
- `crates/theo-domain/src/event.rs` (novo EventType: FailureLearned)

**Microtasks**:

1. **P1-T1.1**: RED
   ```rust
   #[test]
   fn recurring_error_generates_constraint() {
       let events = vec![
           make_error("file not found: src/auth.rs"),
           make_error("file not found: src/auth.rs"),
           make_error("file not found: src/auth.rs"),
       ];
       let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
       assert!(summary.machine_summary.learned_constraints
           .iter().any(|c| c.contains("file not found")),
           "Recurring error should generate constraint");
   }

   #[test]
   fn isolated_error_does_not_generate_constraint() {
       let events = vec![make_error("timeout")];
       let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
       assert!(!summary.machine_summary.learned_constraints
           .iter().any(|c| c.contains("timeout")));
   }

   #[test]
   fn failure_derived_constraint_starts_active() {
       let events = vec![
           make_error("permission denied"),
           make_error("permission denied"),
           make_error("permission denied"),
       ];
       let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
       // Constraint should be run-scoped initially
       assert_eq!(summary.lifecycle, MemoryLifecycle::Active);
   }
   ```

2. **P1-T1.2**: GREEN — Extrair patterns de erros em from_events
   ```rust
   fn extract_failure_constraints(events: &[DomainEvent], threshold: usize) -> Vec<String> {
       let mut counts: HashMap<String, usize> = HashMap::new();
       for e in events.iter().filter(|e| e.event_type == EventType::Error) {
           if let Some(msg) = e.payload.get("message").and_then(|v| v.as_str()) {
               let normalized = normalize_error(msg);
               *counts.entry(normalized).or_insert(0) += 1;
           }
       }
       counts.into_iter()
           .filter(|(_, count)| *count >= threshold)
           .map(|(msg, count)| format!("Avoid: {} (seen {} times)", msg, count))
           .collect()
   }
   ```
   - Integrar no from_events: append results to `learned_constraints`
   - Threshold padrao: 3

3. **P1-T1.3**: REFACTOR — normalize_error como funcao pura
   - Strip line numbers, strip paths, lowercase
   - DoD: testes passam

**Criterio de Aceite**:
- [x] Erro repetido ≥3 vezes → constraint automatica
- [x] Erro isolado → nenhuma constraint
- [x] Constraint normalizada (sem numeros de linha)
- [x] 4+ testes

**DoD**: `cargo test -p theo-domain -- failure` verde.

---

## P1.5 — Multi-agent WorkingSet Isolation

> Per-agent scope. Clone pra sub-agent, merge de volta.

---

### P15-T1: agent_id + merge() em WorkingSet

**Problema**: WorkingSet nao tem nocao de dono. Sub-agents contaminam parent.

**Arquivo**: `crates/theo-domain/src/working_set.rs`

**Microtasks**:

1. **P15-T1.1**: RED
   ```rust
   #[test]
   fn working_set_has_agent_id() {
       let mut ws = WorkingSet::new();
       ws.agent_id = Some("agent-main".into());
       assert_eq!(ws.agent_id, Some("agent-main".to_string()));
   }

   #[test]
   fn working_set_clone_is_independent() {
       let mut parent = WorkingSet::new();
       parent.touch_file("a.rs");
       let mut child = parent.clone();
       child.touch_file("b.rs");
       assert!(!parent.hot_files.contains(&"b.rs".to_string()),
           "Child modification must not affect parent");
   }

   #[test]
   fn working_set_merge_combines_hot_files() {
       let parent = WorkingSet { hot_files: vec!["a.rs".into()], ..Default::default() };
       let child = WorkingSet { hot_files: vec!["b.rs".into()], ..Default::default() };
       let merged = parent.merge_from(&child);
       assert_eq!(merged.hot_files.len(), 2);
   }

   #[test]
   fn working_set_merge_preserves_parent_constraints() {
       let parent = WorkingSet { constraints: vec!["no unwrap".into()], ..Default::default() };
       let child = WorkingSet { constraints: vec!["use Result".into()], ..Default::default() };
       let merged = parent.merge_from(&child);
       assert!(merged.constraints.contains(&"no unwrap".to_string()));
       assert!(merged.constraints.contains(&"use Result".to_string()));
   }
   ```

2. **P15-T1.2**: GREEN
   ```rust
   // Add to WorkingSet struct:
   #[serde(default, skip_serializing_if = "Option::is_none")]
   pub agent_id: Option<String>,

   // Add method:
   pub fn merge_from(&self, other: &WorkingSet) -> WorkingSet {
       let mut merged = self.clone();
       for f in &other.hot_files {
           merged.touch_file(f.clone());
       }
       for eid in &other.recent_event_ids {
           if !merged.recent_event_ids.contains(eid) {
               merged.recent_event_ids.push(eid.clone());
           }
       }
       // Child hypothesis wins if parent has none
       if merged.active_hypothesis.is_none() {
           merged.active_hypothesis = other.active_hypothesis.clone();
       }
       // Merge constraints (dedup)
       for c in &other.constraints {
           if !merged.constraints.contains(c) {
               merged.constraints.push(c.clone());
           }
       }
       merged
   }
   ```

3. **P15-T1.3**: REFACTOR — Dedup hot_files no merge

**Criterio de Aceite**:
- [x] agent_id campo Optional, backward-compatible
- [x] Clone e independente
- [x] merge_from combina sem perder dados do parent
- [x] 4+ testes

**DoD**: `cargo test -p theo-domain -- working_set` verde.

---

## P2 — Hypothesis Engine

> Struct com confidence + status. Manual + fallback automatico leve.

---

### P2-T1: Hypothesis struct + HypothesisStatus

**Problema**: Hipoteses existem so como eventos. Sem struct, nao ha tracking de confidence ou degradacao.

**Arquivo**: `crates/theo-domain/src/episode.rs` (mesmo modulo de EpisodeSummary)

**Microtasks**:

1. **P2-T1.1**: RED
   ```rust
   #[test]
   fn hypothesis_created_with_default_confidence() {
       let h = Hypothesis::new("h-1", "jwt decode bug", "test fails");
       assert_eq!(h.confidence, 0.5);
       assert_eq!(h.status, HypothesisStatus::Active);
   }

   #[test]
   fn hypothesis_degrades_to_stale() {
       let mut h = Hypothesis::new("h-1", "bug", "reason");
       h.mark_stale();
       assert_eq!(h.status, HypothesisStatus::Stale);
   }

   #[test]
   fn hypothesis_superseded_never_enters_assembler() {
       let mut h = Hypothesis::new("h-1", "bug", "reason");
       h.supersede("h-2");
       assert_eq!(h.status, HypothesisStatus::Superseded);
       assert!(!h.is_eligible_for_assembly());
   }

   #[test]
   fn hypothesis_only_active_eligible() {
       let active = Hypothesis::new("h-1", "a", "r");
       let mut stale = Hypothesis::new("h-2", "b", "r");
       stale.mark_stale();
       assert!(active.is_eligible_for_assembly());
       assert!(!stale.is_eligible_for_assembly());
   }

   #[test]
   fn hypothesis_serde_roundtrip() {
       let h = Hypothesis::new("h-1", "test", "reason");
       let json = serde_json::to_string(&h).unwrap();
       let back: Hypothesis = serde_json::from_str(&json).unwrap();
       assert_eq!(h.id, back.id);
       assert_eq!(h.status, back.status);
   }
   ```

2. **P2-T1.2**: GREEN
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   pub enum HypothesisStatus {
       Active,      // in use, enters assembler
       Stale,       // unused for N iterations, assembler ignores
       Superseded,  // contradicted, never enters
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Hypothesis {
       pub id: String,
       pub description: String,
       pub rationale: String,
       pub confidence: f64,
       pub status: HypothesisStatus,
       pub evidence_event_ids: Vec<String>,
       pub superseded_by: Option<String>,
       pub created_at: u64,
       pub last_accessed_iteration: usize,
       pub source: HypothesisSource,
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   pub enum HypothesisSource {
       Explicit,  // model emitted HypothesisFormed
       Inferred,  // system detected repeated pattern
   }

   impl Hypothesis {
       pub fn new(id: &str, description: &str, rationale: &str) -> Self { ... }
       pub fn mark_stale(&mut self) { self.status = HypothesisStatus::Stale; }
       pub fn supersede(&mut self, by: &str) {
           self.status = HypothesisStatus::Superseded;
           self.superseded_by = Some(by.to_string());
       }
       pub fn is_eligible_for_assembly(&self) -> bool {
           self.status == HypothesisStatus::Active
       }
   }
   ```

3. **P2-T1.3**: Fallback automatico — inferir hipotese de padroes repetidos
   ```rust
   pub fn infer_hypotheses_from_patterns(events: &[DomainEvent]) -> Vec<Hypothesis> {
       // Detect repeated tool calls (same tool+args 3+ times)
       // Create Hypothesis with source=Inferred, confidence=0.3
   }
   ```
   - Testes: pattern detection, confidence=0.3, source=Inferred

**Criterio de Aceite**:
- [x] Hypothesis struct com confidence + status
- [x] HypothesisStatus: Active/Stale/Superseded
- [x] Somente Active e elegivel para assembler
- [x] Supersede preserva original (nunca deleta)
- [x] Fallback inferido com confidence=0.3, source=Inferred
- [x] 7+ testes

**DoD**: `cargo test -p theo-domain -- hypothesis` verde.

---

## P2.5 — Block ID Tagging + Citation Shadow

> Infra para causal usefulness. Shadow mode com feedback parcial.

---

### P25-T1: block_id em ContextBlock

**Problema**: ContextBlock nao tem ID unico. Impossivel rastrear qual bloco foi citado.

**Arquivo**: `crates/theo-domain/src/graph_context.rs`

**Microtasks**:

1. **P25-T1.1**: RED
   ```rust
   #[test]
   fn context_block_has_block_id() {
       let block = ContextBlock {
           block_id: "blk-123".into(),
           source_id: "community:auth".into(),
           content: "# Auth".into(),
           token_count: 50,
           score: 0.8,
       };
       assert!(!block.block_id.is_empty());
   }
   ```

2. **P25-T1.2**: GREEN
   - Adicionar `#[serde(default)] pub block_id: String` ao ContextBlock
   - Gerar UUID no assembly (nao na criacao — assembly e o ponto de tagging)
   - DoD: backward-compatible

**Criterio de Aceite**:
- [x] block_id campo em ContextBlock
- [x] Backward-compatible
- [x] Gerado no assembly

**DoD**: `cargo test -p theo-domain -- context_block` verde.

---

### P25-T2: Citation extractor (shadow mode)

**Problema**: Nao sabemos quais blocos o agente realmente citou.

**Arquivo**: `crates/theo-agent-runtime/src/context_metrics.rs`

**Microtasks**:

1. **P25-T2.1**: RED
   ```rust
   #[test]
   fn citation_extractor_finds_paths_in_tool_args() {
       let tool_args = json!({"filePath": "src/auth.rs", "command": "cat src/db.rs"});
       let block_map = HashMap::from([
           ("blk-1".to_string(), vec!["src/auth.rs".to_string()]),
           ("blk-2".to_string(), vec!["src/db.rs".to_string()]),
       ]);
       let cited = extract_citations(&tool_args, &block_map);
       assert!(cited.contains(&"blk-1".to_string()));
       assert!(cited.contains(&"blk-2".to_string()));
   }

   #[test]
   fn citation_extractor_empty_when_no_match() {
       let tool_args = json!({"filePath": "src/unknown.rs"});
       let block_map = HashMap::from([("blk-1".to_string(), vec!["src/auth.rs".to_string()])]);
       let cited = extract_citations(&tool_args, &block_map);
       assert!(cited.is_empty());
   }
   ```

2. **P25-T2.2**: GREEN
   ```rust
   pub fn extract_citations(
       tool_args: &serde_json::Value,
       block_map: &HashMap<String, Vec<String>>, // block_id → file paths
   ) -> Vec<String> {
       let args_str = tool_args.to_string();
       block_map.iter()
           .filter(|(_, files)| files.iter().any(|f| args_str.contains(f)))
           .map(|(block_id, _)| block_id.clone())
           .collect()
   }
   ```

3. **P25-T2.3**: Shadow feedback — alimentar EMA com alpha=0.1
   ```rust
   pub fn apply_shadow_feedback(
       metrics: &mut ContextMetrics,
       cited_block_ids: &[String],
       all_block_ids: &[String],
   ) {
       for bid in all_block_ids {
           let score = if cited_block_ids.contains(bid) { 1.0 } else { 0.0 };
           metrics.record_shadow_citation(bid, score);
       }
   }
   ```

**Criterio de Aceite**:
- [x] extract_citations funciona como pure function
- [x] Shadow feedback com alpha=0.1 (nao atualiza weights de producao)
- [x] 4+ testes

**DoD**: `cargo test -p theo-agent-runtime -- citation` verde.

---

## Sequenciamento Final

```
P0.5:  Memory Typing         → MemoryLifecycle enum + politica + assembler filter
P1:    Failure Learning       → extract_failure_constraints + from_events integration
P1.5:  Multi-agent isolation  → agent_id + merge_from() + per-agent dirs
P2:    Hypothesis Engine      → struct + status + fallback inferido
P2.5:  Block ID + Shadow      → block_id tagging + citation extractor + EMA shadow
P3:    Causal Attribution     → DEFER (gated em 50+ episodes com block_id data)
```

---

## Invariantes Globais (cumulativas)

1. **TDD obrigatorio**: RED → GREEN → REFACTOR
2. **Backward-compatible**: `#[serde(default)]` em todo campo novo
3. **Canonical = append-only**: supersede, never overwrite
4. **Hypothesis prune somente via evento**: nunca auto-delete por idade
5. **Hypothesis degradacao automatica**: Stale apos 10 iteracoes sem uso
6. **Failure threshold ≥3**: nao gerar constraint de erro isolado
7. **Cross-agent quarantine**: EpisodeSummary com TTL ≥1h antes de promotion
8. **Shadow mode obrigatorio**: citation feedback nao atualiza weights ate eval CI gate
9. **Max 3 tipos novos**: MemoryLifecycle, Hypothesis, HypothesisStatus (FailurePattern reutiliza PatternEntry existente)
10. **Assembler 4 hard rules NUNCA violadas**: budget, objective, step, evidence

---

## Evals (Atualizados)

| Eval | Target | Gate |
|---|---|---|
| MRR | ≥ 0.80 | CI (eval_golden.rs) |
| DepCov | ≥ 0.80 | CI (eval_golden.rs) |
| Hit@5 | ≥ 0.80 | CI (eval_golden.rs) |
| Context Precision | ≥ 60% tokens referenciados | P2.5 (shadow data) |
| Resume Success | ≥ 90% sem repeticao | P1.5 (multi-agent) |
| Hypothesis Accuracy | ≥ 70% hipoteses validas | P2 (apos 50+ episodes) |
