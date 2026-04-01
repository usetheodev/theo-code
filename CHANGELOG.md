# Changelog

## [Unreleased]

### Added
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
