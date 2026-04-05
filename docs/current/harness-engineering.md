# Harness Engineering — Documento Tecnico

> "Agent = Model + Harness"
> — Martin Fowler, 2025

## O que e Harness Engineering

Harness Engineering e a disciplina de projetar, construir e otimizar toda a infraestrutura ao redor de um modelo de linguagem para que ele funcione como um agente de codigo autonomo e confiavel. O modelo e commodity — o harness e o diferencial.

O conceito emergiu em dezembro de 2025 quando os modelos atingiram qualidade suficiente para tarefas autonomas de longa duracao. A partir desse ponto, o gargalo deixou de ser a inteligencia do modelo e passou a ser a qualidade do ambiente onde ele opera.

### Analogia

Se o modelo e um piloto de Formula 1, o harness e o carro, o pit stop, a telemetria, as barreiras de seguranca e a estrategia de corrida. Um piloto excelente em um carro ruim perde para um piloto bom em um carro excelente.

## Anatomia de um Harness

```
                    +---------------------------+
                    |        USER / CLI         |
                    +---------------------------+
                              |
                    +---------------------------+
                    |      OUTER HARNESS        |  ← construido pelo usuario
                    |  (CLAUDE.md, agents.md,   |
                    |   skills, hooks, config)  |
                    +---------------------------+
                              |
                    +---------------------------+
                    |      INNER HARNESS        |  ← construido pelo tool builder
                    |  (agent loop, tools,      |
                    |   sandbox, compaction,     |
                    |   context, memory)         |
                    +---------------------------+
                              |
                    +---------------------------+
                    |         MODEL             |  ← commodity (GPT, Claude, Ollama)
                    +---------------------------+
```

**Inner harness** (responsabilidade do builder): agent loop, tools, sandbox, context management, safety, memory.

**Outer harness** (responsabilidade do usuario/time): system prompt, project context, skills, hooks, conventions, architectural guides.

## Os 7 Pilares do Harness Engineering

### 1. Sandbox e Isolamento

O agente executa codigo arbitrario — o harness DEVE garantir que falhas nao causem dano.

**Principio**: O agente opera em um ambiente restrito onde pode fazer qualquer coisa DENTRO dos limites, mas nada fora.

**Implementacao tipica**:
- Container/namespace isolation (bubblewrap, landlock, seccomp)
- Filesystem: whitelist de paths leitura/escrita
- Network: bloqueio de IPs internos (SSRF), rate limiting
- Process: rlimits (CPU, memoria, file size, nproc)
- Environment: sanitizacao de variaveis (strip tokens, API keys)
- Command validation: rejeicao de patterns perigosos antes de fork

**Fonte**: Michael Bolin (OpenAI) confirma que o Codex usa "bubblewrap, seccomp, and landlock" para sandboxing.

### 2. Tools — Genericos, nao Especializados

A tendencia da industria e clara: **menos tools especializadas, mais tools genericas**.

**Principio**: O modelo ja sabe usar bash, ler arquivos e escrever codigo. O harness deve dar tools que o modelo entende nativamente, nao abstrair demais.

**Evidencia**: O OpenAI Codex removeu a maioria das tools especializadas e confia no modelo para usar tools genericas (terminal, file read/write). A performance MELHOROU.

```
BOM:  bash, read, write, edit, grep, glob
RUIM: create_react_component, add_database_migration, deploy_to_staging
```

**Excecoes aceitas**: Tools de meta-controle (done, batch, subagent) e tools de inteligencia (codebase_context, memory) que dao ao modelo capacidades que ele nao tem nativamente.

### 3. Context Engineering

O recurso mais precioso do agente e a context window. Cada token conta.

**Principio**: Colocar melhores tokens (nao mais tokens) no contexto do modelo.

**Tecnicas** (Dex Horthy, "No Vibes Allowed"):

| Tecnica | O que faz | Quando usar |
|---|---|---|
| **Just-in-time context** | Injetar informacao somente quando necessaria | Sempre — default |
| **Compaction** | Comprimir historico preservando informacao critica | Quando tokens > 80% da window |
| **On-demand retrieval** | Modelo pede contexto quando precisa (ex: codebase_context) | Tasks complexas |
| **Planning as compression** | Planos sao compressao de intencao | Inicio de tasks complexas |
| **Context loop** | Re-orientar o modelo periodicamente | A cada N iteracoes |

**Anti-pattern**: "Context stuffing" — jogar tudo na window e esperar que o modelo encontre o que precisa. Isso DEGRADA performance.

**Metricas**: Tokens uteis por turn, compaction rate, context utilization.

### 4. Guides e Sensors (Feedforward e Feedback)

Martin Fowler define dois tipos de controles no harness:

