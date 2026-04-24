# theo-domain — Revisao

> **Contexto**: Crate de tipos puros (zero dependencias de workspace). Define traits, enums, newtypes, state machines e contratos compartilhados por todo o workspace.
>
> **Invariante**: `theo-domain` NUNCA depende de outros crates do workspace (ADR-010).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `agent_run` | State machine de `RunState` (Initialized, Planning, Executing, Evaluating, Converged, Replanning, Waiting, Aborted) e tipos do ciclo de vida de um run. | Pendente |
| 2 | `agent_spec` | Specs de agent (capabilities, limites, modelo, policies). | Pendente |
| 3 | `budget` | Tipos de orcamento (tokens, cost, latency) e contadores. | Pendente |
| 4 | `capability` | Capabilities que um agent/tool pode declarar ou exigir. | Pendente |
| 5 | `code_intel` | Tipos de code intelligence (symbols, ranges, snippets) consumidos por engine/retrieval. | Pendente |
| 6 | `episode` | Representacao de episodios (unidade de aprendizado/memoria). | Pendente |
| 7 | `error` | Hierarquia de erros do dominio, incluindo `TransitionError`. Baseado em `thiserror`. | Pendente |
| 8 | `error_class` | Classificacao tipada de erros (quota, network, auth, policy etc.) para fail-fast. | Pendente |
| 9 | `event` | Enumeracao canonica de eventos de runtime e seus payloads. | Pendente |
| 10 | `evolution` | Tipos para evolucao/auto-melhoria do agent (hypothesis, lesson, review). | Pendente |
| 11 | `graph_context` | Tipos de contexto de grafo de codigo (nodes, edges, neighbourhood). | Pendente |
| 12 | `identifiers` | Newtypes para IDs (`FileId`, `SymbolId`, `SessionId`, `RunId`). | Pendente |
| 13 | `memory` | Trait `MemoryProvider` + contratos de long-term memory (fencing XML, error isolation). | Pendente |
| 14 | `memory/decay` | Regras de decay/expiracao de memorias. | Pendente |
| 15 | `memory/lesson` | Tipos de licoes aprendidas persistidas em memoria. | Pendente |
| 16 | `memory/wiki_backend` | Contrato de backend para Code Wiki (pages, links, metadata). | Pendente |
| 17 | `permission` | Tipos de permissao e politicas de acesso. | Pendente |
| 18 | `priority` | Prioridades de tasks/tool-calls. | Pendente |
| 19 | `retry_policy` | Politicas de retry (backoff, jitter, max attempts). | Pendente |
| 20 | `routing` | Tipos para roteamento de LLM (cascade, auto, rules). | Pendente |
| 21 | `safe_json` | Wrapper seguro de JSON (size limits, depth limits, anti-injection). | Pendente |
| 22 | `sandbox` | Tipos de sandbox policy (bwrap/landlock/noop). | Pendente |
| 23 | `session` | Modelo de sessao (conversa, tree, bootstrap state). | Pendente |
| 24 | `session_search` | Tipos de busca em sessoes passadas. | Pendente |
| 25 | `session_summary` | Sumarios de sessoes (compaction output). | Pendente |
| 26 | `task` | State machine de `TaskState` (Pending, Ready, Running, WaitingTool, WaitingInput, Blocked, Completed, Failed, Cancelled). | Pendente |
| 27 | `tokens` | Contagem de tokens e janelas de contexto. | Pendente |
| 28 | `tool` | Trait `Tool`, `schema()`, `category()`, descoberta. | Pendente |
| 29 | `tool_call` | State machine de `ToolCallState` (Queued, Dispatched, Running, Succeeded, Failed, Timeout, Cancelled). | Pendente |
| 30 | `truncate` | Estrategias de truncamento de texto/contexto. | Pendente |
| 31 | `wiki_backend` | Contrato publico do Code Wiki (separado de `memory/wiki_backend`). | Pendente |
| 32 | `working_set` | Working set do agent (arquivos ativos, foco atual). | Pendente |
| 33 | `StateMachine` trait + `transition()` | Contrato generico de state machine com transicao atomica (preserva estado em erro). | Pendente |
