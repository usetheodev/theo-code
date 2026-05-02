---
name: meeting
description: Convoca reuniao com TODO o time de agentes (22 personas). Cada agente analisa o tema, debate, e produz uma ata estruturada em .claude/meetings/. Use para decisoes estrategicas, tecnicas, arquiteturais, ou qualquer tema que impacte o projeto.
user-invocable: true
allowed-tools: Bash(date *) Bash(git *) Bash(ls *) Bash(cat *) Read Write Edit Glob Grep Agent
argument-hint: "<tema da reuniao>"
---

# Meeting — Reuniao do Time Completo

Convoque TODOS os 22 agentes para uma reuniao sobre: **$ARGUMENTS**

## Protocolo

### 1. Abertura

Gere o ID da reuniao:

```!
date +%Y%m%d-%H%M%S
```

Branch e estado atual:

```!
git branch --show-current
git log --oneline -3
```

### 2. Pauta

Defina a pauta baseada no tema "$ARGUMENTS":
- Contexto: o que motivou essa reuniao
- Questoes a decidir
- Riscos a avaliar
- Restricoes conhecidas

### 3. Convocacao do Time

Convoque TODOS os agentes em grupos paralelos. Cada agente deve analisar o tema da perspectiva do seu dominio e retornar:
- **Posicao**: APPROVE / REJECT / CONCERN / ABSTAIN
- **Analise**: 2-5 frases do ponto de vista do seu dominio
- **Riscos**: o que pode dar errado
- **Recomendacoes**: o que faria diferente

#### Grupo 1 — Lideranca
- `cto-architect` — verdade do sistema, SOTA alignment, features reais vs fantasma

#### Grupo 2 — Arquitetos de Dominio: Runtime (paralelo)
- `agent-loop-architect` — impacto no ReAct cycle, compaction, convergencia
- `subagents-architect` — impacto em sub-agentes, delegacao, guardrails
- `agents-architect` — impacto em task/plan management
- `observability-architect` — impacto em metricas, OTel, dashboard
- `prompt-engineering-architect` — impacto em prompts, schemas, fencing

#### Grupo 3 — Arquitetos de Dominio: Engine (paralelo)
- `context-architect` — impacto em GRAPHCTX, retrieval, RRF
- `languages-architect` — impacto em parsing, Tree-Sitter, 14 langs
- `wiki-architect` — impacto na Code Wiki, BM25, enrichment

#### Grupo 4 — Arquitetos de Dominio: Infra (paralelo)
- `providers-architect` — impacto nos 26 providers, streaming, retry
- `model-routing-architect` — impacto em routing, cost optimization
- `memory-architect` — impacto em STM/WM/LTM, persistence
- `security-governance-architect` — impacto em sandbox, capabilities, secrets

#### Grupo 5 — Arquitetos de Dominio: Surface & Quality (paralelo)
- `cli-architect` — impacto nos 17 subcommands, UX
- `tools-architect` — impacto nos 72 tools, registry
- `debug-architect` — impacto no DAP, 11 debug tools
- `evals-architect` — impacto em benchmarks, metricas
- `self-evolution-architect` — impacto no ciclo de evolucao

#### Grupo 6 — Utilidade (paralelo)
- `arch-validator` — violacoes arquiteturais
- `code-reviewer` — qualidade de codigo
- `frontend-dev` — impacto na UI (Tauri/React)
- `test-runner` — impacto em testes

### 4. Debate

Apos coletar todas as posicoes:
- Identifique **conflitos** (agentes que discordam)
- Identifique **consenso** (agentes que concordam)
- Resolva conflitos com argumentos, nao autoridade
- Se houver REJECT de `cto-architect` → o tema precisa ser revisado obrigatoriamente

### 5. Veredito

```
APPROVED  — maioria aprova, sem REJECT critico
REJECTED  — bloqueios nao resolvidos
DEFERRED  — precisa de mais informacao
REVISED   — aprovado com modificacoes
```

### 6. Ata

Salve a ata em `.claude/meetings/YYYYMMDD-HHMMSS-<slug>.md` com esta estrutura:

```markdown
---
id: YYYYMMDD-HHMMSS
date: YYYY-MM-DD
topic: "<tema>"
verdict: APPROVED | REJECTED | DEFERRED | REVISED
participants: 22
---

# Reuniao: <tema>

## Pauta
<contexto e questoes>

## Posicoes por Agente

### Lideranca
| Agente | Posicao | Resumo |
|--------|---------|--------|
| cto-architect | APPROVE | ... |

### Arquitetos de Dominio: Runtime
| Agente | Posicao | Resumo |
|--------|---------|--------|
| agent-loop-architect | APPROVE | ... |
| subagents-architect | CONCERN | ... |
| agents-architect | APPROVE | ... |
| observability-architect | APPROVE | ... |
| prompt-engineering-architect | APPROVE | ... |

### Arquitetos de Dominio: Engine
| Agente | Posicao | Resumo |
|--------|---------|--------|
| context-architect | APPROVE | ... |
| languages-architect | APPROVE | ... |
| wiki-architect | APPROVE | ... |

### Arquitetos de Dominio: Infra
| Agente | Posicao | Resumo |
|--------|---------|--------|
| providers-architect | APPROVE | ... |
| model-routing-architect | APPROVE | ... |
| memory-architect | APPROVE | ... |
| security-governance-architect | APPROVE | ... |

### Arquitetos de Dominio: Surface & Quality
| Agente | Posicao | Resumo |
|--------|---------|--------|
| cli-architect | APPROVE | ... |
| tools-architect | APPROVE | ... |
| debug-architect | APPROVE | ... |
| evals-architect | APPROVE | ... |
| self-evolution-architect | APPROVE | ... |

### Utilidade
| Agente | Posicao | Resumo |
|--------|---------|--------|
| arch-validator | APPROVE | ... |
| code-reviewer | APPROVE | ... |
| frontend-dev | ABSTAIN | ... |
| test-runner | APPROVE | ... |

## Conflitos
<debates e resolucoes>

## Decisoes
1. <decisao 1>
2. <decisao 2>

## Action Items
- [ ] <quem> — <o que> — <quando>

## Plano TDD
Para cada action item que envolve codigo:
1. RED: <que teste sera escrito primeiro>
2. GREEN: <que implementacao minima>
3. REFACTOR: <que limpeza>
4. VERIFY: `cargo test -p <crate>`

## Veredito Final
**<VERDICT>**: <justificativa em 1-2 frases>
```

## Regras

1. **Todos participam** — nenhum agente pode ser omitido
2. **CTO tem veto** — REJECT do `cto-architect` bloqueia qualquer aprovacao
3. **Conflito obrigatorio** — se todos concordam sem debate, forca contra-argumentos
4. **Ata obrigatoria** — sem ata, a reuniao nao aconteceu
5. **Veredito claro** — APPROVED/REJECTED/DEFERRED/REVISED, sem ambiguidade
6. **Action items concretos** — quem, o que, quando
7. **Historico preservado** — atas nunca sao editadas apos salvas
8. **TDD obrigatorio** — toda decisao que envolva codigo deve incluir plano TDD nos action items
