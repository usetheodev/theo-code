# ADR-001: Refatoracao Estrutural por Bounded Contexts e Application Layer

**Status:** Aceito
**Data:** 2026-03-31
**Autor:** Paulo (Staff-level review)
**Escopo:** Workspace completo — crates, apps, docs, research

---

## Contexto

O Theo Code e um agente autonomo de codificacao com motor de contexto baseado em grafos (GRAPHCTX). O sistema tem boa ambicao tecnica, mas esta organizado por **capacidade tecnica** (graph, context, llm, tools) em vez de **nucleo operacional**. Isso cria risco crescente de acoplamento conforme features sao adicionadas.

### Problemas identificados

1. **Organizacao por capacidade, nao por nucleo operacional**
   - 10 crates + 2 binarios + UI sem fronteiras claras entre dominios
   - `core` e `graph` sao duas raizes independentes que so se encontram nos binarios finais
   - `provider` esta orfao (nenhum crate depende dele)

2. **`context` gordo demais**
   - 17 modulos: assembly, bandit, cascade, compress, contrastive, ensemble, escape, feedback, graph_attention, memory, neural, predictive, search, summary, tfidf, turboquant, budget
   - Mistura retrieval, ranking, compressao, embedding, algoritmos experimentais

3. **Desktop e CLI divergem silenciosamente**
   - Desktop: agent → tools → core (ignora graph/context/governance)
   - CLI: graph → context → governance → parser (ignora agent)
   - Nenhuma camada unifica os dois caminhos

4. **Docs futuros > codigo real**
   - Docs 03-11 descrevem arquitetura que nao existe no codigo
   - Risco de desalinhamento entre "o que o sistema diz ser" e "o que e"

5. **Apps acopladas ao miolo**
   - Tauri chama `AgentLoop::new().run()` diretamente
   - UI consome semantica crua do dominio (fases, eventos internos)
   - Qualquer nova surface (VS Code extension, API HTTP) vai duplicar cola

6. **Benchmark e research misturados com runtime**
   - `benchmark/` Python dentro do corpo do produto
   - `algo-output/` e `referencias/` competem visualmente com codigo de producao

---

## Decisao

Reorganizar o workspace em torno de **4 bounded contexts** com **application layer** explicita e **apps finas**.

### Bounded Contexts

| # | Bounded Context | Responsabilidade |
|---|---|---|
| 1 | **Code Intelligence Engine** | Parser, grafo, indexacao, retrieval, ranking, contexto |
| 2 | **Agent Runtime** | Loop, sessao, estados, fases, eventos, cancelamento |
| 3 | **Governance & Safety** | Impacto, politicas, validacao, gates, checkpoints, audit |
| 4 | **Product Surfaces** | CLI, desktop, UI, benchmark (apps finas) |

### Regras Arquiteturais

| # | Regra | Justificativa |
|---|---|---|
| R1 | Apps nunca importam engines diretamente — falam com `theo-application` | Previne duplicacao de cola entre surfaces |
| R2 | Agent runtime nao conhece UI nem Tauri — emite eventos por port | Desacopla runtime de apresentacao |
| R3 | Ferramentas recebem contexto explicito, sem estado global implicito | Testabilidade e previsibilidade |
| R4 | Governance e obrigatoria no caminho critico de edicao | Diferencial do produto, nao pos-processo |
| R5 | Retrieval e Graph sao engines independentes | Evita "crate Deus" |
| R6 | Benchmark e research isolados do runtime | Naturezas diferentes, entropias diferentes |

---

## Estrutura-Alvo

