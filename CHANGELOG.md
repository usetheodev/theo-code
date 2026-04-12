# Changelog

## [Unreleased]

### Added
- CLI Professionalization — complete plan execution (`docs/roadmap/cli-professionalization.md`):
  - **Fase 0**: `render/style` primitives, `tty/` detection + SIGWINCH listener, `config/` with `TheoConfig` serde + `TheoPaths` XDG (80 tests)
  - **Fase 1**: `render/` subsystem with `markdown`, `code_block` (syntect, 12+ langs), `streaming` (state machine with 6 proptests), `diff`, `table`, `progress`, `tool_result`, `banner`, `errors` (146 tests)
  - **Fase 2**: `commands/` registry with `SlashCommand` trait + dispatcher; new commands `/model`, `/cost`, `/doctor`; rewritten `/help`, `/status`, `/clear`, `/memory`, `/skills`; `input/` with `completer` (`/cmd` and `@file`), `hinter`, `highlighter`, `mention` (64KB cap, 10/turn), `multiline` (triple-backtick) (117 tests)
  - **Fase 3**: `permission/` with `PermissionSession` ACL and `dialoguer`-based `PermissionPrompt` (y/n/always/deny-always, `THEO_AUTO_ACCEPT=1` bypass); `status_line/format.rs`; `render/banner.rs` (39 tests)
  - **Fase 4**: `render/errors.rs` structured `CliError`/`CliWarning` with hint/docs fields; session path migrated to `TheoPaths::sessions()` (10 tests + XDG test)
  - 4 ADRs: ADR-001 Streaming Markdown State Machine, ADR-002 Reject Ratatui, ADR-003 XDG Paths, ADR-004 CLI Infra Exception
  - **Test count**: 23 → 375 (+352); source files 6 → 41; LOC 2378 → 8899
  - **Raw ANSI in production code outside `render/`**: 64 → 0
  - **Release binary size**: 72 MB → 78 MB (+6 MB, within +8 MB budget)
  - `docs/current/cli-baseline.md` with full execution log and post-plan metrics
- Workspace dependencies: `syntect 5`, `indicatif 0.17`, `console 0.15`, `dialoguer 0.11`, `textwrap 0.16`, `comfy-table 7`, `dirs 5`, `insta 1`, `proptest 1`, `async-trait` for theo-cli

### Changed
- `renderer.rs` migrated from 35+ raw ANSI escape sequences to `render/style` primitives; tool-result rendering delegated to pure functions in `render/tool_result`
- `repl.rs`, `commands.rs`, `pilot.rs`, `main.rs` migrated to `render::style` — total 64 raw ANSI sequences eliminated from `apps/theo-cli/src/` outside `render/`
- `CliRenderer::on_event` now buffers `ContentDelta` events through `StreamingMarkdownRenderer` for real-time formatted markdown output
- `rustyline` bumped 14 → 15
- `pulldown-cmark` 0.12 → 0.13, promoted to workspace dependency (shared between `theo-cli` and `theo-marklive`)
- Release binary size: 72 MB → 78 MB (+6 MB, within +8 MB budget)

- Agent Runtime formal com 3 state machines, 8 invariantes, 310 testes:
  - Fase 01: Core Types & State Machines — TaskState (9 estados), ToolCallState (7 estados), RunState (8 estados) com transições exaustivas sem wildcards, newtypes TaskId/CallId/RunId/EventId, contratos Task/ToolCallRecord/ToolResultRecord/AgentRun, trait StateMachine + transition() atômico
  - Fase 02: Event System — DomainEvent + EventType (11 variants), EventBus sync com in-memory log bounded (max 10k), EventListener trait, catch_unwind para listeners, PrintEventListener/NullEventListener. AgentEvent/EventSink marcados #[deprecated]
  - Fase 03: Task Lifecycle — TaskManager com create_task (Invariante 1), transition (Invariantes 4+5), queries by session/active. Thread-safe via Mutex
  - Fase 04: Tool Call Lifecycle — ToolCallManager com enqueue (Invariante 2: call_id único), dispatch_and_execute (Invariante 3: result referencia call_id), eventos ToolCallQueued/Dispatched/Completed. Mutex liberado durante tool execution async
  - Fase 05: Agent Run Lifecycle — AgentRunEngine com ciclo formal Initialized→Planning→Executing→Evaluating→Converged/Replanning/Aborted (Invariante 6: run_id único). Promise gate (git diff) preservado. Context loop preservado. AgentLoop::run como facade. Phase enum #[deprecated]
  - Fase 06: Failure Model — RetryPolicy com exponential backoff + jitter, RetryExecutor genérico async com is_retryable gate, DeadLetterQueue para falhas permanentes, CorrectionStrategy enum (RetryLocal/Replan/Subtask/AgentSwap)
  - Fase 07: Budget Enforcement — Budget (time/tokens/iterations/tool_calls), BudgetUsage com exceeds(), BudgetEnforcer com check() que publica BudgetExceeded event (Invariante 8: sem execução sem budget)
  - Fase 08: Scheduler & Concurrency — Priority enum (Low/Normal/High/Critical) com Ord, Scheduler com BinaryHeap + FIFO tiebreaker + tokio Semaphore para concurrency control, submit/run_next/cancel/drain
  - Fase 09: Capabilities & Security — CapabilitySet (allowed/denied tools, categories, paths, network), CapabilityGate com check_tool/check_path_write, denied_tools > allowed_categories precedência, read_only()/unrestricted() presets
  - Fase 10: Persistence & Resume — RunSnapshot com checksum de integridade (Invariante 7: resume de snapshot consistente), SnapshotStore trait async, FileSnapshotStore (JSON em ~/.theo/snapshots/), validação de checksum no load
  - Fase 11: Observability — RuntimeMetrics + MetricsCollector (RwLock thread-safe) com record_llm_call/tool_call/retry/run_complete, StructuredLogListener (JSON lines via EventListener), safe_div para 0/0=0.0
  - Fase 12: Integration & Convergence — ConvergenceCriterion trait, GitDiffConvergence, EditSuccessConvergence, ConvergenceEvaluator (AllOf/AnyOf), CorrectionEngine com select_strategy baseado em failure type + attempt count
