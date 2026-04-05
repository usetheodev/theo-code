# Harness Engineering — Documento Tecnico

> "Agent = Model + Harness"
> — Martin Fowler, Birgitta Bockeler, 2026

> "Our most difficult challenges now center on designing environments, feedback loops, and control systems."
> — Ryan Lopopolo, OpenAI, 2026

> "Each new session begins with no memory of what came before."
> — Justin Young, Anthropic, 2025

---

## 1. O que e Harness Engineering

Harness Engineering e a disciplina de projetar, construir e otimizar toda a infraestrutura ao redor de um modelo de linguagem para que ele funcione como um agente de codigo autonomo e confiavel.

O termo emergiu como shorthand para "tudo em um AI agent exceto o modelo em si". Em dezembro de 2025, os modelos atingiram qualidade suficiente para tarefas autonomas de longa duracao. A partir desse ponto, o gargalo deixou de ser a inteligencia do modelo e passou a ser a qualidade do ambiente onde ele opera.

A equipe da OpenAI demonstrou isso de forma contundente: construiram um produto inteiro com **zero linhas de codigo manual** — 1 milhao de linhas geradas por Codex em 5 meses, com 3 engenheiros e throughput de 3.5 PRs/engenheiro/dia. O segredo nao foi um modelo melhor — foi um harness melhor.

### Definicao formal (Fowler/Bockeler)

No contexto de coding agents, o harness tem duas camadas concentricas:

- **Inner harness (builder)**: system prompt, mecanismo de retrieval de codigo, orquestracao, sandbox, tools — construido pelo criador do coding agent
- **Outer harness (user)**: guides especificos do projeto, sensors customizados, skills, hooks, conventions — construido pelo usuario/time para seu caso de uso

```
┌─────────────────────────────────────────────┐
│              OUTER HARNESS                   │
│  .theo/theo.md, skills, hooks, conventions   │
│  ┌─────────────────────────────────────────┐ │
│  │           INNER HARNESS                 │ │
│  │  Agent loop, tools, sandbox, compaction │ │
│  │  ┌───────────────────────────────────┐  │ │
│  │  │           MODEL                   │  │ │
│  │  │  (GPT, Claude, Ollama, Codex)     │  │ │
│  │  └───────────────────────────────────┘  │ │
│  └─────────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

Um harness bem construido serve dois objetivos: (1) aumenta a probabilidade de o agente acertar de primeira, e (2) fornece um feedback loop que auto-corrige o maximo de problemas possivel antes que cheguem aos olhos humanos.

---

## 2. Guides e Sensors — O Framework Conceitual

Fowler/Bockeler definem dois tipos de controles que formam a base de todo harness:

### Guides (Feedforward Controls)

Antecipam o comportamento do agente e direcionam ANTES da execucao. Aumentam a probabilidade de resultado correto na primeira tentativa.

| Guide | Tipo | Exemplo |
|---|---|---|
| Coding conventions | Inferential | AGENTS.md, .theo/theo.md |
| Bootstrap instructions | Computational + Inferential | Skill com instrucoes + script de setup |
| Code mods | Computational | Tool com acesso a OpenRewrite recipes |
| Architecture docs | Inferential | ARCHITECTURE.md, ADRs |
| LSP integration | Computational | Language Server como guide em tempo real |
| API documentation | Inferential | Skills com specs de API |

### Sensors (Feedback Controls)

Observam DEPOIS que o agente age e permitem auto-correcao. Particularmente poderosos quando produzem sinais otimizados para consumo pelo LLM — por exemplo, mensagens de linter customizadas que incluem instrucoes para a correcao.

| Sensor | Tipo | Exemplo |
|---|---|---|
| Structural tests | Computational | ArchUnit, boundary checks, module deps |
| Linters com mensagens para LLM | Computational | Custom lints com "remediation instructions" |
| Type checkers | Computational | rustc, tsc, mypy |
| Code review por AI | Inferential | Sub-agent reviewer, LLM judge |
| Doom loop detection | Computational | Ring buffer de tool calls repetidos |
| Circuit breaker | Computational | Parar apos N falhas consecutivas |
| Mutation testing | Computational | Valida qualidade dos testes |

**Principio fundamental**: Feedback-only = agente repete os mesmos erros. Feedforward-only = agente codifica regras mas nunca descobre se funcionaram. **Os dois juntos formam o loop de correcao.**

```
Guide (feedforward)              Sensor (feedback)
      │                                │
      ▼                                ▼
  "Aqui esta o que              "O teste falhou
   voce deve fazer"              porque X. Tente Y."
      │                                │
      └──────────► MODEL ◄─────────────┘
                     │
                     ▼
                  ACTION