**Guides (Feedforward)** — controles que antecipam problemas ANTES da execucao:
- System prompt com instrucoes claras
- Project context (.theo/theo.md) com arquitetura real
- Architectural Decision Records (ADRs)
- Coding conventions e patterns do projeto
- Skills que injetam instrucoes especializadas

**Sensors (Feedback)** — controles que detectam problemas DEPOIS da execucao:
- Testes automatizados (cargo test, npm test)
- Linters com mensagens otimizadas para LLM
- Type checkers (rustc, tsc)
- Code review por sub-agente
- Doom loop detection
- Circuit breaker no pilot

**Principio**: Harness sem feedback e um tiro no escuro. Harness sem feedforward e tentativa-e-erro infinita. Os dois juntos formam o loop de correcao.

```
Guide (feedforward)          Sensor (feedback)
      |                            |
      v                            v
  "Aqui esta o que          "O teste falhou
   voce deve fazer"          porque X. Tente Y."
      |                            |
      +---------> MODEL <----------+
```

### 5. Environment Legibility

O harness deve manter o ambiente LEGIVEL para o modelo.

**Principio**: Se o modelo nao consegue entender rapidamente onde esta e o que foi feito, ele vai perder tempo (e tokens) tentando se orientar.

**Tecnicas**:
- Auto-init: criar .theo/theo.md automaticamente com contexto do projeto
- Clean state: forcar o ambiente a ficar limpo ao final de cada task
- Progress tracking: arquivo de progresso que o modelo le no inicio de cada sessao
- Git como memoria: commits descritivos que o modelo pode consultar
- File naming: diretorios e arquivos bem nomeados facilitam navegacao

**Fonte**: OpenAI Codex usa `claude-progress.txt` + git history como mecanismo de re-orientacao entre sessoes.

### 6. Safety e Governance

O harness e a camada de seguranca entre o modelo e o mundo real.

**Niveis de seguranca**:

| Nivel | O que protege | Mecanismo |
|---|---|---|
| **Model-level** | Respostas inseguras | Training, RLHF, system prompt |
| **Tool-level** | Execucao perigosa | Command validator, path traversal check |
| **Sandbox-level** | Escape do container | Namespace isolation, rlimits |
| **Governance-level** | Decisoes erradas | Policy engine, impact analysis |
| **Human-level** | Erros sistemicos | Approval gates, review |

**Principio de Bolin**: "You can't compromise on safety at all. It's baked in."

### 7. Memory e Persistencia

Agentes autonomos de longa duracao precisam de memoria entre sessoes.

**Tipos de memoria**:

| Tipo | Escopo | Exemplo |
|---|---|---|
| **Session** | Um run | Messages do turn atual |
| **Cross-session** | Projeto | .theo/learnings.json, session persistence |
| **Cross-project** | Global | ~/.config/theo/learnings.json |
| **Episodica** | Eventos | RunSnapshot com trajetorias completas |

**Principio**: O agente deve poder retomar trabalho onde parou, sem perder contexto.

**Tecnica da Anthropic**: "Each new session begins with no memory of what came before" — o harness resolve isso com progress files + git history + structured state.

## O Harness para Agentes de Longa Duracao

A Anthropic descreve um padrao especifico para agentes que rodam por horas ou dias:

### Initializer + Coding Agent Pattern

```
Session 1 (Initializer):
  - Setup environment (init.sh, deps, config)
  - Create progress tracking (progress.txt, feature list)
  - Commit initial state to git

Session 2..N (Coding Agent):
  1. Read progress file + git log (re-orient)
  2. Select highest-priority incomplete feature
  3. Run init script + verify environment
  4. Implement ONE feature
  5. Test end-to-end
  6. Commit + update progress file
  7. Leave environment in clean state
```

### Regras para Sessoes Longas

1. **Uma feature por sessao** — nao tentar fazer tudo de uma vez
2. **Commit frequente** — git como checkpoint, permite rollback
3. **Progress file como memoria** — JSON, nao markdown (evita edicoes acidentais)
4. **Clean state obrigatorio** — proxima sessao nao deve precisar de cleanup
5. **Testes E2E** — verificar como usuario, nao como desenvolvedor

## Computational vs Inferential Controls

Martin Fowler distingue dois tipos de execucao no harness:

### Computational (deterministic)

```
Input → Regra → Output (sempre o mesmo)
Custo: milissegundos
Exemplos: testes, linters, type checkers, coverage
Confiabilidade: ALTA
```

### Inferential (probabilistic)

```
Input → LLM → Output (pode variar)
Custo: segundos + tokens
Exemplos: code review por AI, classificacao de risco, reflexao
Confiabilidade: MEDIA
```

