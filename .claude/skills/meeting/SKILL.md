---
name: meeting
description: Convoca reuniao com TODO o time de agentes (16 personas). Cada agente analisa o tema, debate, e produz uma ata estruturada em .claude/meetings/. Use para decisoes estrategicas, tecnicas, arquiteturais, ou qualquer tema que impacte o projeto.
user-invocable: true
allowed-tools: Bash(date *) Bash(git *) Bash(ls *) Bash(cat *) Read Write Edit Glob Grep Agent
argument-hint: "<tema da reuniao>"
---

# Meeting — Reuniao do Time Completo

Convoque TODOS os 16 agentes para uma reuniao sobre: **$ARGUMENTS**

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

#### Grupo 1 — Estrategia (paralelo)
- `chief-architect` — impacto no pipeline e execucao
- `evolution-agent` — impacto no sistema como um todo

#### Grupo 2 — Conhecimento (paralelo)
- `knowledge-compiler` — impacto na wiki e knowledge base
- `ontology-manager` — impacto na taxonomia e conceitos
- `data-ingestor` — impacto na ingestao de dados
- `wiki-expert` — impacto na experiencia da wiki

#### Grupo 3 — Qualidade (paralelo)
- `validator` — riscos de corrupcao e consistencia
- `linter` — impacto na saude do sistema
- `retrieval-engineer` — impacto na busca e ranking
- `memory-synthesizer` — impacto na sintese e datasets

#### Grupo 4 — Engineering (paralelo)
- `code-reviewer` — qualidade de codigo
- `graphctx-expert` — impacto no GRAPHCTX
- `arch-validator` — violacoes arquiteturais
- `test-runner` — impacto em testes
- `frontend-dev` — impacto na UI

#### Grupo 5 — Pesquisa
- `research-agent` — estado da arte e referencias externas

### 4. Debate

Apos coletar todas as posicoes:
- Identifique **conflitos** (agentes que discordam)
- Identifique **consenso** (agentes que concordam)
- Resolva conflitos com argumentos, nao autoridade
- Se houver REJECT de chief-architect ou validator → o tema precisa ser revisado

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
participants: 16
---

# Reuniao: <tema>

## Pauta
<contexto e questoes>

## Posicoes por Agente

### Estrategia
| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE | ... |
| evolution-agent | CONCERN | ... |

### Conhecimento
| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | ... |
| ... | ... | ... |

### Qualidade
| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | APPROVE | ... |
| ... | ... | ... |

### Engineering
| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | APPROVE | ... |
| ... | ... | ... |

### Pesquisa
| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE | ... |

## Conflitos
<debates e resolucoes>

## Decisoes
1. <decisao 1>
2. <decisao 2>

## Action Items
- [ ] <quem> — <o que> — <quando>

## Veredito Final
**<VERDICT>**: <justificativa em 1-2 frases>
```

## Regras

1. **Todos participam** — nenhum agente pode ser omitido
2. **Conflito obrigatorio** — se todos concordam sem debate, forca contra-argumentos
3. **Ata obrigatoria** — sem ata, a reuniao nao aconteceu
4. **Veredito claro** — APPROVED/REJECTED/DEFERRED/REVISED, sem ambiguidade
5. **Action items concretos** — quem, o que, quando
6. **Historico preservado** — atas nunca sao editadas apos salvas
7. **TDD obrigatorio** — toda decisao que envolva codigo deve incluir um plano TDD (RED-GREEN-REFACTOR) nos action items. Sem plano de testes = decisao incompleta.

## TDD na Ata

Toda decisao que envolva codigo DEVE incluir na ata:

```markdown
## Plano TDD
Para cada action item que envolve codigo:
1. RED: <que teste sera escrito primeiro>
2. GREEN: <que implementacao minima>
3. REFACTOR: <que limpeza>
4. VERIFY: `cargo test -p <crate>`
```

O test-runner DEVE validar que o plano TDD e viavel. Se nao for → CONCERN obrigatorio.