```

### Computational vs Inferential

| Propriedade | Computational | Inferential |
|---|---|---|
| Execucao | CPU, deterministic | GPU/NPU, probabilistic |
| Velocidade | Milissegundos | Segundos |
| Custo | Negligivel | Tokens + compute |
| Confiabilidade | ALTA | MEDIA |
| Exemplos | Testes, linters, type checkers | LLM review, AI judge, reflexao |
| Quando usar | Tudo que pode ser deterministic | Julgamento semantico |

**Regra de ouro**: Computational sensors capturam problemas estruturais de forma confiavel (codigo duplicado, complexidade ciclomatica, cobertura, drift arquitetural). Inferential sensors abordam problemas semanticos de forma probabilistica (logica redundante, over-engineering, brute-force fixes). **Nenhum dos dois captura de forma confiavel**: diagnostico errado de issues, over-engineering, features desnecessarias, especificacoes mal interpretadas.

---

## 3. O Steering Loop — Como o Harness Evolui

O harness nao e estatico. Evolui com o uso atraves de um loop de melhoria continua:

```
1. Agent executa task
2. Humano observa resultado
3. Identifica falha recorrente
4. Adiciona Guide (prevencao) ou Sensor (deteccao)
5. Proxima execucao e melhor
6. Goto 1
```

**Insight critico de Fowler**: O PROPRIO agente pode ajudar a melhorar o harness. Coding agents tornam muito mais barato construir controles customizados:

- Escrever testes estruturais
- Gerar regras de linting a partir de padroes observados
- Scaffoldar linters customizados
- Criar how-to guides a partir de "arqueologia" do codebase
- Documentar patterns recorrentes como skills

A OpenAI codificou isso como "golden principles" com garbage collection recorrente — background Codex tasks que escaneiam desvios, atualizam quality grades, e abrem PRs de refatoracao.

---

## 4. Timing — Keep Quality Left

Fowler/Bockeler distribuem controles pelo lifecycle de desenvolvimento segundo custo, velocidade e criticidade:

### Pre-integracao (rapido, cada commit)

- LSP guides (computational feedforward)
- Architecture documentation (inferential feedforward)
- Fast test suites + basic code review agent (computational + inferential feedback)
- MCP servers com acesso a conhecimento do time
- Skills com documentacao de API

### Post-integracao (pipeline)

- Mutation testing (computational feedback — valida qualidade dos testes)
- Code review arquitetural mais amplo (inferential feedback)
- Repeticao de todos os sensors pre-integracao
- Verificacao de coverage e boundary tests

### Monitoramento continuo (drift detection)

- Dead code detection
- Analise de qualidade da cobertura de testes
- Dependency scanning
- Degradacao de SLOs em runtime
- AI judges amostrando qualidade de respostas
- Deteccao de anomalias em logs

---

## 5. Environment Legibility — O Insight da OpenAI

A equipe do Codex (Lopopolo) descobriu que o bottleneck nao era o modelo — era o ambiente estar sub-especificado:

> "Early progress was slower than we expected, not because Codex was incapable, but because the environment was underspecified."

### Principio: Give a map, not a 1000-page manual

A OpenAI tentou a abordagem "one big AGENTS.md" e **falhou**:

- **Context e recurso escasso.** Arquivo gigante de instrucoes ocupa espaco do task, do codigo e dos docs relevantes.
- **Muita orientacao vira nao-orientacao.** Quando tudo e "importante", nada e.
- **Rota instantaneamente.** Manual monolitico vira cemiterio de regras obsoletas.
- **Dificil de verificar.** Um blob nao se presta a checks mecanicos (coverage, freshness, ownership).

### Solucao: AGENTS.md como indice, nao enciclopedia

```
AGENTS.md          ← ~100 linhas, mapa com ponteiros
ARCHITECTURE.md    ← visao top-level de dominios e camadas
docs/
├── design-docs/   ← catalogados e indexados, com status de verificacao
├── exec-plans/    ← planos versionados (ativos, completos, tech-debt)
├── product-specs/ ← especificacoes funcionais
├── references/    ← llms.txt de deps externas
├── DESIGN.md
├── FRONTEND.md
├── QUALITY_SCORE.md  ← grades por dominio, rastreando gaps
├── RELIABILITY.md
└── SECURITY.md
```

Isso habilita **progressive disclosure**: agentes comecam com um ponto de entrada pequeno e estavel e sao ensinados onde buscar mais, em vez de serem sobrecarregados de inicio.

### Agent legibility como objetivo

> "From the agent's point of view, anything it can't access in-context while running effectively doesn't exist."

Conhecimento em Google Docs, threads de Slack ou cabeca das pessoas NAO EXISTE para o agente. Tudo deve ser codificado no repositorio como artefatos versionados (codigo, markdown, schemas, planos executaveis).

A OpenAI favoreceu dependencias e abstracoes que pudessem ser completamente internalizadas pelo agente no repo. Tecnologias "chatas" tendem a ser mais faceis para agentes modelarem por composability, estabilidade de API, e representacao no training set.

---

## 6. Architecture Enforcement — Invariantes, nao Microgerenciamento

A OpenAI nao prescreve implementacoes — enforce invariantes:

> "By enforcing invariants, not micromanaging implementations, we let agents ship fast without undermining the foundation."

### Layered Domain Architecture

Cada dominio de negocio e dividido em camadas fixas com direcoes de dependencia validadas mecanicamente:

```
Types → Config → Repo → Providers → Service → Runtime → UI
```

Cross-cutting concerns (auth, connectors, telemetry, feature flags) entram por uma unica interface explicita: **Providers**. Qualquer outra coisa e proibida e enforced mecanicamente.

### Enforcement mecanico

- Custom linters (escritos pelo proprio Codex) validam:
  - Structured logging
  - Naming conventions para schemas e tipos
  - File size limits
  - Platform-specific reliability requirements
- As mensagens de erro dos lints incluem **instrucoes de remediacao** que sao injetadas no contexto do agente — um tipo positivo de prompt injection.

> "In a human-first workflow, these rules might feel pedantic. With agents, they become multipliers: once encoded, they apply everywhere at once."

---

## 7. O Harness para Agentes de Longa Duracao

A Anthropic (Justin Young) descreve o padrao para agentes que trabalham por horas ou dias:

### O Problema

Agentes de longa duracao trabalham em sessoes discretas. Cada nova sessao comeca **sem memoria** do que veio antes. Dois failure modes dominam:

1. **One-shot tendency**: Agente tenta fazer tudo de uma vez, fica sem contexto no meio, deixa features half-implemented e sem documentacao.
2. **Premature completion**: Apos algumas features implementadas, agente olha ao redor, ve progresso, e declara o trabalho feito.

### Solucao: Initializer + Coding Agent Pattern

```
Session 1 (Initializer Agent):
  - Prompt especializado para setup inicial
  - Cria init.sh (script de dev server)
  - Cria progress.txt (log de trabalho)
  - Gera feature list em JSON (200+ features, todas "passes: false")
  - Commit inicial documentando arquivos adicionados