```text
theo-code/
├── apps/
│   ├── theo-cli/                    # Binario CLI (context, impact, stats)
│   ├── theo-desktop/                # Tauri v2 backend
│   ├── theo-ui/                     # React + TypeScript frontend
│   └── theo-benchmark/              # Harness Python + scripts
│
├── crates/
│   │
│   │  ── Camada 0: Dominio ──
│   ├── theo-domain/                 # Tipos puros, zero infra
│   │
│   │  ── Camada 1: Engines ──
│   ├── theo-engine-parser/          # Tree-sitter, symbol extraction
│   ├── theo-engine-graph/           # Code graph, persistencia, co-change
│   ├── theo-engine-retrieval/       # BM25, embeddings, rerank, assembly
│   │
│   │  ── Camada 2: Runtimes ──
│   ├── theo-agent-runtime/          # Loop, sessao, fases, eventos
│   ├── theo-tooling/                # Contratos + registry + executor
│   ├── theo-governance/             # Impacto, policy, validation, gates
│   │
│   │  ── Camada 3: Infraestrutura ──
│   ├── theo-infra-llm/             # Clientes OpenAI/Anthropic/compatible
│   ├── theo-infra-auth/            # OAuth PKCE, device flow, token store
│   ├── theo-infra-storage/         # Cache bincode, filesystem, persist
│   ├── theo-infra-observability/   # Tracing, metrics, logs estruturados
│   │
│   │  ── Camada 4: Aplicacao ──
│   ├── theo-application/           # Casos de uso / orquestracao
│   │
│   │  ── Camada 5: Contratos ──
│   └── theo-api-contracts/         # DTOs, eventos serializaveis para UI/CLI
│
├── docs/
│   ├── current/                     # Arquitetura implementada
│   ├── target/                      # Arquitetura planejada
│   ├── adr/                         # Architecture Decision Records
│   └── roadmap/                     # Plano de implementacao
│
├── research/
│   ├── references/                  # Papers, projetos de referencia
│   └── experiments/                 # algo-output, prototipos
│
├── fixtures/                        # Dados de teste compartilhados
├── tests/                           # Testes de integracao cross-crate
└── scripts/                         # Automacao, CI, release
```

---

## Mapeamento: Estado Atual → Estado Alvo

### Crates

| Atual | Alvo | Acao |
|---|---|---|
| `crates/core` | `crates/theo-domain` | Renomear. Remover deps de infra (tempfile vai para theo-tooling). Manter: tipos, traits, erros, session, permission. |
| `crates/parser` | `crates/theo-engine-parser` | Renomear. Sem mudanca de conteudo na fase 1. |
| `crates/graph` | `crates/theo-engine-graph` | Renomear. Mover `persist.rs` para `theo-infra-storage` (fase 2). |
| `crates/context` | `crates/theo-engine-retrieval` | **Quebrar.** Ver secao "Desmembramento do context" abaixo. |
| `crates/agent` | `crates/theo-agent-runtime` | Renomear. Extrair `tool_bridge.rs` para `theo-tooling`. Dependencia de `auth` vai para port. |
| `crates/tools` | `crates/theo-tooling` | Renomear. Separar internamente: contratos vs registry vs implementacoes. |
| `crates/llm` | `crates/theo-infra-llm` | Renomear. Absorver `crates/provider` (conversao de formatos). |
| `crates/provider` | (absorvido por `theo-infra-llm`) | **Eliminar como crate separado.** Ja esta orfao. Mover modulos para `theo-infra-llm/src/providers/`. |
| `crates/auth` | `crates/theo-infra-auth` | Renomear. Sem mudanca de conteudo. |
| `crates/governance` | `crates/theo-governance` | Renomear. Manter. Expandir conforme docs 03-06 forem implementados. |
| (novo) | `crates/theo-application` | **Criar.** Casos de uso que orquestram engines + runtime + governance. |
| (novo) | `crates/theo-api-contracts` | **Criar.** DTOs e eventos serializaveis para surfaces. |
| (novo) | `crates/theo-infra-storage` | **Criar.** Bincode persist, cache manager, filesystem ops. |
| (novo) | `crates/theo-infra-observability` | **Criar (fase 2+).** Tracing, metricas, logs estruturados. |

### Apps

| Atual | Alvo | Acao |
|---|---|---|
| `src/` (raiz) | `apps/theo-cli/` | Mover. Manter como binario fino que chama `theo-application`. |
| `src-tauri/` | `apps/theo-desktop/` | Mover. Substituir chamadas diretas a `AgentLoop` por `theo-application`. |
| `ui/` | `apps/theo-ui/` | Mover. Consumir apenas tipos de `theo-api-contracts`. |
| `benchmark/` | `apps/theo-benchmark/` | Mover. Isolar do runtime de producao. |

### Docs

