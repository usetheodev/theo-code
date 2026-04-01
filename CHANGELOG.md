# Changelog

## [Unreleased]

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