Session 2..N (Coding Agent):
  1. pwd — verificar diretorio
  2. Ler progress.txt + git log — se orientar
  3. Ler feature list — escolher feature de maior prioridade nao completa
  4. Rodar init.sh — verificar que app funciona (E2E basico)
  5. Implementar UMA feature
  6. Testar end-to-end (como usuario faria, nao como dev)
  7. Commit com mensagem descritiva + atualizar progress.txt
  8. Deixar ambiente em clean state (code mergeavel para main)
```

### Feature List em JSON (nao Markdown)

```json
{
  "category": "functional",
  "description": "New chat button creates a fresh conversation",
  "steps": [
    "Navigate to main interface",
    "Click the 'New Chat' button",
    "Verify a new conversation is created",
    "Check that chat area shows welcome state"
  ],
  "passes": false
}
```

JSON porque o modelo e **menos propenso a editar/sobrescrever** comparado a Markdown.

Instrucoes strongly-worded: *"It is unacceptable to remove or edit tests because this could lead to missing or buggy functionality."*

### Failure Modes e Solucoes

| Problema | Initializer Agent | Coding Agent |
|---|---|---|
| Declara vitoria cedo demais | Feature list com TODAS as requirements | Le feature list, escolhe UMA |
| Deixa ambiente com bugs | Repo git + progress.txt | Le progress + git log, roda teste basico |
| Marca features como done sem testar | Feature list detalhada | Self-verify ANTES de marcar "passes" |
| Gasta tempo descobrindo como rodar o app | Escreve init.sh | Le init.sh no inicio |

---

## 8. Entropy e Garbage Collection

A OpenAI descobriu que agentes replicam patterns existentes no repo — mesmo os subotimos. Isso causa drift inevitavel.

### Problema

> "Our team used to spend every Friday (20% of the week) cleaning up 'AI slop.' Unsurprisingly, that didn't scale."

### Solucao: Golden Principles + GC Recorrente

1. Codificar "golden principles" no repositorio (regras opinativas e mecanicas)
2. Cadencia regular de background Codex tasks que:
   - Escaneiam desvios
   - Atualizam quality grades
   - Abrem PRs de refatoracao targeted
3. A maioria pode ser revisada em < 1 minuto e auto-merged

> "Technical debt is like a high-interest loan: it's almost always better to pay it down continuously in small increments than to let it compound."

Taste humano e capturado UMA VEZ, depois enforced continuamente em cada linha de codigo.

---

## 9. Harnessability e Ambient Affordances

Nem todo codebase e igualmente ameno a harness. Fowler/Bockeler introduzem o conceito de **harnessability**:

- Linguagens tipadas → type-checking como sensor natural
- Modulos com fronteiras bem definidas → regras de constraint arquitetural
- Frameworks que abstraem detalhes → reduzem espaco de decisao do agente
- Legacy com tech debt → harness e mais necessario onde e mais dificil de construir

### Ambient Affordances (Ned Letcher)

> "Structural properties of the environment itself that make it legible, navigable, and tractable to agents operating within it."

**Greenfield**: Pode incorporar harnessability desde o dia 1 — escolhas de tecnologia e arquitetura determinam quao governavel o codebase sera.

**Legacy**: Harness e mais necessario onde e mais dificil de construir. Paradoxo central do harness engineering.

---

## 10. Harness Templates

Fowler propoe que harnesses podem ser "templatezados" por topologia de servico:

```
CRUD API Template:
  Guides: REST conventions, DB migration patterns, error handling
  Sensors: Integration tests, schema validation, endpoint coverage

