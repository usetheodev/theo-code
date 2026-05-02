---
id: 20260429-143744
date: 2026-04-29
topic: "Arquitetura do pipeline de validacao e refinamento SOTA para features existentes do Theo Code"
verdict: REVISED
participants: 16
---

# Reuniao: Pipeline de Validacao e Refinamento SOTA

## Pauta

**Contexto:** O objetivo e validar e refinar todas as features existentes do Theo Code (72 tools, 26 providers, GRAPHCTX, agent runtime) ate nivel SOTA — sem criar features novas. Inspirado no `algorithmic-research-loop` (16 agentes especialistas) e Karpathy autoresearch (keep/discard).

**Questoes a decidir:**
1. Qual a arquitetura do loop? (agentes, fases, criterios de parada)
2. Como definir thresholds SOTA com base nas pesquisas existentes?
3. Como estruturar o "agente usuario" para teste E2E com LLM real?
4. Como o "agente coletor" armazena e expoe metricas?
5. Como atualizar `.claude/` para alinhar hooks/skills/agents ao estado real?
6. Qual a relacao com o benchmark existente em `apps/theo-benchmark/`?

**Restricoes conhecidas:**
- NAO criar features novas
- Respeitar ADR-010 (boundary contract)
- Respeitar allowlists com sunsets
- Budget de LLM finito

**Branch:** `develop` @ `cd2d047`

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | CONCERN | Escopo muito amplo como iniciativa unica. Mistura 3 workstreams com riscos diferentes. 16-agent swarm contradiz pesquisa. Propoe 4 fases sequenciadas. |
| evolution-agent | CONCERN | Conflata 3 problemas distintos. Loop autonomo de 16 agentes e over-engineering para um problema finito. Propoe fases independentes com gates. |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | Pipeline produz dados empiricos que enriquecem wiki. Exige schema canonico de metricas e change events rastreáveis. |
| ontology-manager | CONCERN | "SOTA" e sobrecarregado — confunde benchmark externo com gate interno. Propoe terminologia canonica: `research-benchmark-reference` vs `dod-gate`, `e2e-probe-agent` vs "user agent". |
| data-ingestor | N/A | Nao convocado (nao listado nos 16 originais — wiki-expert substitui) |
| wiki-expert | N/A | Coberto pelo knowledge-compiler nesta pauta |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | CONCERN | Loop autonomo pode laundering allowlists, gaming testes, corromper CLAUDE.md. Exige: loop sem write access a allowlists, human-gated merge, append-only metrics. |
| linter | APPROVE | Metricas de saude atuais sao solidas (0 clippy, 0 arch violations). Sunsets de allowlists precisam auditoria mensal. DAP E2E (Gap 6.1) critico. |
| retrieval-engineer | APPROVE | GRAPHCTX precisa de validacao contra held-out corpora. Recall@5<0.92 e Recall@10<0.95 sao gaps reais. Propoe 6 metricas com floors. |
| memory-synthesizer | APPROVE | Dados de runs reais sao o melhor sinal para sintese. Exige schema estruturado por run, granularidade subtask, cross-provider delta como artefato. |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | CONCERN | Loop autonomo viola TDD contract se nao for estruturado como RED-GREEN-REFACTOR. Exige: full suite como gate, proibir modificacao de allowlists, human-gated merge. |
| graphctx-expert | APPROVE | Metricas de retrieval ja existem em Rust (`RetrievalMetrics`). Smoke report mostra `avg_context_size_tokens=0` — critico resolver antes de confiar no loop. Propoe 6 floors incluindo per-language Recall@5>=0.85. |
| arch-validator | APPROVE | Pipeline em Python via subprocess respeita ADR-010 completamente. Nenhuma violacao identificada. Documentar regra explicita em architecture.md. |
| frontend-dev | APPROVE+CONCERN | Sem impacto direto na UI. Exige schema versionado em `theo-api-contracts` antes de qualquer dashboard. Defer UI para Phase 2 desktop. |
| test-runner | N/A | Coberto pelo code-reviewer e graphctx-expert nesta pauta |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE+CONCERN | **Evidencia forte CONTRA 16 agentes:** verifiers -0.8 SWE-Bench, multi-candidate -2.4. Todos os sistemas de producao usam 2-5 agentes. Caminho de 50% → 70%: self-evolution loop (+4.8), representacao estruturada (+16.8), execucao incremental. NAO mais agentes. |

---

## Conflitos

### Conflito 1: Escopo — Monolito vs Fases

