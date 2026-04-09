---
id: 20260409-115317
date: 2026-04-09
topic: "Context Manager 4.2→5/5: Prioridade e abordagem para 5 gaps restantes"
verdict: REVISED
participants: 16
---

# Reuniao: Context Manager — 5 Gaps para 5/5

## Pauta

**Contexto**: Implementacao do roadmap completa (S0-S3). Nota subiu de 3.2/5 para 4.2/5. Paulo identificou 5 gaps restantes:

1. **Closed-loop learning** — assembler nao melhora com uso
2. **Memory lifecycle** — sem promotion/decay/pruning real
3. **Contradiction management** — sem deteccao ativa de conflitos
4. **Context usefulness metrics** — nao mede quais tokens influenciaram decisoes
5. **Eval-driven development** — evals nao integrados com CI

**Questoes**: Prioridade de cada gap. Abordagem tecnica. Sequenciamento. Risco de over-engineering.

**Restricoes**: 939 testes passando. Paulo alerta contra over-engineering Sprint 3. Cognitive payloads manuais. WorkingSet simples demais.

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE c/ sequenciamento | P0=context usefulness, P1=eval CI, P2+=lifecycle/learning/contradiction. Medir primeiro, construir depois. Usefulness deve ter <5% overhead. |
| evolution-agent | CONCERN | Risco de building lifecycle policy sem dados de usefulness. Hard-gate P2 em 2 semanas de P0 signal. |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | Episode summaries como base para learning loop. Compaction antes de eventos brutos. |
| ontology-manager | APPROVE c/ constraint | Max 3 novos tipos. Extend DomainEvent, nao criar paralelos. LearningSignal e MemoryDecayEvent como variants. |
| data-ingestor | APPROVE | Compaction agressiva: raw events retidos apenas episode atual + 1 anterior. |
| wiki-expert | APPROVE | Tier badge em wiki pages (Core/Working/Archived). Defer hypothesis conflict UI. |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | CONCERN (bloqueante) | EpisodeSummary TTL silenciosamente deleta workspace constraints (bug critico). HypothesisInvalidated nao valida que prior_event_id existe. successful_steps sempre vazio. |
| linter | CONCERN | uuid_v4_simple nao-deterministico sob testes paralelos. |
| retrieval-engineer | APPROVE | Gap 3 (eval CI) executavel HOJE com metricas existentes. Gap 2 requer referenced_community_ids em EpisodeSummary. Gap 1 defer ate 50+ observacoes. |
| memory-synthesizer | APPROVE | Compaction mandatoria em episode boundaries. Cap: 10MB raw, 500 summaries ativos, 30-day archival TTL. |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | APPROVE | Extend DomainEvent, nao duplicar. ContextUsefulnessScore como domain event. |
| graphctx-expert | APPROVE c/ YAGNI hold | NAO auto-tunar ScoringWeights ate 50+ observacoes reais. Eval CI com MRR >= 0.80 + DepCov >= 0.95. |
| arch-validator | APPROVE | Component placement validado. Nenhuma violacao nova. Max 2-3 traits novos em domain. |
| test-runner | APPROVE | TDD plans para top 3: usefulness proxy, memory lifecycle TTL, eval Recall@K. |
| frontend-dev | APPROVE | Build: learning indicator, tier badge, context budget bar. Defer: hypothesis conflicts UI, eval dashboard. |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | CONCERN | Closed-loop parcialmente resolvido (Reflexion/LATS). Contradiction aberto. Token attribution caro demais para runtime — usar proxy (tool success). Borrowar MemGPT tiered model. |

---

## Conflitos

### Conflito 1: Prioridade P0 — Context usefulness vs Eval CI

**Chief-architect**: P0 = context usefulness metrics
**Graphctx-expert**: P0 = eval CI (executavel hoje com metricas existentes)
**Research-agent**: P0 = compaction + learning loop

**Resolucao**: **Chief-architect vence com ajuste**. Context usefulness e P0 porque habilita todas as outras decisoes. Porem, eval CI e executavel em paralelo (usa infra existente) — logo ambos sao P0 com trabalho paralelo. Compaction ja existe (S3-T4 implementou archival). P0 = {usefulness + eval CI} em paralelo.

### Conflito 2: Validator encontrou bugs criticos

**Validator**: EpisodeSummary TTL silenciosamente deleta workspace constraints. HypothesisInvalidated aceita prior_event_id inexistente.

**Resolucao**: **Validator vence**. Esses sao bugs de correcao, nao features. Devem ser corrigidos ANTES de qualquer gap novo. Sao P-1 (pre-requisitos).

### Conflito 3: Closed-loop learning timing

**Chief-architect**: P3 (defer ate P0 ter dados)
**Research-agent**: Reflexion pattern viavel agora
**Graphctx-expert**: YAGNI hold ate 50+ observacoes

**Resolucao**: **Graphctx-expert + chief-architect vencem**. Learning sem dados e especulacao. Hard-gate em 50+ observacoes de usefulness.

---

## Consensos

1. **UNANIME**: Medir antes de otimizar — context usefulness precede learning loop
2. **UNANIME**: Eval CI executavel hoje com infra existente (metrics.rs ja tem MRR/DepCov)
3. **UNANIME**: Contradiction management e prematuro — menor prioridade
4. **UNANIME**: Extend DomainEvent, nao criar tipos paralelos (max 3 novos)
5. **FORTE**: Compaction como unidade duravel (summaries sobrevivem, raw events descartados)
6. **FORTE**: WorkingSet precisa de aging mas NAO agora (P2+)
7. **BLOQUEANTE (validator)**: Fix TTL promotion bug + prior_event_id validation ANTES de features novas

---

## Decisoes