| Atual | Alvo | Acao |
|---|---|---|
| `docs/01-vision-and-principles.md` | `docs/current/01-vision-and-principles.md` | Mover. |
| `docs/02-architecture.md` | `docs/current/02-architecture.md` | Mover. Atualizar para refletir nova estrutura. |
| `docs/03-decision-control-plane.md` | `docs/target/03-decision-control-plane.md` | Mover. E arquitetura planejada. |
| `docs/04-policy-engine.md` | `docs/target/04-policy-engine.md` | Mover. |
| `docs/05-validation-pipeline.md` | `docs/target/05-validation-pipeline.md` | Mover. |
| `docs/06-governance-layer.md` | `docs/target/06-governance-layer.md` | Mover. |
| `docs/07-agent-loop.md` | `docs/current/07-agent-loop.md` | Mover. Implementado. |
| `docs/08-llm-client.md` | `docs/current/08-llm-client.md` | Mover. Implementado. |
| `docs/09-promise-system.md` | `docs/target/09-promise-system.md` | Mover. |
| `docs/10-context-loop-and-decomposer.md` | `docs/target/10-context-loop-and-decomposer.md` | Mover. |
| `docs/11-checkpoint-and-resilience.md` | `docs/target/11-checkpoint-and-resilience.md` | Mover. |
| `docs/12-implementation-roadmap.md` | `docs/roadmap/12-implementation-roadmap.md` | Mover. |
| `docs/PROJECT_STRUCTURE.md` | `docs/current/PROJECT_STRUCTURE.md` | Mover. Atualizar. |

### Research

| Atual | Alvo | Acao |
|---|---|---|
| `referencias/` (raiz repo) | `research/references/` | Mover. |
| `algo-output/` | `research/experiments/algo-output/` | Mover. |
| `.theo-cache/` | Mantido onde esta (runtime cache) | Sem mudanca. |

---

## Desmembramento do `context` (17 modulos → 1 engine + modulos de infra)

O crate `context` e o mais critico para quebrar. Proposta:

### `theo-engine-retrieval` (novo, substitui `context`)

Mantém o nucleo de retrieval e assembly:

```text
theo-engine-retrieval/
├── src/
│   ├── lib.rs
│   ├── search.rs          # MultiSignalScorer (BM25 + 6 sinais)
│   ├── assembly.rs        # Montagem de contexto com budget
│   ├── budget.rs          # BudgetConfig, alocacao de tokens
│   ├── summary.rs         # CommunitySummary por cluster
│   ├── escape.rs          # ContextMembership, deteccao de miss
│   ├── graph_attention.rs # Propagacao de atencao no grafo
│   │
│   ├── scoring/           # Sub-modulo de sinais de ranking
│   │   ├── mod.rs
│   │   ├── bm25.rs        # (extraido de search.rs)
│   │   ├── pagerank.rs    # (extraido de search.rs)
│   │   └── recency.rs     # (extraido de search.rs)
│   │
│   ├── embedding/         # Sub-modulo de embeddings
│   │   ├── mod.rs
│   │   ├── neural.rs      # NeuralEmbedder (fastembed)
│   │   ├── tfidf.rs       # TF-IDF fallback
│   │   └── turboquant.rs  # Compressao vetorial
│   │
│   └── experimental/      # Algoritmos em pesquisa
│       ├── mod.rs
│       ├── bandit.rs       # Multi-armed bandit
│       ├── ensemble.rs     # Ensemble ranking
│       ├── contrastive.rs  # Contrastive learning
│       ├── predictive.rs   # Predictive scoring
│       ├── feedback.rs     # Feedback loop
│       ├── cascade.rs      # Cascade filtering
│       ├── compress.rs     # Context compression
│       └── memory.rs       # Memory/history
```

**Justificativa da organizacao interna:**
- `scoring/` — sinais isolados, testáveis independentemente
- `embedding/` — tudo relacionado a vetorizacao e compressao
- `experimental/` — algoritmos que ainda nao sao core path, podem ser feature-flagged

---

## `theo-application` — Casos de Uso

Este e o crate mais importante da refatoracao. Ele e a **unica porta de entrada** para apps.

```text
theo-application/
├── src/
│   ├── lib.rs
│   │
│   ├── use_cases/
│   │   ├── mod.rs
│   │   ├── build_project_graph.rs     # Constroi/atualiza grafo do projeto
│   │   ├── assemble_query_context.rs  # Monta contexto para uma query
│   │   ├── run_agent_session.rs       # Orquestra sessao completa do agente
│   │   ├── execute_tool_call.rs       # Executa tool com governance
│   │   ├── analyze_edit_impact.rs     # Analise de impacto pos-edicao
│   │   ├── validate_change.rs         # Validacao de mudanca com policies
│   │   ├── load_project_config.rs     # Carrega config do projeto
│   │   └── authenticate.rs            # Fluxos de autenticacao
│   │
│   └── ports/
│       ├── mod.rs
│       ├── graph_engine.rs            # trait GraphEngine
│       ├── retrieval_engine.rs        # trait RetrievalEngine
│       ├── llm_client.rs             # trait LlmClient
│       ├── tool_executor.rs          # trait ToolExecutor
│       ├── governance_gate.rs        # trait GovernanceGate
│       ├── auth_provider.rs          # trait AuthProvider
│       ├── event_emitter.rs          # trait EventEmitter
│       └── storage.rs                # trait StorageBackend
```

