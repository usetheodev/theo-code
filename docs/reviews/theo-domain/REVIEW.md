# theo-domain — Revisao

> **Contexto**: Crate de tipos puros (zero dependencias de workspace). Define traits, enums, newtypes, state machines e contratos compartilhados por todo o workspace.
>
> **Invariante**: `theo-domain` NUNCA depende de outros crates do workspace (ADR-010).
>
> **Status global**: deep-review concluido em 2026-04-25. Cargo.toml verificado: zero `path =` ou workspace deps internos — apenas `tokio`, `serde`, `serde_json`, `thiserror`, `async-trait`, `tempfile` (todos external). Invariante ADR-010 preservada. 452 tests passando, 0 falhas. `cargo clippy -p theo-domain --lib --tests` silent (zero warnings).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `agent_run` | State machine de `RunState` (Initialized, Planning, Executing, Evaluating, Converged, Replanning, Waiting, Aborted) e tipos do ciclo de vida de um run. | Revisado |
| 2 | `agent_spec` | Specs de agent (capabilities, limites, modelo, policies). | Revisado |
| 3 | `budget` | Tipos de orcamento (tokens, cost, latency) e contadores. | Revisado |
| 4 | `capability` | Capabilities que um agent/tool pode declarar ou exigir. | Revisado |
| 5 | `code_intel` | Tipos de code intelligence (symbols, ranges, snippets) consumidos por engine/retrieval. | Revisado |
| 6 | `episode` | Representacao de episodios (unidade de aprendizado/memoria). | Revisado |
| 7 | `error` | Hierarquia de erros do dominio, incluindo `TransitionError`. Baseado em `thiserror`. | Revisado |
| 8 | `error_class` | Classificacao tipada de erros (quota, network, auth, policy etc.) para fail-fast. | Revisado |
| 9 | `event` | Enumeracao canonica de eventos de runtime e seus payloads. | Revisado |
| 10 | `evolution` | Tipos para evolucao/auto-melhoria do agent (hypothesis, lesson, review). | Revisado |
| 11 | `graph_context` | Tipos de contexto de grafo de codigo (nodes, edges, neighbourhood). | Revisado |
| 12 | `identifiers` | Newtypes para IDs (`FileId`, `SymbolId`, `SessionId`, `RunId`). | Revisado |
| 13 | `memory` | Trait `MemoryProvider` + contratos de long-term memory (fencing XML, error isolation). | Revisado |
| 14 | `memory/decay` | Regras de decay/expiracao de memorias. | Revisado |
| 15 | `memory/lesson` | Tipos de licoes aprendidas persistidas em memoria. | Revisado |
| 16 | `memory/wiki_backend` | Contrato de backend para Code Wiki (pages, links, metadata). | Revisado |
| 17 | `permission` | Tipos de permissao e politicas de acesso. | Revisado |
| 18 | `priority` | Prioridades de tasks/tool-calls. | Revisado |
| 19 | `retry_policy` | Politicas de retry (backoff, jitter, max attempts). | Revisado |
| 20 | `routing` | Tipos para roteamento de LLM (cascade, auto, rules). | Revisado |
| 21 | `safe_json` | Wrapper seguro de JSON (size limits, depth limits, anti-injection). | Revisado |
| 22 | `sandbox` | Tipos de sandbox policy (bwrap/landlock/noop). | Revisado |
| 23 | `session` | Modelo de sessao (conversa, tree, bootstrap state). | Revisado |
| 24 | `session_search` | Tipos de busca em sessoes passadas. | Revisado |
| 25 | `session_summary` | Sumarios de sessoes (compaction output). | Revisado |
| 26 | `task` | State machine de `TaskState` (Pending, Ready, Running, WaitingTool, WaitingInput, Blocked, Completed, Failed, Cancelled). | Revisado |
| 27 | `tokens` | Contagem de tokens e janelas de contexto. | Revisado |
| 28 | `tool` | Trait `Tool`, `schema()`, `category()`, descoberta. | Revisado |
| 29 | `tool_call` | State machine de `ToolCallState` (Queued, Dispatched, Running, Succeeded, Failed, Timeout, Cancelled). | Revisado |
| 30 | `truncate` | Estrategias de truncamento de texto/contexto. | Revisado |
| 31 | `wiki_backend` | Contrato publico do Code Wiki (separado de `memory/wiki_backend`). | Revisado |
| 32 | `working_set` | Working set do agent (arquivos ativos, foco atual). | Revisado |
| 33 | `StateMachine` trait + `transition()` | Contrato generico de state machine com transicao atomica (preserva estado em erro). | Revisado |

