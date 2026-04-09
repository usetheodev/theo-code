---
id: 20260409-145443
date: 2026-04-09
topic: "SOTA Precision Fix: Benchmark 14 repos revelou Precision=55%, 5 fixes para 75%"
verdict: REVISED
participants: 16
---

# Reuniao: SOTA Precision — "Sabe encontrar, nao sabe descartar"

## Pauta

**Contexto**: Benchmark de 14 repos reais (ripgrep→next.js) revelou Precision=55% como gargalo #1. Target SOTA: 75%. O sistema inclui demais — encontra contexto relevante mas nao filtra irrelevante.

**Metricas atuais vs SOTA**:
| Metrica | Atual | Target | Gap |
|---|---|---|---|
| Precision | 55% | 75% | +20% |
| Resume | 80% | 93% | +13% |
| Memory Reuse | 30% | 60% | +30% |
| Repetition | 15% | 8% | -7% |
| Recall | 85% | 92% | +7% |

**5 fixes propostos**: Penalidades, stability bonus, memory-before-graph, action blocking, hierarchical summarization.

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE | P0=penalidades (+10% precision), P0.5=stability bonus (drift+repetition), P1=memory-before-graph (reuse), P2=action blocking, P3=hierarchical (defer). Cada fix independente. |
| evolution-agent | APPROVE | Ratio Precision/Recall=0.65 confirma diagnostico. Penalizacao negativa e o fix correto. |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | EpisodeSummary como section reservada entre constraints e events. Max 10% do budget. |
| ontology-manager | APPROVE | ZERO tipos novos. Tudo com tipos existentes. Resistir a PenaltyConfig/BlockFilter. |
| data-ingestor | APPROVE | Pipeline de feedback ja existe mas nao esta alimentado com dados reais no runtime. |
| wiki-expert | APPROVE | Context utilization bar (tokens used/budget) como UI V1. |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | APPROVE c/ invariantes | Penalty floor=0.5 (nunca zerar score). Stability decay obrigatorio. Action blocker diferencia repeat vs retry-after-mutation. |
| linter | APPROVE | Zero tipos novos = zero complexity growth. |
| retrieval-engineer | APPROVE c/ condicoes | Penalidades como multiplicadores [0.5, 1.0], NUNCA aditivos. Stability bonus com decay=0.7/turn, cap=0.15. Community-level summary para mega-repos. |
| memory-synthesizer | APPROVE | Episode budget reservado (10% max), nao compete com structural. |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | APPROVE | Mudancas localizadas em context_assembler.rs e search.rs. Sem cross-crate. |
| graphctx-expert | APPROVE | Penalidades pos-score, nao pre-score. MRR nao pode cair abaixo de 0.84. |
| arch-validator | APPROVE | Sem violacoes de fronteira. Tudo dentro de application e retrieval. |
| test-runner | APPROVE | TDD plans para top 3 fixes concretos. Backward-compat via metodos novos. |
| frontend-dev | APPROVE | Context bar V1 (ratio tokens). Block provenance V2. Sem confidence badges. |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | CONCERN | "Compress better" > "discard more". Mainstream SOTA usa recency + tool-driven context, nao penalty math. Recomenda: alimentar record_feedback no runtime ANTES de implementar penalties. |

---

## Conflitos

### Conflito 1: Penalty-based scoring vs "compress better"

**Chief-architect**: Penalidades como multiplicadores [0.5, 1.0] no score
**Research-agent**: SOTA nao usa penalty scoring. Recency + tool-driven e o padrao. O real problema e que feedback_scores estao em 0.5 (default) — nao alimentados com dados reais.

**Resolucao**: **Ambos vencem**. Research-agent esta correto que o pipeline de feedback nao esta alimentado. Mas chief-architect esta correto que penalidades complementam feedback. **Sequencia revisada**: (1) Primeiro instrumentar record_feedback no runtime loop, (2) DEPOIS aplicar penalidades informadas por dados reais. Nao penalizar sem dados.

### Conflito 2: Stability bonus — risco de lock-in

**Graphctx-expert**: Decay=0.7/turn, cap=0.15
**Validator**: Decay obrigatorio, floor em 0.01 apos 5 turns sem uso

**Resolucao**: **Validator vence**. Bonus so se aplica com positive signal (tool use, citation), NAO por mera presenca. Decay exponencial: `bonus = 0.15 * 0.7^turns_without_use`. Floor natural em ~0.01 apos 5 turns.

---

## Consensos

1. **UNANIME**: Diagnostico correto — Precision=55% e o gargalo #1
2. **UNANIME**: Zero tipos novos — extend metodos existentes
3. **UNANIME**: Penalty floor=0.5 (nunca zerar score, proteger recall)
4. **UNANIME**: Stability bonus requer positive signal + decay
5. **FORTE**: Memory budget reservado (10% max), nao compete com structural
6. **FORTE**: Action blocker diferencia repeat_identical vs retry_after_mutation
7. **FORTE**: Hierarchical summary = community-level (Leiden), nao directory
8. **REVISADO**: Alimentar feedback pipeline ANTES de penalidades