**Regra critica:** apps importam `theo-application` e `theo-api-contracts`. Nunca engines ou runtime diretamente.

---

## `theo-api-contracts` — DTOs para Surfaces

```text
theo-api-contracts/
├── src/
│   ├── lib.rs
│   ├── events.rs          # AgentEvent, FrontendEvent (serializaveis)
│   ├── requests.rs        # SendMessageRequest, ConfigUpdateRequest, etc.
│   ├── responses.rs       # SessionResult, ImpactReport (DTO), etc.
│   ├── config.rs          # AppConfig (DTO para UI)
│   └── auth.rs            # AuthStatus, LoginRequest (DTOs)
```

---

## Grafo de Dependencias Alvo

```text
Camada 0 — Dominio (zero deps externas de infra):
  theo-domain

Camada 1 — Engines (dependem de theo-domain):
  theo-engine-parser     → theo-domain
  theo-engine-graph      → theo-domain
  theo-engine-retrieval  → theo-domain, theo-engine-graph

Camada 2 — Runtimes (dependem de domain + ports):
  theo-agent-runtime     → theo-domain
  theo-tooling           → theo-domain
  theo-governance        → theo-domain, theo-engine-graph

Camada 3 — Infraestrutura (implementam ports):
  theo-infra-llm         → theo-domain
  theo-infra-auth        → theo-domain
  theo-infra-storage     → theo-domain
  theo-infra-observability → theo-domain

Camada 4 — Aplicacao (orquestra tudo via ports):
  theo-application       → theo-domain, theo-agent-runtime, theo-tooling,
                            theo-governance, theo-engine-retrieval,
                            theo-engine-graph, theo-engine-parser,
                            theo-infra-llm, theo-infra-auth,
                            theo-infra-storage

Camada 5 — Contratos (DTOs serializaveis):
  theo-api-contracts     → theo-domain

Camada 6 — Apps (consomem application + contracts):
  theo-cli               → theo-application, theo-api-contracts
  theo-desktop            → theo-application, theo-api-contracts
  theo-ui                → theo-api-contracts (TypeScript, sem Rust dep)
  theo-benchmark         → theo-application (Python, sem Rust dep)
```

**Invariantes do grafo:**
- Nenhuma seta sobe de camada (apps nunca importam engines)
- `theo-domain` nao depende de nada do workspace
- Engines nao dependem de runtimes
- Infra nao depende de engines (apenas implementa ports definidos em domain/application)
- `theo-api-contracts` depende apenas de `theo-domain`

---

## Plano de Execucao em Fases

### Fase 0: Preparacao (sem mudanca de codigo)

| # | Tarefa | Risco | Estimativa |
|---|---|---|---|
| 0.1 | Criar `docs/adr/` e mover este ADR para la | Nenhum | Trivial |
| 0.2 | Reorganizar `docs/` em `current/`, `target/`, `adr/`, `roadmap/` | Nenhum | Trivial |
| 0.3 | Mover `referencias/` para `research/references/` | Nenhum | Trivial |
| 0.4 | Mover `algo-output/` para `research/experiments/` | Nenhum | Trivial |
| 0.5 | Garantir que todos os testes passam no estado atual | Blocker se falhar | Medio |

**Criterio de avanso:** tudo compila, testes passam, zero mudanca de comportamento.

---

### Fase 1: Criar `theo-domain` (extrair de `core`)

| # | Tarefa | Detalhe |
|---|---|---|
| 1.1 | Criar `crates/theo-domain/Cargo.toml` | Deps minimas: serde, thiserror |
| 1.2 | Mover de `core`: `session.rs`, `error.rs`, `permission.rs` | Tipos puros, sem IO |
| 1.3 | Mover de `core`: trait `Tool` (sem `ToolContext` que tem path deps) | Contrato puro |
| 1.4 | Avaliar `truncate.rs` — possivelmente mover para `theo-tooling` | Usa filesystem, nao e dominio |
| 1.5 | `core` passa a re-exportar de `theo-domain` (backward compat temporario) | Evita quebrar tudo de uma vez |
| 1.6 | Todos os crates que dependem de `core` compilam sem mudanca | Validacao |

