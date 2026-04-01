# Theo Code — Estrutura do Projeto

## Visao Geral

**Theo Code** e um agente autonomo de codificacao com motor de contexto baseado em grafos de codigo (GRAPHCTX). O projeto combina:

- **Backend em Rust** — motor de grafos, parser multi-linguagem, busca semantica, loop de agente
- **Desktop em Tauri v2** — aplicacao desktop com UI React
- **Benchmarks em Python** — validacao contra SWE-bench Lite (50% pass rate com Qwen3-30B)

---

## Estrutura de Diretorios

```
theo-code/                          # Raiz do repositorio
├── referencias/                    # Material de referencia e inspiracao
│   ├── 2601.20245v1.pdf            # Paper: Context Graphs as Control Plane
│   ├── Cognition.pdf               # Paper de referencia
│   ├── controlplane/               # Codigo de referencia: control plane
│   └── opencode/                   # Codigo de referencia: projeto OpenCode
│
└── theo-code/                      # Codigo-fonte principal
    ├── Cargo.toml                  # Workspace Rust (edition 2024)
    ├── src/                        # Binario CLI — motor GRAPHCTX
    ├── src-tauri/                  # Binario Desktop — app Tauri v2
    ├── crates/                     # Crates Rust (10 modulos)
    ├── ui/                         # Frontend React + TypeScript
    ├── benchmark/                  # Suite de benchmarks Python
    ├── docs/                       # Documentacao tecnica (12 docs)
    ├── tests/                      # Testes e fixtures
    ├── algo-output/                # Saida de pesquisa algoritmica
    └── .theo-cache/                # Cache do grafo (graph.bin, etc.)
```

---

## Crates Rust

O workspace contem 10 crates + o binario raiz. A arquitetura segue camadas com dependencias unidirecionais.

```
                    ┌─────────────┐
                    │   agent     │  ← Loop principal do agente
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
        ┌─────▼────┐ ┌────▼─────┐ ┌────▼─────┐
        │   llm    │ │  tools   │ │ context  │
        └─────┬────┘ └────┬─────┘ └────┬─────┘
              │            │            │
        ┌─────▼────┐      │      ┌─────▼─────┐
        │ provider │      │      │   graph   │
        └──────────┘      │      └─────┬─────┘
                          │            │
                    ┌─────▼────┐ ┌─────▼─────┐
                    │   core   │ │  parser   │
                    └──────────┘ └───────────┘

        ┌──────────┐ ┌────────────┐
        │   auth   │ │ governance │  ← Modulos independentes
        └──────────┘ └────────────┘
```

### `core` — Tipos Fundamentais
Tipos compartilhados por todo o projeto:
- **`Tool` trait** — interface async para ferramentas (`id()`, `description()`, `execute()`)
- **`PermissionType`** — enum de permissoes (Read, Edit, Bash, Glob, Grep, WebFetch, etc.)
- **`PermissionRule`** — avaliacao baseada em glob patterns
- **`SessionId`/`MessageId`** — wrappers tipados para IDs
- **`ToolError`** — erro padronizado

### `parser` — Extracao Multi-Linguagem
Parser baseado em tree-sitter com suporte a **14 linguagens**: Rust, Python, TypeScript, JavaScript, C, C++, C#, Go, Java, Kotlin, PHP, Ruby, Scala, Swift.

Extrai:
- Simbolos (funcoes, classes, structs, traits, enums)
- Referencias cruzadas entre arquivos
- Tabelas de simbolos
- Resolucao de imports

### `graph` — Grafo de Propriedades do Codigo (MCPH)
**Multi-relational Code Property Hypergraph** — o modelo de dados central.

**Tipos de nos:** File, Symbol, Import, Type, Test

**Tipos de arestas:** Contains, Calls, Imports, Inherits, TypeDepends, Tests, CoChanges, References

Modulos:
- `model.rs` — `CodeGraph` com adjacencia forward/reverse e indice de filhos
- `parse.rs` — conversao tree-sitter → nos/arestas
- `bridge.rs` — DTOs (`FileData`, `SymbolData`) e `build_graph()`
- `cluster.rs` — deteccao de comunidades (Louvain, Leiden, FileLeiden)
- `git.rs` — arestas de co-change extraidas do git log com decaimento temporal
- `persist.rs` — serializacao bincode para disco

### `context` — Motor GRAPHCTX
**Inovacao central do projeto.** Busca e montagem de contexto com 6 sinais:

| Sinal | Peso | Descricao |
|---|---|---|
| BM25 | 25% | Relevancia textual classica |
| Neural/Semantico | 20% | Embeddings 384-dim (fastembed) |
| File Symbol Boost | 20% | Boost por simbolos no arquivo |
| Graph Attention | 15% | Propagacao de atencao no grafo (2 hops, damping 0.5) |
| PageRank | 10% | Centralidade do no no grafo |
| Recency | 10% | Atividade recente (git) |