**Principio**: Use computational controls para tudo que pode ser deterministic. Reserve inferential controls para julgamento semantico.

| Problema | Control Type | Exemplo |
|---|---|---|
| Codigo nao compila | Computational | `cargo check` |
| Funcao muito longa | Computational | Linter com threshold |
| Codigo duplicado | Computational | Analise estrutural |
| Logica redundante | Inferential | LLM review |
| Over-engineering | Inferential | LLM review |
| Especificacao mal interpretada | Inferential | LLM + human review |

## Harness Templates

Fowler propoe que harnesses podem ser "templatezados" por tipo de projeto:

```
CRUD API Template:
  Guides: REST conventions, DB migration patterns, error handling
  Sensors: Integration tests, schema validation, endpoint coverage

Data Pipeline Template:
  Guides: Idempotency rules, schema evolution, retry patterns
  Sensors: Data validation, lineage tracking, SLO monitoring

Frontend App Template:
  Guides: Component patterns, accessibility rules, state management
  Sensors: Visual regression, a11y audit, bundle size check
```

## O Steering Loop — Como o Harness Evolui

O harness nao e estatico — ele evolui com o uso:

```
1. Agent executa task
2. Humano observa resultado
3. Identifica falha recorrente
4. Adiciona Guide (prevencao) ou Sensor (deteccao)
5. Proxima execucao e melhor
6. Goto 1
```

**Insight critico**: O PROPRIO agente pode ajudar a melhorar o harness:
- Escrever testes estruturais
- Gerar regras de linting a partir de padroes
- Documentar arqueologia do codebase
- Sugerir skills baseadas em tarefas repetitivas

Isso e exatamente o que o Self-Improving Pilot faz: o Reflector analisa falhas e gera learnings que melhoram o harness automaticamente.

## Problemas em Aberto

| Problema | Status na Industria |
|---|---|
| Medir qualidade do harness | Sem metrica padrao (equivalente a code coverage?) |
| Harness coherence | Guides e sensors podem contradizer — sem deteccao automatica |
| Behavioral harness | Testes gerados por AI ainda nao sao confiaveis o suficiente |
| Harness drift | Harness desatualizado em relacao ao codigo — sem deteccao |
| Trade-off decisions | Quando sensors conflitam, quem decide? |
| Legacy codebases | Baixa "harnessability" — dificil de instrumentar |

## O Theo como Harness Engineering Platform

O Theo foi projetado desde o inicio como uma plataforma de harness engineering. A tabela abaixo mostra o mapeamento entre conceitos da industria e a implementacao no Theo:

| Conceito | OpenAI Codex | Claude Code | Theo |
|---|---|---|---|
| Sandbox | bwrap + seccomp + landlock | Container isolation | bwrap > landlock > noop cascade |
| Tools | Generic (terminal, file) | Read, Write, Bash, etc. | 21 builtin + plugins + codebase_context |
| Context management | agents.md + compaction | CLAUDE.md + compaction | .theo/theo.md + compaction + context loops |
| Safety | Sandbox + model training | Permission system | Governance engine + command validator + sandbox |
| Memory | agents.md + progress file | CLAUDE.md + memory | Session persistence + memory + learnings + snapshots |
| Autonomous loop | Codex tasks (background) | N/A | Pilot (circuit breaker, dual-exit, reflector) |
| Code intelligence | N/A | N/A | GRAPHCTX (parser + graph + retrieval) on-demand |
| Self-improvement | N/A | N/A | HeuristicReflector + LearningStore (planned) |
| Environment legibility | progress.txt + git | .claude/ directory | Auto-init + .theo/ + git + clean state |
| Guides (feedforward) | agents.md, AGENTS.md | CLAUDE.md, rules/ | .theo/theo.md, skills, system prompt |
| Sensors (feedback) | Tests, CI | Tests, linters | Tests + doom loop + circuit breaker + governance |

O diferencial do Theo: **code intelligence nativo** (GRAPHCTX) e **self-improvement** (Reflector). Nenhum outro coding agent tem ambos integrados no harness.

## Referencias

1. Martin Fowler. "Harness Engineering." martinfowler.com, 2025.
2. OpenAI. "Harness Engineering." openai.com/index/harness-engineering, 2025.
3. Anthropic. "Effective Harnesses for Long-Running Agents." anthropic.com/engineering, 2025.
4. Michael Bolin (OpenAI). "Codex, Harness Engineering, and the Real Future of Coding Agents." Video interview, 2025.
5. Dex Horthy (HumanLayer). "No Vibes Allowed: Solving Hard Problems in Complex Codebases." Conference talk, 2025.
6. "WTF is Harness Engineer & Why is it Important." Video essay, 2025.
