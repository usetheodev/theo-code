---
id: 20260409-134738
date: 2026-04-09
topic: "Context Manager 4.7→5.0: 5 gaps finais para excelencia de producao"
verdict: REVISED
participants: 16
---

# Reuniao: Context Manager 4.7 → 5.0

## Pauta

**Contexto**: Sistema avaliado em 4.7/5 apos implementacao completa dos roadmaps S0-S3 e P-1 a P2. Paulo identificou 5 gaps finais de nivel refinamento (nao arquitetural).

**Questoes a decidir**:
1. Prioridade dos 5 gaps
2. Abordagem tecnica minima viavel para cada
3. Sequenciamento com gates de dados
4. Novos tipos necessarios (ontologia)

**Restricoes**: 981 testes passando. Principio "medir antes de otimizar". Max novos tipos limitado.

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE c/ sequenciamento | P0.5=Memory Typing (pre-requisito), P1=Failure Learning (maior valor pratico), P1.5=Multi-agent isolation, P2=Hypothesis (manual only), P3=Causal (DEFER). Total: 6 sessoes. |
| evolution-agent | APPROVE c/ YAGNI | Gaps sao refinamento. Risco principal: over-engineering hypothesis engine. Minimal struct sem automacao e o ponto de parada correto. |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | Episode summaries com memory_tier. Compaction episode-boundary. |
| ontology-manager | APPROVE | 3 novos tipos + 2 extensoes. MemoryLifecycle 4 variants correto. Nao criar MemoryManager god-struct. |
| data-ingestor | APPROVE | Per-agent directories (.theo/agents/{id}/). Advisory flock em shared paths. |
| wiki-expert | APPROVE | Tier badges em wiki pages. Reasoning trace panel = P0 trust surface. |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | APPROVE c/ invariantes | Memory typing e o gap mais perigoso (wrong promotion = permanent noise). Canonical = append-only, supersede nao overwrite. Auto-pruning so com HypothesisInvalidated event (nunca por idade). |
| linter | APPROVE | 3 novos tipos e aceitavel. Nenhum god-struct. |
| retrieval-engineer | APPROVE | Causal: tag ContextBlock com block_id UUID no assembly. Citation extractor pos-response. Shadow mode antes de atualizar weights. |
| memory-synthesizer | APPROVE | Hypothesis competition: confidence normalizada por grupo (soma=1.0). Prune abaixo de 0.1 ou apos 3 rounds sem melhoria. |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | APPROVE | Extend tipos existentes, nao duplicar. |
| graphctx-expert | APPROVE c/ shadow mode | Causal bridge: block_id → citation extractor → EMA update. Shadow mode obrigatorio antes de produção. |
| arch-validator | APPROVE | Todos os 5 cabem no modelo de boundaries. Nenhuma violacao nova. |
| test-runner | APPROVE | TDD plans para P0.5, P1, P1.5 concretos. Nenhum gap de testes existente. |
| frontend-dev | APPROVE | Build: tier badges, reasoning trace, constraint display (read-only). Defer: confidence badges, override UI, per-agent views. |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE | MemGPT para memory typing, Reflexion para failure learning, LATS para hypothesis. Causal attribution continua aberto — proxy e a abordagem correta. |

---

## Conflitos

### Conflito 1: Causal Usefulness — implementar ou deferir?

**Graphctx-expert**: P0, implementar citation extractor agora
**Chief-architect**: P3, DEFER — collect data first
**Research-agent**: Proxy e suficiente, causal e open research

**Resolucao**: **Chief-architect vence com compromisso**. DEFER causal attribution completo. IMPLEMENTAR apenas block_id tagging no assembly (custo zero) como infraestrutura. Citation extractor em shadow mode (loga mas nao atualiza weights). Decisao de produtificar apos 50+ episodes com dados reais.

### Conflito 2: Hypothesis Engine — quanto automatizar?