Modulos adicionais:
- `assembly.rs` — montagem de contexto com budget de tokens (greedy knapsack)
- `summary.rs` — sumarios legíveis por comunidade
- `turboquant.rs` — compressao de vetores (32x)
- `tfidf.rs` — fallback TF-IDF (128-dim)
- `neural.rs` — wrapper fastembed
- `escape.rs` — deteccao de arquivos faltantes no contexto
- `bandit.rs`, `ensemble.rs`, `contrastive.rs` — algoritmos auxiliares de ranking

### `llm` — Cliente LLM
Cliente OpenAI-compatible com streaming SSE.

- Suporte a qualquer API OpenAI-compatible
- Endpoint especial para Codex (`chatgpt.com/backend-api/codex/responses`)
- Parser XML Hermes para modelos que usam tool-calling via XML
- Streaming via `SseStream` + `StreamDelta`

### `provider` — Conversao de Protocolos
Camada de conversao entre formatos de diferentes provedores LLM:
- `CommonMessage` — formato intermediario unificado
- Conversores: Anthropic ↔ Common, OpenAI ↔ Common
- Suporte generico OpenAI-compatible

### `agent` — Loop do Agente
O loop principal que orquestra LLM ↔ ferramentas:

- **Fases:** Explore (1/3 inicial das iteracoes) → Edit (2/3 restantes)
- **Max iteracoes:** 15 (configuravel)
- **Context loop:** injeta contexto GRAPHCTX a cada N iteracoes
- **Done gate:** `done()` so e aceito se `git diff` mostra mudancas reais
- **Retry:** 1 retry automatico em erro de LLM
- **Eventos:** Token, ToolStart, ToolEnd, PhaseChange, Done, Error

### `tools` — Ferramentas do Agente
Implementacoes concretas do `Tool` trait:

**Registry padrao:** `bash`, `read`, `write`, `edit`, `grep`, `glob`, `apply_patch`, `webfetch`

**Ferramentas adicionais:** `codesearch`, `ls`, `lsp`, `multiedit`, `plan`, `question`, `skill`, `task`, `todo`, `websearch`, `batch`

### `governance` — Governanca Pos-Edicao
Analise de impacto apos edicoes:
- **BFS** a partir de simbolos editados (3 hops via Calls/Imports/Inherits/TypeDepends)
- **`ImpactReport`**: comunidades afetadas, cobertura de testes, candidatos a co-change
- **Alertas de risco:** modificacoes sem teste, impacto cross-cluster, co-change warnings

### `auth` — Autenticacao OpenAI OAuth2
Fluxo completo de autenticacao:
- OAuth2 PKCE via browser (porta 1455)
- Device authorization flow (uso headless)
- Armazenamento de tokens em `~/.config/theo-code/auth.json`
- Refresh automatico de tokens

---

## Binarios

### CLI (`src/`)
Motor GRAPHCTX como ferramenta de linha de comando:

```bash
# Monta contexto para uma query
theo-code context <repo-path> <query>

# Analisa impacto de um arquivo editado
theo-code impact <repo-path> <file>

# Exibe estatisticas do grafo
theo-code stats <repo-path>
```

O `Pipeline` (`src/pipeline.rs`) e o orquestrador principal:
- `build_graph()` / `build_from_directory()` — construcao do grafo
- `update_file()` — atualizacao incremental (re-clusteriza se >10% de arestas mudaram)
- `cluster()` — Leiden + sumarios + cache do scorer
- `assemble_context()` — montagem com budget de tokens
- `impact_analysis()` — analise de impacto pos-edicao
- Cache em `<repo>/.theo-cache/` (`graph.bin`, `clusters.bin`, `summaries.bin`)

### Desktop (`src-tauri/`)
Aplicacao Tauri v2 com:

**Comandos expostos ao frontend:**
- `send_message` — inicia o agent loop em background
- `cancel_agent` — cancela execucao do agente
- `set_project_dir` / `get_project_dir` — diretorio ativo do projeto
- `update_config` / `get_config` — configuracao do agente
- `auth_login_browser` — login OAuth via browser
- `auth_start_device_flow` / `auth_poll_device_flow` — auth para headless
- `auth_status` / `auth_logout` / `auth_apply_to_config` — gestao de tokens

**Eventos:** `TauriEventSink` emite `AgentEvent` → `FrontendEvent` no canal `"agent-event"`

---

## Frontend (`ui/`)

React + TypeScript, Vite, Tailwind CSS, shadcn/ui, framer-motion.