---

## Decisoes

### 1. Sequenciamento revisado

```
P0:   Instrumentar record_feedback no runtime loop (dados reais)
P0.5: Scoring penalties informadas por feedback (multiplicadores [0.5, 1.0])
P1:   Stability bonus com decay (0.7/turn, cap 0.15, requer positive signal)
P1.5: Memory-before-graph (EpisodeSummary 10% budget reservado)
P2:   Action blocking (repeat != retry-after-mutation)
P3:   Hierarchical summary (defer ate P0-P2 validados)
```

### 2. Invariantes

- **Penalty floor**: score * multiplier onde multiplier ∈ [0.5, 1.0]. NUNCA zero.
- **Stability requires signal**: bonus so com tool use/citation, nao presenca.
- **Memory cap**: episodios no assembler limitados a 10% do budget total.
- **Action blocker**: whitelist tools idempotentes (read). So bloqueia mutacoes repetidas.
- **MRR gate**: nenhum fix pode fazer MRR cair abaixo de 0.84 no eval CI.

### 3. Metricas de validacao por fix

| Fix | Metrica | Gate |
|---|---|---|
| P0 (feedback data) | feedback_scores populated > 0 | Verificar .theo/metrics/ |
| P0.5 (penalties) | Precision@5 ≥ 0.65 | eval_golden.rs |
| P1 (stability) | Drift ≥ 94% | ContextMetrics report |
| P1.5 (memory) | Memory Reuse ≥ 45% | EpisodeSummary referenced |
| P2 (blocking) | Repetition ≤ 9% | ContextMetrics report |

---

## Plano TDD

### P0: Instrumentar feedback no runtime

```
RED:
  #[test]
  fn feedback_scores_populated_after_tool_execution()
  // Verify that after a tool call referencing a file, the corresponding
  // community's feedback_score is updated (> default 0.5)

GREEN: In run_engine.rs, after tool call completion, call
       context_metrics.compute_usefulness() and feed results to
       assembler.record_feedback(community_id, score)

VERIFY: cargo test -p theo-agent-runtime -- feedback
```

### P0.5: Scoring penalties

```
RED:
  #[test]
  fn penalty_floor_prevents_recall_collapse()
  // Apply max penalty (5 repetitions) → score >= 0.5 * unpunished

  #[test]
  fn penalty_zero_for_first_occurrence()
  // First time → multiplier = 1.0

  #[test]
  fn penalty_monotonically_decreasing()
  // More repetitions → lower score, but never below floor

GREEN: fn apply_repetition_penalty(score: f64, count: u32) -> f64 {
         (1.0 - 0.1 * count as f64).max(0.5) * score
       }

VERIFY: cargo test -p theo-application -- penalty
```

### P1: Stability bonus

```
RED:
  #[test]
  fn stability_bonus_decays_over_turns()
  // After 5 turns without positive signal → bonus < 0.01

  #[test]
  fn stability_bonus_requires_positive_signal()
  // Presence without tool use → no bonus

GREEN: fn compute_stability_bonus(turns_in_context: u32, has_positive_signal: bool) -> f64 {
         if !has_positive_signal { return 0.0; }
         0.15 * 0.7_f64.powi(turns_in_context as i32)
       }

VERIFY: cargo test -p theo-application -- stability
```

### P1.5: Memory before graph

```
RED:
  #[test]
  fn episode_constraints_appear_before_structural()
  // Constraints from EpisodeSummary positioned before graph blocks

  #[test]
  fn episode_budget_capped_at_10_percent()
  // Episode sections never exceed 10% of total budget

GREEN: Add episode: Option<&EpisodeSummary> parameter to assemble().
       Insert learned_constraints + failed_attempts after events section.
       Cap at budget * 0.10.

VERIFY: cargo test -p theo-application -- episode_constraints
```

---

## Action Items

- [ ] **runtime-engineer** — Instrumentar feedback pipeline (record_feedback apos tool calls) — P0
- [ ] **graphctx-expert** — Implementar apply_repetition_penalty com floor=0.5 — P0.5
- [ ] **retrieval-engineer** — Implementar stability bonus com decay=0.7 — P1
- [ ] **memory-synthesizer** — Integrar EpisodeSummary no assembler (10% cap) — P1.5
- [ ] **runtime-engineer** — Action blocker (repeat != retry-after-mutation) — P2
- [ ] **test-runner** — TDD plans para todos os fixes + MRR gate — continuo
- [ ] **frontend-dev** — Context utilization bar (tokens/budget ratio) — P1 (UI)

---

## Veredito Final

**REVISED**: Diagnostico unanimemente aprovado. Sequenciamento revisado: alimentar feedback pipeline ANTES de penalidades (research-agent corrigiu a ordem). Invariantes definidos (penalty floor=0.5, stability requer positive signal, memory cap=10%, MRR gate≥0.84). Zero tipos novos — tudo com tipos existentes. Principio: **"aprender a descartar começa por aprender o que foi util"**.