**Memory-synthesizer**: Confidence normalizada por grupo com auto-prune em 0.1
**Chief-architect**: Manual only, sem auto-pruning
**Validator**: Auto-prune so com HypothesisInvalidated event, nunca por idade

**Resolucao**: **Validator vence**. Confidence e campo, competicao e opcional, pruning SOMENTE por evento explicito. Nenhum auto-prune por idade ou threshold. Isso evita o cenario catastrofico de deletar hipotese valida.

### Conflito 3: Memory Typing — 3 ou 4 tiers?

**Chief-architect propoe**: 4 (ephemeral/episodic/reusable/canonical)
**Ontology-manager propoe**: 4 (ephemeral/episodic/reusable/canonical)
**Memory-synthesizer propoe**: 4 com compaction por tier

**Resolucao**: **Consenso em 3 tiers operacionais**: Active (runtime), Cooling (pos-episode), Archived (long-term). O quarto tier (Canonical) ja e coberto por `TtlPolicy::Permanent`. Nao duplicar conceito. 3 variants = KISS.

---

## Consensos

1. **UNANIME**: Memory typing e pre-requisito para todo o resto
2. **UNANIME**: Causal attribution completo e prematuro — block_id tagging como infra OK
3. **UNANIME**: Hypothesis pruning somente por evento explicito, nunca automatico
4. **UNANIME**: Multi-agent = per-agent directories + flock em shared
5. **FORTE**: Failure learning threshold = 3 ocorrencias minimo
6. **FORTE**: Extend tipos existentes, max 3 novos tipos
7. **FORTE**: Shadow mode obrigatorio antes de atualizar weights em producao

---

## Decisoes

### 1. Novos tipos aprovados (3)

| Tipo | Crate | Descricao |
|---|---|---|
| `MemoryLifecycle` enum | theo-domain/episode.rs | `Active`, `Cooling`, `Archived` (3 variants) |
| `Hypothesis` struct | theo-domain (novo) | `id`, `confidence: f64`, `evidence_event_ids`, `superseded_by` |
| `FailurePattern` struct | theo-domain (novo) | `signature_hash`, `occurrence_count`, `suggested_constraint`, `first_seen`, `last_seen` |

### 2. Extensoes de tipos existentes

| Tipo | Campo novo | Crate |
|---|---|---|
| `EpisodeSummary` | `lifecycle: MemoryLifecycle` | theo-domain |
| `WorkingSet` | `agent_id: Option<String>` + `merge()` | theo-domain |
| `ContextBlock` | `block_id: String` (UUID tagging) | theo-domain/graph_context.rs |

### 3. Sequenciamento

```
P0.5  Memory Typing (MemoryLifecycle enum + EpisodeSummary field)       → theo-domain
P1    Failure Learning (FailurePattern + threshold-based constraint)    → theo-domain + theo-application
P1.5  Multi-agent isolation (agent_id + merge + per-agent dirs)        → theo-domain + theo-agent-runtime
P2    Hypothesis Engine (Hypothesis struct, manual confidence, NO auto-prune) → theo-domain
P2.5  Block ID tagging + citation extractor (shadow mode)              → theo-engine-retrieval + theo-agent-runtime
P3    Causal attribution (DEFER — gated em 50+ episodes com block_id data)
```

### 4. Invariantes (validator)

- Canonical memory = append-only. Supersede, never overwrite.
- Hypothesis pruning ONLY via explicit HypothesisInvalidated event.
- Failure→constraint promotion requires N≥3 independent occurrences.
- Cross-agent memory transfer requires EpisodeSummary with quarantine TTL (≥1h).
- Citation extractor runs in shadow mode until eval CI gates pass.
- Attribution scores are advisory, never authoritative for deletion.

### 5. UI surfaces (frontend-dev)

**Build now**: Memory tier badges em wiki, reasoning trace panel, read-only constraint display.
**Defer**: Confidence badges, constraint override UI, per-agent memory views.

---

## Plano TDD

### P0.5: Memory Typing