- Roadmap executável do Agent Runtime em docs/roadmap/agent-runtime/ (13 documentos com DoDs)
- Tool Registry: cada tool declara schema/category, registry valida e gera LLM definitions automaticamente
- Sandbox de execução segura (ADR-002):
  - Bubblewrap (bwrap) como backend: PID ns, network isolation, capability drop, mount isolation, auto-cleanup
  - Landlock como fallback (filesystem isolation, Linux 5.13+)
  - Resource limits via setrlimit (CPU, memória, file size, nproc)
  - Env var sanitization (strip tokens AWS, GitHub, OpenAI, Anthropic)
  - Command validator léxico (rm -rf, fork bombs, interpreter escape)
  - Governance sandbox policy engine com risk assessment e sequence analyzer
- LLM Provider system (Strategy + Registry + Factory):
  - `LlmProvider` trait, `ProviderSpec` declarativo, `ProviderRegistry` com 25 providers
  - `AuthStrategy` (BearerToken, CustomHeader, NoAuth), `FormatConverter` (OaPassthrough, Anthropic, Codex)
  - Error taxonomy: AuthFailed, RateLimited, ProviderNotFound, Timeout, ServiceUnavailable
- GitHub Copilot OAuth end-to-end:
  - CopilotAuth com device flow RFC 8628 (GitHub.com + Enterprise)
  - Tauri commands para login/logout/status/apply/models
  - DeviceAuthDialog: Radix Dialog, clipboard auto-copy, countdown 15min, polling animation
  - Model selectbox dinamico — backend e fonte de verdade para modelos por provider
- PolicyLock para ambientes corporativos
- SandboxAuditTrail thread-safe
- ADR-002 e roadmaps executaveis com DoDs

### Changed
- tool_bridge usa tool.schema() em vez de schemas hardcoded (elimina DRY violation)
- theo-infra-llm: modulo provider/ com auth/, format/, catalog/
- theo-governance: sandbox_policy, sequence_analyzer, sandbox_audit
- SettingsPage: presets com badge, model select dinamico, API Key auto-disable para Copilot
- beforeDevCommand corrigido para workspace com opencode

### Fixed
- Divergencia de schema no tool_bridge: oldText→oldString, patch→patchText
- Copilot endpoint: api.githubcopilot.com/chat/completions (sem /v1/)
- AppLayout: nao sobrescreve config Copilot com OpenAI Codex no boot

### Changed
- Reorganizacao estrutural completa: crates renomeados por bounded context (ADR-001)
  - `core` → `theo-domain`
  - `graph` → `theo-engine-graph`
  - `parser` → `theo-engine-parser`
  - `context` → `theo-engine-retrieval` (com sub-modulos `embedding/` e `experimental/`)
  - `llm` → `theo-infra-llm` (absorveu `provider`)
  - `auth` → `theo-infra-auth`
  - `tools` → `theo-tooling`
  - `agent` → `theo-agent-runtime`
  - `governance` → `theo-governance`
- Apps movidos para `apps/`: `theo-cli`, `theo-desktop`, `theo-ui`, `theo-benchmark`
- Docs separados em `current/` (implementado), `target/` (planejado), `adr/`, `roadmap/`
- Research isolado em `research/references/` e `research/experiments/`

### Added
- `theo-api-contracts` — DTOs e eventos serializaveis para surfaces (FrontendEvent)
- `theo-application` — camada de casos de uso (run_agent_session)
- `docs/adr/001-structural-refactor-bounded-contexts.md`

### Removed
- `crates/provider` — modulos absorvidos por `theo-infra-llm/src/providers/`
- Dependencia fantasma de `theo-code-core` no desktop (declarada mas nao usada)

### Fixed
- Teste quebrado em `webfetch` (referenciava metodo removido `is_svg_content_type`)
- Teste quebrado em `codex` (esperava `max_output_tokens` que endpoint Codex nao suporta)