**chief-architect + evolution-agent + code-reviewer** vs **conhecimento + qualidade + pesquisa**

- Os 3 agentes de estrategia/engineering exigem decomposicao em fases independentes
- Os agentes de conhecimento/qualidade aprovam o conceito mas com condicoes
- **Resolucao:** UNANIMIDADE em decompor. Nenhum agente defende monolito. Aprovado em fases.

### Conflito 2: 16 Agentes vs Simplicidade

**Proposta original (16 agentes)** vs **research-agent + chief-architect + evolution-agent**

- research-agent: evidencia empirica CONTRA. Verifiers: -0.8, multi-candidate: -2.4. Nenhum sistema de producao usa >5 agentes.
- chief-architect: "o unico modulo que ajuda e self-evolution loop. Um agente com bons prompts supera 16."
- evolution-agent: "algorithmic-research-loop e para fronteiras de pesquisa abertas. Nosso problema e finito."
- **Resolucao:** REJEITADO o modelo de 16 agentes. Aprovado: 1 agente com self-evolution loop (keep/discard), max 4-5 roles se necessario. Decisao baseada em evidencia, nao opiniao.

### Conflito 3: Autonomia do Loop vs Human Gate

**validator + code-reviewer** vs **eficiencia do loop**

- validator: "Loop sem write access a allowlists. Human-gated merge. Append-only metrics."
- code-reviewer: "Cada commit autonomo deve passar full suite + check-arch + check-sizes + check-unwrap."
- **Resolucao:** APROVADO human-gated merge. Loop propoe, humano aprova. Loop PROIBIDO de modificar: `.claude/rules/*-allowlist.txt`, `CLAUDE.md`, gate configs. Sem excecoes.

### Conflito 4: Terminologia

**ontology-manager** levanta que "SOTA threshold" e ambiguo.

- **Resolucao:** APROVADA terminologia canonica:
  - `research-benchmark-ref` = referencia externa de paper
  - `dod-gate` = gate interno de CI (ja existe)
  - `e2e-probe` = o que era "user agent"
  - `metrics-collector` = o que era "collector agent"
  - `refinement-cycle` = o que era "autonomous loop"

---

## Consensos

1. **Estender `apps/theo-benchmark/`** — unanimidade. Nao criar sistema paralelo.
2. **Schema canonico de metricas** — knowledge-compiler, memory-synthesizer, frontend-dev todos exigem.
3. **Human-gated merge** — validator, code-reviewer, chief-architect concordam.
4. **Fases sequenciadas** — todos os agentes de estrategia + engineering concordam.
5. **Resolver `avg_context_size_tokens=0`** antes de confiar no loop — graphctx-expert (critico).
6. **Floors de retrieval** — retrieval-engineer + graphctx-expert convergem nos mesmos numeros.

---

## Decisoes

### D1: Decompor em 4 fases sequenciadas

| Fase | Escopo | Entrega |
|------|--------|---------|
| **Phase 0** | Atualizar `.claude/` para refletir estado real | PR de housekeeping |
| **Phase 1** | E2E probe harness + metrics collector | Suite de validacao em `apps/theo-benchmark/e2e/` |
| **Phase 2** | SOTA thresholds baseados em evidencia | `docs/sota-thresholds.toml` + gates integrados |
| **Phase 3** | Refinement cycle (keep/discard) | Script em `apps/theo-benchmark/autoloop/` |

Cada fase e independentemente shippable. Phase N+1 so comeca quando Phase N esta validada.

### D2: Rejeitar modelo de 16 agentes

Baseado em evidencia empirica (Tsinghua ablation: verifiers -0.8, multi-candidate -2.4). Aprovado: single-agent refinement cycle com self-evolution pattern (keep/discard). Max 4-5 roles se ablation local provar beneficio.

### D3: Human-gated merge obrigatorio

O refinement cycle (Phase 3) propoe mudancas em worktree isolado. Humano aprova antes de merge. Loop PROIBIDO de modificar allowlists, CLAUDE.md, gate configs.

### D4: Terminologia canonica

| Termo proposto | Termo canonico | Razao |
|------|--------|---------|
| User agent | `e2e-probe` | Evita colisao com HTTP User-Agent e agent-runtime |
| Collector agent | `metrics-collector` | Explicito |
| Autonomous loop | `refinement-cycle` | Nao e autonomo — e human-gated |
| SOTA threshold (externo) | `research-benchmark-ref` | Citacao de paper com data |
| SOTA threshold (interno) | `dod-gate` | Ja existe no sistema |

### D5: Metricas de retrieval com floors