## Modulos auxiliares (nao listados originalmente, presentes em `src/`)

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 34 | `clock` | Wrapper de relogio (`now_millis`) para mock-friendly testing. | Revisado |
| 35 | `environment` | Helpers para variaveis de ambiente saneadas. | Revisado |
| 36 | `prompt_sanitizer` | Strip de injection tokens + fencing XML para prompt safety (T1.2). | Revisado |
| 37 | `user_paths` | Lookup canonico de paths user-config (`~/.config/theo/...`) com fail-closed em HOME unset (T1.4). | Revisado |

---

## Notas de Deep-Review por Dominio

> Auditoria orientada a: (1) zero workspace-internal deps (ADR-010 invariante), (2) `derive(Debug, Clone, Serialize, Deserialize)` consistente, (3) state-machine atomicity (`#[test] transition_atomicity_state_preserved_on_error`), (4) zero `unwrap()` em paths de producao.

### 1. agent_run (170 LOC)
`RunState` enum + StateMachine impl + transition table. Tests `transition_table_exhaustive` verificam cada par (from, to) explicitamente. `transition_atomicity_state_preserved_on_error` prova que falha em transition NAO altera o estado original.

### 2. agent_spec (146 LOC)
`AgentSpec` struct para specs YAML/TOML de sub-agents. Inclui `on_demand` factory + `output_format` + `output_format_strict` + `mcp_servers` + `isolation` field (None/"shared"/"worktree").

### 3. budget (219 LOC)
`Budget`, `TokenUsage`, `BudgetUsage`. `accumulate(other)` + `recompute_cost(model_cost)`. Pure types.

### 4. capability (253 LOC)
`CapabilitySet`, `AllowedTools::Only{tools}`, `CapabilityDenied`. Filtering helpers para `tool_call_manager`. T1.3 plugin owner check usa esses tipos.

### 5. code_intel (121 LOC)
`SymbolKind`, `Range`, `CodeSnippet`, `LineRange`. Consumido por engine-graph e engine-retrieval.

### 6. episode (665 LOC) — maior modulo do crate
`EpisodeSummary` + `MachineSummary` + `from_events(run_id, task_id, objective, &events)`. Maior porque tem o lookup table de event→episode-fields. Cobertura ampla via `from_events_extracts_files_edited_from_payloads` etc.

### 7. error (30 LOC)
`TransitionError { from, to }` minimal. Driven por `thiserror`.

### 8. error_class (102 LOC)
`ErrorClass::{Solved, Aborted, Exhausted, RateLimited, AuthFailed, QuotaExceeded, ContextOverflow}`. Headless v3 schema separa infra failures de agent failures. Test invariante: `success == true ⇔ class == Some(Solved)`.

### 9. event (380 LOC)
`DomainEvent { event_id, event_type, entity_id, payload, timestamp }` + `EventType` enum (RunInitialized, LlmCallStart, ToolCallQueued, etc.). Used by event_bus, observability, characterization tests.

### 10. evolution (60 LOC)
`HypothesisRecord`, `LessonRecord` types. Pequeno, focado.

### 11. graph_context (286 LOC)
Trait `GraphContextProvider` + types (Node, Edge, Neighbourhood). Implementacao concreta em `theo-application::use_cases::graph_context_service`.

### 12. identifiers (77 LOC)
Newtypes: `FileId(u32)`, `SymbolId(u32)`, `SessionId(String)`, `RunId(String)`, `MessageId(String)`, `TaskId(String)`, `EventId(u64)`, `CallId(String)`. `RunId::generate()` deterministico-unique via clock nanos.

### 13. memory (mod.rs 128 LOC)
Trait `MemoryProvider` async + fencing XML + error-isolation contract (errors swallowed dentro do trait, nunca crasham o run). Sub-modules: decay, lesson, wiki_backend.

### 14. memory/decay (94 LOC)
`DecayPolicy` + `MemoryEntry::age_at` calculations. Pure functions.

### 15. memory/lesson (253 LOC)
`LessonRecord`, `LessonOutcome`, persistence helpers. Driven por `theo-agent-runtime::lesson_pipeline`.

### 16. memory/wiki_backend (70 LOC)
Contrato de backend wiki para memoria (separado do wiki_backend.rs publico). Apartheid intencional.

### 17. permission (79 LOC)
`PermissionMode`, `PermissionDecision`. Inputs to capability_gate.

### 18. priority (24 LOC)
`Priority { Low, Normal, High }` enum minimal. Used by task_manager.

### 19. retry_policy (104 LOC)
`RetryPolicy { max_retries, base_delay_ms, max_delay_ms, jitter }` + `delay_for_attempt(n)` exponential. Defaults: `default_llm()` (3 retries, 1s base, 30s cap, jitter on); `benchmark()` (no jitter, no delay).

### 20. routing (192 LOC)
`ModelRouter` trait + `RoutingContext`, `RoutingPhase::{Normal, Recovery}`, `ModelChoice`, `RoutingFailureHint`. Implementacoes em `theo-infra-llm::routing`.