Event Processing Template:
  Guides: Idempotency rules, schema evolution, retry patterns
  Sensors: Data validation, lineage tracking, SLO monitoring

Frontend App Template:
  Guides: Component patterns, accessibility rules, state management
  Sensors: Visual regression, a11y audit, bundle size check
```

### Ashby's Law

> "A regulator must have at least as much variety as the system it governs."

Um LLM pode produzir quase qualquer coisa. Commitar-se a uma topologia **reduz o espaco de possibilidades**, tornando um harness abrangente mais atingivel. Definir topologias e um "variety-reduction move."

Times podem comecar a escolher tech stacks e estruturas parcialmente baseado em **quais harnesses ja estao disponiveis**.

---

## 11. Categorias de Regulacao

Fowler distingue tres categorias do que o harness deve regular:

### Maintainability Harness

A mais desenvolvida. Computational sensors capturam problemas estruturais de forma confiavel. Inferential controls abordam problemas semanticos probabilisticamente.

**Limitacao**: Nenhum dos dois captura de forma confiavel: diagnostico errado, over-engineering, features desnecessarias.

### Architecture Fitness Harness

Guides e sensors que definem e verificam fitness functions arquiteturais:

- Skills com requirements de performance + performance tests como feedback
- Conventions de observabilidade (logging standards) + debugging instructions que pedem reflexao sobre qualidade dos logs

### Behaviour Harness

O elefante na sala. Como garantir que o app funciona como deveria?

Abordagem atual:
- Feedforward: Especificacao funcional (de prompt curto a multi-file descriptions)
- Feedback: Test suite gerada por AI com coverage/mutation analysis + testes manuais

> "This approach puts a lot of faith into the AI-generated tests, that's not good enough yet."

O pattern de **approved fixtures** (fixtures pre-aprovadas que servem de ground truth) funciona seletivamente em algumas areas, mas nao e resposta geral.

---

## 12. O Papel do Humano

> "A coding agent has none of this: no social accountability, no aesthetic disgust at a 300-line function, no intuition that 'we don't do it that way here,' and no organisational memory."

Desenvolvedores humanos trazem um harness implicito: convencoes absorvidas, sensibilidade a complexidade, consciencia organizacional, passo deliberado que cria espaco para reflexao.

O harness explicito tenta externalizar essa experiencia implicita, mas so ate certo ponto. O objetivo nao e eliminar input humano — e **direciona-lo para onde e mais importante**.

---

## 13. Problemas em Aberto

| Problema | Status | Fonte |
|---|---|---|
| Medir qualidade do harness | Sem metrica padrao (equivalente a code coverage?) | Fowler |
| Harness coherence | Guides e sensors podem contradizer — sem deteccao automatica | Fowler |
| Behavioral harness | Testes gerados por AI nao sao confiaveis o suficiente | Fowler |
| Harness drift | Harness desatualizado em relacao ao codigo — sem deteccao | Fowler |
| Trade-off decisions | Quando signals apontam em direcoes diferentes, quem decide? | Fowler |
| Legacy codebases | Baixa harnessability — harness mais necessario onde mais dificil | Fowler |
| Architectural coherence over years | Nao se sabe como evolui em sistema 100% agent-generated | OpenAI |
| Where human judgment adds most leverage | Ainda aprendendo onde codificar julgamento humano | OpenAI |
| Single vs multi-agent | Melhor um agente generalista ou especializados? | Anthropic |
| Generalization beyond web dev | Findings aplicaveis a outros dominios? | Anthropic |

---

## 14. O Theo como Harness Engineering Platform

O Theo foi projetado desde o inicio como plataforma de harness engineering. Mapeamento:

### Guides (Feedforward) implementados

| Guide | Como no Theo | Status |
|---|---|---|
| System prompt | .theo/system-prompt.md (substitui default) | COMPLETO |
| Project context | .theo/theo.md (auto-init + AI enrichment via `theo init`) | COMPLETO |
| Skills | 10 bundled + project + global (.theo/skills/) | COMPLETO |
| Architecture docs | Project context com arquitetura real gerada por AI | COMPLETO |
| Bootstrap instructions | Auto-init cria .theo/ automaticamente no primeiro run | COMPLETO |
| Coding conventions | Injetadas no system prompt e project context | COMPLETO |

### Sensors (Feedback) implementados

| Sensor | Como no Theo | Tipo | Status |
|---|---|---|---|
| Doom loop detection | DoomLoopTracker (ring buffer de tool calls) | Computational | COMPLETO |
| Circuit breaker | PilotLoop (Closed/Open/HalfOpen) | Computational | COMPLETO |
| Corrective guidance | HeuristicReflector (classify_failure → guidance) | Computational | COMPLETO |
| Context loop | Re-orientacao periodica com progresso | Computational | COMPLETO |
| Compaction | Truncamento heuristico com preservacao de pares | Computational | COMPLETO |
| Git progress tracking | SHA comparison entre loops do Pilot | Computational | COMPLETO |
| Governance policy engine | Impact analysis, risk assessment | Computational | COMPLETO |
| Command validator | Rejeicao de patterns perigosos pre-fork | Computational | COMPLETO |

### Infraestrutura de Harness

| Conceito | OpenAI Codex | Anthropic Claude | Theo |
|---|---|---|---|
| Sandbox | bwrap + seccomp + landlock | Container isolation | bwrap > landlock > noop cascade |
| Tools | Generic (terminal, file) | Bash, Read, Write | 21 builtin + plugins + codebase_context |
| Context mgmt | AGENTS.md (map) + compaction | progress.txt + git + compaction | .theo/theo.md + compaction + context loops |
| Safety | Sandbox + model training | Strongly-worded instructions | Governance engine + command validator + sandbox |
| Memory | AGENTS.md + progress file | claude-progress.txt + git | Session persistence + memory + learnings + snapshots |
| Autonomous loop | Codex tasks (background, 6+ hours) | Initializer + Coding Agent pattern | Pilot (circuit breaker, dual-exit, reflector) |
| Code intelligence | Chrome DevTools + observability stack | Puppeteer MCP | GRAPHCTX (parser + graph + retrieval) on-demand |
| Self-improvement | Golden principles + GC recorrente | N/A | HeuristicReflector + LearningStore (planned) |
| Environment legibility | docs/ as system of record, progressive disclosure | Feature list JSON + init.sh + progress.txt | Auto-init + .theo/ + git |
| Architecture enforcement | Custom linters (Codex-generated) with remediation msgs | Strongly-worded instructions | Governance policy engine + impact analysis |
| Drift detection | Recurring GC agent tasks + quality grades | N/A | Planned (Phase 2-3 Reflector) |

### Diferenciais do Theo

1. **Code intelligence nativo** (GRAPHCTX): 16 linguagens, semantic search, graph attention — nenhum outro coding agent tem isso integrado no harness
2. **Self-improvement** (Reflector): Aprende com falhas e melhora o harness automaticamente — alinhado com o steering loop de Fowler
3. **Model-agnostic** (25 providers): Commodity model, premium harness — o valor esta no harness, nao no modelo
4. **Pilot com circuit breaker**: Autonomo com safety nets — alinhado com Anthropic mas com protecoes mais robustas
5. **Auto-init + AI enrichment**: Ambiente legivel desde o primeiro run — alinhado com OpenAI mas com geracao automatica

---

## Referencias

### Artigos Primarios

1. Bockeler, Birgitta. "Harness Engineering for Coding Agent Users." martinfowler.com, 02 April 2026.
2. Lopopolo, Ryan. "Harness Engineering: Leveraging Codex in an Agent-First World." openai.com, 11 February 2026.
3. Young, Justin. "Effective Harnesses for Long-Running Agents." anthropic.com, 26 November 2025.

### Video Sources

4. Michael Bolin (OpenAI). "Codex, Harness Engineering, and the Real Future of Coding Agents." Interview, 2025-2026.
5. Dex Horthy (HumanLayer). "No Vibes Allowed: Solving Hard Problems in Complex Codebases." Conference talk, 2025-2026.
6. "WTF is Harness Engineer & Why is it Important." Video essay, 2025-2026.

### Conceitos Referenciados

7. Ashby's Law of Requisite Variety (Cybernetics)
8. Fitness Functions (Thoughtworks Radar)
9. Approved Fixtures Pattern (Augmented Coding Patterns)
10. Progressive Disclosure (Information Architecture)
11. Ralph Wiggum Loop (ghuntley.com/loop)