| Metrica | Floor | Fonte |
|---------|-------|-------|
| MRR | >= 0.90 | retrieval-engineer + graphctx-expert |
| Recall@5 | >= 0.92 | retrieval-engineer |
| Recall@10 | >= 0.95 | retrieval-engineer |
| DepCov | >= 0.96 | retrieval-engineer (margem sobre 0.967 atual) |
| nDCG@5 | >= 0.85 | graphctx-expert |
| Per-language Recall@5 | >= 0.85 each | graphctx-expert |

### D6: Pre-requisito critico — resolver zero-convergence

`avg_context_size_tokens=0` no smoke report e bloqueante. Sem telemetria de contexto, o refinement cycle nao pode distinguir retrieval bom de ruim. Resolver ANTES de Phase 3.

### D7: Schema canonico de metricas

Definir em `docs/schemas/benchmark-run.schema.json` antes da Phase 1 emitir dados. Campos obrigatorios: `run_id`, `model_id`, `model_version`, `timestamp`, `task_id`, `subtask_results[]`, `pass_rate`, `context_bytes`, `tool_calls[]`, `cost_usd`.

---

## Action Items

- [ ] **Paulo** — Phase 0: Atualizar `.claude/agents/`, `.claude/skills/`, `.claude/rules/`, `CLAUDE.md` para estado real — **esta semana (ate 2026-05-02)**
- [ ] **Paulo** — Phase 1: Criar `apps/theo-benchmark/e2e/` com probe suite + metrics collector — **ate 2026-05-09**
- [ ] **Paulo** — Phase 1 prereq: Resolver `avg_context_size_tokens=0` no smoke report — **ate 2026-05-05**
- [ ] **Paulo** — Phase 2: Criar `docs/sota-thresholds.toml` com research-benchmark-refs citados — **ate 2026-05-12**
- [ ] **Paulo** — Phase 2: Integrar thresholds no `make check-sota-dod` — **ate 2026-05-16**
- [ ] **Paulo** — Phase 3: Criar `apps/theo-benchmark/autoloop/` com refinement cycle (keep/discard) — **ate 2026-05-23**
- [ ] **Paulo** — Escrever ADR para terminologia canonica (D4) — **ate 2026-05-02**

---

## Plano TDD

### Phase 0 (config update — sem codigo de producao)
Nao requer TDD. Verificacao: `make check-arch && make check-sizes && make check-sota-dod --quick`.

### Phase 1 (E2E probe harness)
1. **RED:** Escrever `tests/test_e2e_probe.py` com assertions para cada subcommand do CLI (17) e tool categories criticas. Testes falham porque o probe runner nao existe.
2. **GREEN:** Implementar `e2e/probe_runner.py` que invoca `theo --headless` contra LLM real (ou mock para CI). Testes passam.
3. **REFACTOR:** Extrair schema de metricas para modulo separado. Testes continuam passando.
4. **VERIFY:** `python -m pytest tests/test_e2e_probe.py -v`

### Phase 2 (SOTA thresholds)
1. **RED:** Escrever `tests/test_thresholds.py` que carrega `docs/sota-thresholds.toml`, valida schema, e verifica que cada threshold tem `source`, `date`, `confidence`. Falha porque arquivo nao existe.
2. **GREEN:** Criar `docs/sota-thresholds.toml` com os 6 floors de D5 + research-benchmark-refs.
3. **REFACTOR:** Integrar comparacao no `check-sota-dod`.
4. **VERIFY:** `make check-sota-dod --quick`

### Phase 3 (refinement cycle)
1. **RED:** Escrever `tests/test_autoloop.py` com: max iterations (5), budget cap, keep/discard logic, proibicao de modificar allowlists. Falha porque autoloop nao existe.
2. **GREEN:** Implementar `autoloop/cycle.py` — run benchmark, compare, propose patch, human gate, keep/discard.
3. **REFACTOR:** Extrair hypothesis generator.
4. **VERIFY:** `python -m pytest tests/test_autoloop.py -v`

---

## Veredito Final

**REVISED**: Proposta aprovada com modificacoes significativas baseadas em evidencia:
1. Decomposta em 4 fases sequenciadas (nao monolito)
2. Rejeitado modelo de 16 agentes (evidencia contra: -0.8 a -8.4 pts em benchmarks)
3. Aprovado single-agent refinement cycle com keep/discard
4. Human-gated merge obrigatorio
5. Terminologia canonica definida
6. Pre-requisito critico: resolver zero-convergence antes de Phase 3