```
RED:
  #[test]
  fn episode_summary_has_lifecycle_field()
  #[test]
  fn lifecycle_defaults_to_active()
  #[test]
  fn lifecycle_serde_roundtrip_all_variants()

GREEN: Adicionar MemoryLifecycle enum (Active/Cooling/Archived) + campo em EpisodeSummary com #[serde(default)].
REFACTOR: Default impl retorna Active.
VERIFY: cargo test -p theo-domain -- lifecycle
```

### P1: Failure Learning

```
RED:
  #[test]
  fn recurring_error_above_threshold_generates_constraint()
  #[test]
  fn isolated_error_does_not_generate_constraint()
  #[test]
  fn failure_pattern_tracks_occurrence_count()

GREEN: FailurePattern struct + extract_failure_patterns(events, threshold) -> Vec<FailurePattern>.
REFACTOR: Integrar com from_events() para popular learned_constraints automaticamente.
VERIFY: cargo test -p theo-domain -- failure
```

### P1.5: Multi-agent Isolation

```
RED:
  #[test]
  fn working_set_clone_is_independent()
  #[test]
  fn working_set_merge_combines_hot_files()
  #[test]
  fn working_set_merge_preserves_parent_constraints()

GREEN: agent_id field + merge(other, strategy) method em WorkingSet.
REFACTOR: MergeStrategy enum (SubAgentFirst/ParentFirst).
VERIFY: cargo test -p theo-domain -- working_set
```

### P2: Hypothesis Engine

```
RED:
  #[test]
  fn hypothesis_created_with_default_confidence()
  #[test]
  fn hypothesis_confidence_adjustable()
  #[test]
  fn hypothesis_superseded_preserves_original()

GREEN: Hypothesis struct em theo-domain. Manual confidence updates. superseded_by: Option<String>.
REFACTOR: Integrar com DomainEvent HypothesisFormed payload.
VERIFY: cargo test -p theo-domain -- hypothesis
```

### P2.5: Block ID Tagging (shadow)

```
RED:
  #[test]
  fn context_block_has_unique_block_id()
  #[test]
  fn citation_extractor_finds_file_paths_in_tool_calls()
  #[test]
  fn shadow_mode_logs_but_does_not_update_weights()

GREEN: block_id field em ContextBlock. extract_citations() pure function. Shadow flag no scorer.
REFACTOR: Integrar com ContextMetrics.
VERIFY: cargo test -p theo-engine-retrieval -- citation
```

---

## Action Items

- [ ] **chief-architect** — Implementar MemoryLifecycle enum + campo em EpisodeSummary — P0.5
- [ ] **validator** — Implementar invariantes de promotion (append-only canonical, quarantine TTL) — P0.5
- [ ] **runtime-engineer** — Implementar FailurePattern detection em episode boundaries — P1
- [ ] **ontology-manager** — Implementar Hypothesis struct em theo-domain — P2
- [ ] **arch-validator** — Implementar agent_id + merge() em WorkingSet — P1.5
- [ ] **graphctx-expert** — Implementar block_id em ContextBlock + citation extractor shadow — P2.5
- [ ] **test-runner** — Validar TDD plans e escrever testes para todos os levels — contínuo
- [ ] **frontend-dev** — Tier badges + reasoning trace panel — P1 (UI)
- [ ] **research-agent** — Documentar ADR: "Causal attribution deferred — proxy + block_id as infra" — P0.5
- [ ] **infra/SRE** — Per-agent directory structure + flock implementation — P1.5

---

## Veredito Final

**REVISED**: Os 5 gaps sao unanimemente reconhecidos como reais. Abordagem revisada do proposto: (1) Causal attribution completo deferido — apenas block_id tagging como infra. (2) Hypothesis engine sem auto-prune — manual only com invariante de evento explicito. (3) Memory typing simplificado de 4 para 3 tiers (Active/Cooling/Archived). (4) Multi-agent via per-agent directories, nao shared mutable state. Principio central: **"refinamento disciplinado — cada gap tem versao minima viavel e gate de dados para evolucao"**.

---