### 21. safe_json (76 LOC)
`from_str_bounded(s, limit)` + `DEFAULT_JSON_LIMIT = 10MB`. Anti-DoS: limita tamanho ANTES de serde alocar. T2.7 invariante.

### 22. sandbox (445 LOC)
`ProcessPolicy` (rlimits + env allowlist), `FilesystemPolicy` (allowed_read/write + denied), `NetworkPolicy`, `SandboxConfig`. Constants: `ALWAYS_DENIED_READ` (~/.ssh, ~/.aws, etc.), `ALWAYS_DENIED_WRITE`, `SENSITIVE_FILE_PATTERNS`. Cobertura ampla.

### 23. session (21 LOC)
`SessionId` newtype trivial.

### 24. session_search (101 LOC)
Types para busca em sessoes passadas via `theo-engine-retrieval`.

### 25. session_summary (97 LOC)
Compaction output type. Driven por `theo-agent-runtime::compaction_summary`.

### 26. task (210 LOC)
`TaskState` enum + `Task` struct + `transition(target)` com `TransitionError`. `transition_table_exhaustive` test pina cada transicao. `transition_atomicity_state_preserved_on_error` test valida atomicidade.

### 27. tokens (27 LOC)
`TokenCount` newtype + helpers. Trivial.

### 28. tool (501 LOC) — maior fora de episode
Trait `Tool { name, description, schema, category, execute }` + `ToolDefinition`, `ToolContext`, `ToolParam`, `ToolCategory`, `BuildableContext`. Test contracts via concrete impls in `theo-tooling`.

### 29. tool_call (148 LOC)
`ToolCallState` + `ToolCall { id, function: FunctionCall { name, arguments } }` + transition table. Same atomicity invariants as agent_run/task.

### 30. truncate (156 LOC)
`char_boundary_truncate(s, max_bytes)`, `truncate_lines(s, max)`. UTF-8 safe (T1.2 cobertura: `char_boundary_truncate_never_slices_multibyte_scalars`).

### 31. wiki_backend (59 LOC)
Public contract para Code Wiki backend. `WikiBackend` trait. Separado de `memory/wiki_backend.rs` por bounded-context.

### 32. working_set (93 LOC)
`WorkingSet` com touch_file, record_event, record_edit_attempt. Used by run_engine/post_dispatch_updates.rs.

### 33. StateMachine + transition() (lib.rs:36-50)
`pub trait StateMachine: Copy + PartialEq + Debug { fn allowed_transitions(&self) -> Vec<Self>; }` + `pub fn transition<S: StateMachine>(state: &mut S, target: S) -> Result<(), TransitionError>`. Inline test `transition_error_contains_from_and_to`. Three concrete impls: agent_run::RunState, task::TaskState, tool_call::ToolCallState — todos com atomicity tests.

### 34. clock (auxiliar)
`now_millis()` wrapper. Mock-friendly via tokio time::pause em tests.

### 35. environment (auxiliar)
Helpers env var safety. Used by user_paths.

### 36. prompt_sanitizer (auxiliar)
T1.2 implementacao: `strip_injection_tokens`, `fence_untrusted`, `fence_untrusted_default`, `char_boundary_truncate`. ALWAYS_STRIPPED token list (~17 tokens: `<|im_start|>`, `<|endoftext|>`, `[INST]`, etc.). Cobertura via security_t7_1.

### 37. user_paths (auxiliar)
`home_dir()`, `theo_config_dir()`, `theo_config_subdir(name)`. T1.4: HOME unset → `None` (NUNCA `/tmp` fallback). Verificado por security_t7_1::home_unset_does_not_fallback_to_tmp.

---

## Conclusao

Todos os 33 dominios listados no REVIEW + 4 auxiliares (clock, environment, prompt_sanitizer, user_paths) revisitados e marcados **Revisado**.

**Invariantes verificados:**
- ADR-010 (zero workspace-internal deps): Cargo.toml inspecionado — apenas tokio/serde/serde_json/thiserror/async-trait/tempfile (external). ✓
- State machines (agent_run, task, tool_call) tem transition tables exaustivas + atomicity tests. ✓
- Anti-DoS: safe_json::DEFAULT_JSON_LIMIT = 10MB. ✓
- T1.2 sanitizer: ALWAYS_STRIPPED tokens + char_boundary_truncate UTF-8 safety. ✓
- T1.4 user_paths: HOME unset → None (zero `/tmp` fallback). ✓

**Hygiene:**
- 452 tests passando, 0 falhas
- `cargo clippy -p theo-domain --lib --tests` silent (zero warnings)
- Module sizes: maior e episode.rs (665 LOC), seguido de tool.rs (501) e sandbox.rs (445); todos focados em single responsibility, tamanho legitimo dado o scope.

Sem follow-ups bloqueadores. O bounded-context do dominio puro mantem o invariante ADR-010 e oferece os contratos compartilhados estaveis sobre os quais os outros 10 crates do workspace dependem.