**Dependencia de `tempfile` no core:** confirmar se e usado apenas em testes. Se sim, mover para `[dev-dependencies]`. Se nao, mover o codigo que usa para `theo-tooling`.

**Criterio de avanso:** `cargo build --workspace` e `cargo test --workspace` passam.

---

### Fase 2: Renomear crates (mecanico)

| # | Atual | Novo nome |
|---|---|---|
| 2.1 | `crates/parser` | `crates/theo-engine-parser` |
| 2.2 | `crates/graph` | `crates/theo-engine-graph` |
| 2.3 | `crates/llm` | `crates/theo-infra-llm` |
| 2.4 | `crates/auth` | `crates/theo-infra-auth` |
| 2.5 | `crates/tools` | `crates/theo-tooling` |
| 2.6 | `crates/agent` | `crates/theo-agent-runtime` |
| 2.7 | `crates/governance` | `crates/theo-governance` |

**Procedimento por rename:**
1. Renomear diretorio
2. Atualizar `name` no `Cargo.toml` do crate
3. Atualizar `[workspace.members]` no `Cargo.toml` raiz
4. Atualizar `[dependencies]` em todos os crates que referenciam o nome antigo
5. Atualizar `use` statements no codigo (se o crate name mudou em `extern crate` ou paths)
6. `cargo build --workspace`

**Absorver `provider` em `theo-infra-llm`:**
1. Mover `provider/src/{common,anthropic,openai,openai_compatible,converter}.rs` para `theo-infra-llm/src/providers/`
2. Remover `crates/provider/` do workspace
3. Atualizar imports (nenhum crate depende de provider, entao impacto zero)

**Criterio de avanso:** workspace compila, testes passam, `cargo doc` gera sem erro.

---

### Fase 3: Quebrar `context` → `theo-engine-retrieval`

| # | Tarefa | Detalhe |
|---|---|---|
| 3.1 | Criar `crates/theo-engine-retrieval/` | Novo Cargo.toml, dep em `theo-domain` + `theo-engine-graph` |
| 3.2 | Mover modulos core: `search`, `assembly`, `budget`, `summary`, `escape`, `graph_attention` | Nucleo do retrieval |
| 3.3 | Criar sub-modulo `scoring/` com `bm25`, `pagerank`, `recency` extraidos de `search.rs` | Separacao de sinais |
| 3.4 | Criar sub-modulo `embedding/` com `neural`, `tfidf`, `turboquant` | Separacao de vetorizacao |
| 3.5 | Criar sub-modulo `experimental/` com `bandit`, `ensemble`, `contrastive`, `predictive`, `feedback`, `cascade`, `compress`, `memory` | Isolamento de algoritmos em pesquisa |
| 3.6 | Remover `crates/context/` do workspace | Substituido |
| 3.7 | Atualizar binario CLI (`src/pipeline.rs`) para usar `theo-engine-retrieval` | Unico consumidor direto |

**Criterio de avanso:** `cargo build --workspace`, testes do retrieval passam, CLI produz mesmos resultados.

---

### Fase 4: Mover apps para `apps/`

| # | Tarefa | Detalhe |
|---|---|---|
| 4.1 | Criar `apps/theo-cli/` e mover `src/main.rs`, `src/pipeline.rs`, `src/extract.rs` | Binario CLI |
| 4.2 | Mover `src-tauri/` para `apps/theo-desktop/` | Atualizar tauri.conf.json paths |
| 4.3 | Mover `ui/` para `apps/theo-ui/` | Atualizar vite.config.ts e tauri.conf.json |
| 4.4 | Mover `benchmark/` para `apps/theo-benchmark/` | Scripts Python isolados |
| 4.5 | Atualizar `[workspace.members]` no Cargo.toml raiz | Incluir novos paths |
| 4.6 | Atualizar `.gitignore` se necessario | Paths de build/cache |

**Risco:** Tauri tem paths relativos no `tauri.conf.json` (distDir, devUrl). Precisam ser ajustados.

**Criterio de avanso:** `cargo build --workspace`, `cargo tauri dev` funciona, `npm run dev` no UI funciona.

---

### Fase 5: Criar `theo-application` e `theo-api-contracts`

