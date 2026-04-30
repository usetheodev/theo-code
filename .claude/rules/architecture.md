---
paths:
  - "crates/**/*.rs"
  - "apps/**/*.rs"
---

# Architectural Boundaries

## Research-Aligned Domains

The workspace is organized around the domains documented in `docs/pesquisas/*`.
Rules in this file should match those research artifacts and the current
workspace, not an older or idealized architecture.

1. **Code Intelligence / Context Engineering**
   - `theo-engine-parser` — Tree-Sitter extraction and language-aware parsing
   - `theo-engine-graph` — code graph construction, clustering, git/co-change signals
   - `theo-engine-retrieval` — search, ranking, context assembly, impact inputs
   - Research basis: `docs/pesquisas/context/INDEX.md`, `docs/pesquisas/languages/INDEX.md`

2. **Wiki / Human Knowledge Layer**
   - `theo-engine-wiki` — wiki storage/generation primitives
   - `theo-application` — wiki enrichment and backend integration
   - `theo-agent-runtime` — background wiki-agent triggers and orchestration
   - `theo-tooling` — wiki-facing tools
   - Research basis: `docs/pesquisas/wiki/INDEX.md`

3. **Agent Runtime / Subagents / Self-Evolution**
   - `theo-agent-runtime` — loop orchestration, subagents, compaction, checkpoints, observability
   - Research basis: `docs/pesquisas/agent-loop/INDEX.md`, `docs/pesquisas/subagents/INDEX.md`, `docs/pesquisas/self-evolution/INDEX.md`

4. **Governance / Isolation**
   - `theo-governance` — policy, permission, and risk decisions
   - `theo-isolation` — worktree/sandbox execution primitives
   - Research basis: `docs/pesquisas/security-governance/INDEX.md`

5. **Provider / Routing / External Infra**
   - `theo-infra-llm` — provider catalog, streaming, retry, routing support
   - `theo-infra-auth` — OAuth/device flow/API-key auth
   - `theo-infra-mcp` — MCP client/discovery/transport
   - `theo-infra-memory` — memory persistence/backends
   - Research basis: `docs/pesquisas/providers/INDEX.md`, `docs/pesquisas/model-routing/INDEX.md`, `docs/pesquisas/memory/INDEX.md`

6. **Application Boundary**
   - `theo-application` coordinates use-cases and is the dependency boundary for `apps/*`
   - `theo-api-contracts` carries serializable DTOs for app/IPC surfaces

7. **Surfaces**
   - `apps/theo-cli` — terminal/TUI surface
   - `apps/theo-desktop` — Tauri shell
   - `apps/theo-marklive` — markdown/wiki renderer
   - `apps/theo-ui` — React/Vite UI consumed by desktop/dashboard flows
   - Research basis: `docs/pesquisas/cli/INDEX.md`

## Dependency Direction (INVIOLABLE)

Each line below gives the **upper bound** of workspace crates a target may
depend on (ADR-010 — a crate may declare fewer deps than listed, but never
more). `check-arch-contract.sh` enforces the bound.

```
theo-domain              → (nothing)
theo-engine-graph        → theo-domain
theo-engine-parser       → theo-domain
theo-engine-retrieval    → theo-domain, theo-engine-graph, theo-engine-parser   (ADR-011)
theo-engine-wiki         → theo-domain, theo-engine-graph, theo-engine-parser
theo-governance          → theo-domain
theo-isolation           → theo-domain
theo-infra-llm           → theo-domain
theo-infra-auth          → theo-domain
theo-infra-mcp           → theo-domain
theo-infra-memory        → theo-domain, theo-engine-retrieval (optional, feature-gated)   (ADR-011)
theo-tooling             → theo-domain
theo-agent-runtime       → theo-domain, theo-governance,
                            theo-infra-llm, theo-infra-auth, theo-tooling,
                            theo-isolation, theo-infra-mcp   (ADR-016/021/022)
theo-api-contracts       → theo-domain
theo-application         → all crates above
apps/*                   → theo-application, theo-api-contracts, theo-domain
```

## Prohibitions

- Apps NEVER import engine/infra crates directly
- `theo-domain` NEVER depends on any other crate
- Circular dependencies are forbidden
- Research documents inform architecture, but production boundaries are enforced by code and gates
- Benchmark/research code never enters production runtime
- Do not introduce new cross-layer shortcuts; expose lower-layer functionality through `theo-application` when an app needs it