1. **P-1 (BUG FIXES, bloqueantes)**:
   - Fix: EpisodeSummary TTL promotion automatica quando workspace-scoped constraints presentes
   - Fix: HypothesisInvalidated valida que prior_event_id resolve para evento existente
   - Fix: Populat successful_steps/failed_attempts em from_events (ou deprecar)

2. **P0 (paralelo)**:
   - Context usefulness proxy: tag context chunks com IDs, score por tool success post-response
   - Eval CI: 10-15 golden cases com MRR >= 0.80, DepCov >= 0.95. Gate em PRs que tocam retrieval.

3. **P1**: Memory lifecycle
   - Promotion policy baseada em usefulness data (requer P0)
   - Decay: summaries sem re-acesso apos N episodios → archival
   - Hard limits: 10MB raw events, 500 summaries ativos, 30-day archival TTL

4. **P2**: Closed-loop learning
   - Hard-gated em 50+ observacoes de usefulness
   - Pattern: Reflexion (episode summary → adjust assembly)
   - NAO auto-tunar ScoringWeights sem dados suficientes

5. **P3**: Contradiction management
   - Defer ate memory volume justifique
   - Quando implementar: flag-then-resolve, nao auto-prune

6. **Novos tipos aprovados (max 3)**:
   - `ContextUsageSignal` (EventType variant) — which blocks were referenced
   - `MemoryDecayEvent` (EventType variant) — triggered by age/irrelevance
   - `referenced_community_ids: Vec<String>` campo em EpisodeSummary

7. **Operational limits (SRE)**:
   - Max raw event buffer: 10MB before forced compaction
   - Max active summaries: 500 before archival pruning
   - Archival TTL: 30 days
   - Context usefulness overhead: <5% assembler latency

---

## Plano TDD

### P-1 Bug Fix: TTL Promotion

```
RED:
  #[test]
  fn ttl_promoted_to_permanent_when_workspace_constraint_present() {
      // events with ConstraintLearned scope=workspace-local
      // assert: summary.ttl_policy == TtlPolicy::Permanent
  }

  #[test]
  fn ttl_stays_run_scoped_when_no_workspace_constraints() {
      // events with scope=run-local only
      // assert: summary.ttl_policy == TtlPolicy::RunScoped
  }

GREEN: Modify from_events() to inspect ConstraintLearned scope before assigning ttl_policy.
REFACTOR: Extract fn infer_ttl_policy(events) -> TtlPolicy.
VERIFY: cargo test -p theo-domain -- episode
```

### P-1 Bug Fix: prior_event_id validation

```
RED:
  #[test]
  fn hypothesis_invalidated_rejects_nonexistent_prior() {
      // prior_event_id = "evt-999" (not in event list)
      // assert: validate_cognitive_event_in_context() returns Err
  }

GREEN: New fn validate_cognitive_event_in_context(type, payload, known_events) that checks prior_event_id existence.
REFACTOR: Compose with existing validate_cognitive_event.
VERIFY: cargo test -p theo-domain -- event
```

### P0: Context Usefulness Proxy

```
RED:
  #[test]
  fn usefulness_score_positive_when_context_file_appears_in_tool_call() {
      // Arrange: assembled context with src/auth.rs, agent calls read("src/auth.rs")
      // Assert: usefulness_score > 0.0 for auth.rs community
  }

  #[test]
  fn usefulness_score_zero_when_context_not_referenced() {
      // Arrange: assembled context with src/db.rs, agent only touches auth.rs
      // Assert: usefulness_score == 0.0 for db.rs community
  }

GREEN: fn compute_usefulness(assembled_communities, tool_calls) -> HashMap<String, f64>
REFACTOR: Integrate into ContextMetrics.
VERIFY: cargo test -p theo-application -- usefulness
```

### P0: Eval CI

```
RED:
  #[test]
  fn eval_mrr_above_floor() {
      // 10 golden queries with expected file hits
      // assert: aggregate_mrr >= 0.80
  }

GREEN: Eval fixture in theo-benchmark with known queries.
REFACTOR: Parameterize queries from TOML config.
VERIFY: cargo test -p theo-benchmark -- eval
```

---

## Action Items

- [ ] **validator** — Fix TTL promotion bug em EpisodeSummary::from_events — P-1 (imediato)
- [ ] **validator** — Fix prior_event_id validation — P-1 (imediato)
- [ ] **validator** — Populate ou deprecar successful_steps/failed_attempts — P-1
- [ ] **retrieval-engineer** — Implementar eval CI com 10-15 golden cases — P0
- [ ] **chief-architect** — Implementar context usefulness proxy (tag + score) — P0
- [ ] **ontology-manager** — ADR: ContextUsageSignal + MemoryDecayEvent variants — P0
- [ ] **memory-synthesizer** — Definir promotion/decay policy baseada em P0 data — P1 (apos 2 semanas de P0)
- [ ] **graphctx-expert** — YAGNI hold: doc em ADR que ScoringWeights nao auto-tuna sem 50+ observacoes — P0
- [ ] **infra/SRE** — Implementar hard limits (10MB raw, 500 summaries, 30-day TTL) — P1
- [ ] **frontend-dev** — Context budget bar + tier badge em wiki pages — P1
- [ ] **research-agent** — Prototipar Reflexion-style learning loop apos 50+ observacoes — P2

---

## Veredito Final

**REVISED**: Os 5 gaps sao reais mas desiguais em prioridade. Sequenciamento revisado: P-1 (bug fixes criticos do validator) → P0 (usefulness metrics + eval CI em paralelo) → P1 (lifecycle com dados reais) → P2 (learning loop gated em 50+ observacoes) → P3 (contradiction defer). Principio central: **medir antes de otimizar, corrigir antes de construir**.