| # | Tarefa | Detalhe |
|---|---|---|
| 5.1 | Criar `crates/theo-api-contracts/` | DTOs, eventos serializaveis. Dep: `theo-domain`, serde. |
| 5.2 | Extrair `FrontendEvent` do Tauri para `theo-api-contracts` | Primeiro contrato |
| 5.3 | Criar `crates/theo-application/` | Dep em todos os crates de engine/runtime/infra |
| 5.4 | Criar use case `run_agent_session` | Extrair logica de `send_message` do Tauri |
| 5.5 | Criar use case `build_project_graph` | Extrair de `Pipeline::build_graph` do CLI |
| 5.6 | Criar use case `assemble_query_context` | Extrair de `Pipeline::assemble_context` |
| 5.7 | Criar use case `analyze_edit_impact` | Extrair de `Pipeline::impact_analysis` |
| 5.8 | Refatorar CLI para chamar `theo-application` em vez de engines direto | Aplicar R1 |
| 5.9 | Refatorar Desktop para chamar `theo-application` em vez de `AgentLoop` direto | Aplicar R1 |

**Esta e a fase mais importante e mais arriscada.** Cada use case deve ser criado e validado incrementalmente.

**Criterio de avanso:** apps chamam apenas `theo-application`, zero import direto de engines.

---

### Fase 6: Criar `theo-infra-storage`

| # | Tarefa | Detalhe |
|---|---|---|
| 6.1 | Criar `crates/theo-infra-storage/` | Cache manager, bincode persist, filesystem |
| 6.2 | Mover `graph/persist.rs` para storage | Serializacao de grafo |
| 6.3 | Mover cache logic de `Pipeline` para storage | `.theo-cache/` management |
| 6.4 | Mover `auth/store.rs` (token persistence) para storage | Unificar persistencia |

**Criterio de avanso:** toda persistencia passa por `theo-infra-storage`.

---

### Fase 7: Ports e DIP no agent-runtime

| # | Tarefa | Detalhe |
|---|---|---|
| 7.1 | Definir trait `LlmPort` em `theo-agent-runtime/ports/` | Contrato, nao implementacao |
| 7.2 | Definir trait `ToolExecutorPort` em `theo-agent-runtime/ports/` | Contrato |
| 7.3 | Definir trait `EventSinkPort` (ja existe como `EventSink`) | Formalizar |
| 7.4 | Definir trait `ContextProviderPort` | Para injecao de contexto GRAPHCTX |
| 7.5 | `AgentLoop` recebe ports via construtor, nao implementacoes concretas | DIP |
| 7.6 | Remover dependencia direta de `theo-infra-llm` e `theo-infra-auth` do agent | Inversao |

**Criterio de avanso:** `theo-agent-runtime` depende apenas de `theo-domain` e seus proprios ports.

---

## Riscos e Mitigacoes

| Risco | Impacto | Mitigacao |
|---|---|---|
| Rename em massa quebra imports | Alto | Fazer um crate por PR, validar compilacao a cada passo |
| Tauri paths quebram ao mover para `apps/` | Medio | Testar `cargo tauri dev` imediatamente apos mover |
| `theo-application` vira god crate | Alto | Use cases sao structs independentes, nao um service monolitico |
| Fase 5 (application layer) muda muita coisa de uma vez | Alto | Criar um use case por vez, manter backward compat temporario |
| Performance de build aumenta com mais crates | Baixo | Rust compila crates em paralelo, mais crates = mais paralelismo |
| Overhead de abstraccao em ports | Baixo | Ports sao traits com zero-cost dispatch (generics ou static dispatch) |

---

## O que NAO muda nesta refatoracao

- Algoritmos internos (GRAPHCTX, Leiden, BM25, embeddings)
- Logica de negocio do agent loop
- UI React (apenas path de import muda)
- Formato de dados (.theo-cache, auth.json)
- API do Tauri (mesmos comandos, mesmo canal de eventos)
- Suporte a linguagens do parser

**Esta refatoracao e puramente estrutural. Zero mudanca de comportamento.**

---

## Criterios de Sucesso

| Criterio | Como validar |
|---|---|
| Workspace compila | `cargo build --workspace` |
| Testes passam | `cargo test --workspace` |
| CLI produz mesmos resultados | Comparar output de `context`, `impact`, `stats` |
| Desktop funciona | `cargo tauri dev` + interacao manual |
| UI renderiza | `npm run dev` no theo-ui |
| Nenhuma regressao de performance | Benchmark GRAPHCTX com/sem contexto |
| Grafo de deps respeita camadas | Script que valida: apps → application → engines (nunca pula) |
| Docs refletem realidade | `current/` = codigo real, `target/` = planejado |

---

## Referencias

- Avaliacao Staff-level original (2026-03-31)
- [ADR format](https://adr.github.io/)
- Docs atuais: `docs/02-architecture.md`
- Memory: `architecture_refactor_decision.md`