### Rotas

| Rota | Pagina | Status |
|---|---|---|
| `/assistant` | Chat com o agente (5 tabs) | Implementado |
| `/logs` | Visualizacao de logs | Implementado |
| `/code` | Visualizacao de codigo | Implementado |
| `/settings` | Configuracao do agente | Implementado |
| `/deploys` | Deploy monitoring | Placeholder |
| `/monitoring` | Observabilidade | Placeholder |
| `/database` | Banco de dados | Placeholder |

### Componentes Principais

- **`AppLayout`** — shell da aplicacao, carrega config/auth/project dir
- **`AssistantPage`** — pagina principal com 5 tabs (Agent, Plan, Tests, Review, Security)
- **`useAgentEvents`** — hook que escuta eventos Tauri e gerencia estado do chat
- **`SettingsPage`** — presets de providers, config de API, selecao de projeto

### Modos do Agente
- **Edit** — modo padrao, agente faz edicoes
- **Plan** — prepend `[MODE: PLAN...]`, agente planeja sem editar
- **Review** — prepend `[MODE: REVIEW...]`, agente revisa codigo

---

## Benchmarks (`benchmark/`)

Suite de validacao em Python:

| Arquivo | Funcao |
|---|---|
| `swe_bench_harness.py` | Avaliacao SWE-bench Lite (300 tasks, 600s/task) |
| `theo_agent.py` | Prototipo Python do agent loop (50% SWE-bench) |
| `run_benchmark.py` | Benchmark GRAPHCTX: com vs sem contexto |
| `mentor_validation.py` | Validacao de metricas do mentor |
| `decompose.py` | Engine de decomposicao de tasks |
| `results.json` | Resultados comparativos |
| `VALIDATION_LOG.md` | 4/7 bugs reais corrigidos (Express, Marshmallow, Requests) |

---

## Documentacao (`docs/`)

12 documentos tecnicos (portugues):

| # | Documento | Conteudo |
|---|---|---|
| 00 | Index | Indice geral |
| 01 | Vision & Principles | Contexto, problema, resultado esperado |
| 02 | Architecture | Estrutura de crates, fluxo de dados, padroes |
| 03 | Decision Control Plane | Lifecycle: PROPOSED→APPROVED→ACTIVE→COMPLETED |
| 04 | Policy Engine | Policy trait, mini-DSL, policies built-in |
| 05 | Validation Pipeline | Deterministica, <50ms, fail-fast |
| 06 | Governance Layer | GovernanceLayer, AgentIdentity, AuditLog |
| 07 | Agent Loop | Loop async, fases, transicoes |
| 08 | LLM Client | LlmClient, Hermes XML, MessageHistory |
| 09 | Promise System | Promise trait, PromiseGate, GitDiffPromise |
| 10 | Context Loop & Decomposer | ContextLoopEngine, diagnosticos |
| 11 | Checkpoint & Resilience | Undo stack, snapshots, circuit breaker |
| 12 | Implementation Roadmap | Fases, estrategia de testes |

> **Nota:** Docs 03-11 descrevem a arquitetura futura planejada (governance completo, policy engine, promise gates, checkpoints). O codigo atual implementa o agent loop e o motor de contexto.

---

## Fluxo de Dados Principal

```
           Query do usuario
                 │
                 ▼
          ┌─────────────┐
          │  Agent Loop  │ ← max 15 iteracoes
          └──────┬───────┘
                 │
    ┌────────────┼────────────┐
    │            │            │
    ▼            ▼            ▼
 Context      Tools         LLM
 Assembly    Execution     Client
    │            │            │
    ▼            │            │
 GRAPHCTX       │            │
 Pipeline       │            │
    │            │            │
    ▼            │            │
 CodeGraph      │            │
 (tree-sitter   │            │
  + Leiden      │            │
  + 6 sinais)   │            │
    │            │            │
    └────────────┼────────────┘
                 │
                 ▼
          Resultado final
       (com git diff gate)
```

---

## Stack Tecnologico

| Camada | Tecnologia |
|---|---|
| Linguagem backend | Rust (edition 2024) |
| Linguagem frontend | TypeScript + React |
| Desktop framework | Tauri v2 |
| Bundler | Vite |
| Estilos | Tailwind CSS + shadcn/ui |
| Animacoes | framer-motion |
| Parser de codigo | tree-sitter (14 linguagens) |
| Embeddings | fastembed (all-MiniLM-L6-v2, 384-dim) |
| Serializacao | bincode + serde |
| Paralelismo | rayon + tokio |
| Streaming | SSE (Server-Sent Events) |
| Autenticacao | OAuth2 PKCE + device flow |
| Benchmarks | Python (SWE-bench Lite) |