## Addendum: Review do Paulo (pos-reuniao)

**Data**: 2026-04-09
**Avaliacao**: 4.8/5 (arquitetura). Cinco refinamentos incorporados.

### Refinamento 1: MemoryLifecycle precisa de POLITICA, nao so classificacao

O time definiu 3 tiers mas sem comportamento real. Sem politica, vira enum decorativo.

**Regras adotadas**:

```
Active:
  - sempre elegivel para assembler
  - prioridade alta no ranking
  - TTL: duracao da run

Cooling:
  - elegivel condicional (usefulness_score > 0.3)
  - prioridade media
  - TTL: 24h apos episode boundary

Archived:
  - nunca entra no assembler por default
  - so via lookup explicito
  - TTL: 30 dias ou Permanent (se promoted)
```

Transicoes:
- Active → Cooling: ao fechar episode
- Cooling → Archived: apos TTL ou enforce_limits()
- Cooling → Active: se re-acessado (LRU promotion)
- Archived → Permanent: se reusado em ≥2 runs independentes

### Refinamento 2: Hypothesis degradacao automatica (nao delete)

"NO auto-prune" era conservador demais. Hipoteses mortas acumulam e poluem context.

**Solucao**: Degradacao por status, nao delecao.

```rust
pub enum HypothesisStatus {
    Active,       // em uso, entra no assembler
    Stale,        // nao usada em N iteracoes → assembler ignora
    Superseded,   // contradita/substituida → nunca entra
}
```

Regras:
- Nao usada em 10 iteracoes → `Stale`
- Contradita parcialmente → confidence reduzida (nao deletada)
- Superseded por evento explicito → `Superseded`
- **NUNCA deletada** — mantida para auditoria

Assembler: so inclui `Active`. `Stale` e `Superseded` ficam no storage mas nao poluem contexto.

### Refinamento 3: Fallback automatico leve para Hypothesis Engine

Dependencia total do modelo para emitir eventos cognitivos e fragil.

**Fallback adotado**:
- Detectar padroes de tentativa repetida (mesma acao 3+ vezes)
- Inferir hipotese implicita: "agente pode estar tentando X repetidamente"
- Registrar como `HypothesisFormed` com `confidence: 0.3` (low)
- Rotular como `source: "inferred"` no payload (distinto de `source: "explicit"`)

Nao substitui o modelo — complementa quando o modelo nao emite.

### Refinamento 4: Shadow mode com feedback parcial

Block ID tagging sem feedback loop desperdi¢a o investimento. Mesmo em shadow mode, alimentar EMA.

**Regra adotada**:
- Blocos citados pelo agente: boost score via EMA existente
- Blocos ignorados: decay score via EMA
- Fator de confianca reduzido (alpha=0.1 em shadow vs 0.3 em producao)
- Gate: so promover a producao quando eval CI confirmar que MRR nao regrediu

### Refinamento 5: FailurePattern → MemoryLifecycle binding

Constraints derivados de falhas precisam de lifecycle explicito.

**Regra adotada**:
```
Failure-derived constraint:
  Criacao → Active (run-scoped)
  Se persistir apos run → Cooling (task-scoped, 24h)
  Se reusada em ≥2 runs → Promoted para Permanent (workspace-scoped)
```

Isso garante que constraints uteis sobrevivem e transientes morrem.

### Riscos residuais identificados

| # | Risco | Mitigacao |
|---|---|---|
| 1 | Context bloat por hipoteses nao degradadas | HypothesisStatus::Stale apos 10 iteracoes |
| 2 | Lifecycle sem politica real | Regras explicitas de transicao por tier |
| 3 | Dependencia do modelo para memoria | Fallback automatico com confidence=0.3 |
| 4 | Shadow mode sem valor | Feedback parcial via EMA com alpha=0.1 |

### Avaliacao do Paulo

> "Voces nao estao mais construindo um context manager — estao construindo um sistema que controla a evolucao do proprio contexto."

Nota: **4.8/5 (arquitetura)**
