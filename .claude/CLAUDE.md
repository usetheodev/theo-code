# Theo Code

AI coding assistant with deep code understanding. CLI + Desktop (Tauri v2).

## What is Theo Code?

Standalone AI coding assistant — same category as Claude Code, Codex, Cursor. Differentiators: **GRAPHCTX** (structural code intelligence) and **Code Wiki** (Obsidian-like knowledge base auto-generated from code).

**Theo** (the platform) is the Vercel for developers who have a backend. Theo Code helps users build applications on the Theo platform.

## Architecture

Rust workspace with 11 crates in 4 bounded contexts:

```
crates/
  theo-domain          # Pure types, traits, errors (ZERO dependencies)
  theo-engine-graph    # Code graph via Tree-Sitter (16 languages)
  theo-engine-parser   # AST parser, symbol extraction
  theo-engine-retrieval # Semantic search, RRF 3-ranker, embeddings
  theo-governance      # Policy engine (simplified)
  theo-agent-runtime   # Agent loop, state machine, streaming, sub-agents
  theo-infra-llm       # 25 LLM providers (OA-compatible internally)
  theo-infra-auth      # OAuth PKCE, device flow, token management
  theo-tooling         # 21 tools + sandbox (bwrap/landlock)
  theo-api-contracts   # DTOs and events
  theo-application     # Use cases layer

apps/
  theo-cli             # CLI binary (primary interface)
  theo-desktop         # Tauri v2 (Rust backend + React frontend)
  theo-ui              # React 18 + TypeScript + Tailwind + Radix UI
  theo-benchmark       # Benchmark runner (isolated)
```

## Build & Test

```bash
cargo build                          # Build workspace
cargo test                           # All tests
cargo test -p theo-engine-graph      # Specific crate
cd apps/theo-desktop && cargo tauri dev  # Desktop dev
cd apps/theo-ui && npm run dev       # Frontend dev
```

## Dependency Rules

```
theo-domain         → (nothing — pure types)
theo-engine-*       → theo-domain
theo-governance     → theo-domain
theo-infra-*        → theo-domain
theo-tooling        → theo-domain
theo-agent-runtime  → theo-domain, theo-governance
theo-api-contracts  → theo-domain
theo-application    → all crates above
apps/*              → theo-application, theo-api-contracts
```

## Conventions

- **Code language**: English (variables, functions, types, technical comments)
- **Communication**: Portugues Brasil
- **Rust edition**: 2024
- **Tests**: Required for business logic. Arrange-Act-Assert pattern.
- **Errors**: Typed with `thiserror`. Never swallow errors silently.
- **Workspace deps**: Declared in root `Cargo.toml` `[workspace.dependencies]`
- **Shared types**: Import from `theo-domain`
- **LLM protocol**: Everything OA-compatible internally. Providers convert at boundary.

## Key Invariants

- `theo-domain` NEVER depends on other crates
- Apps NEVER import engine/infra crates directly — go through `theo-application`
- Circular dependencies are forbidden
- Sandbox is mandatory for bash: bwrap > landlock > noop cascade
- Every tool declares `schema()` and `category()`
- Benchmark/research code stays isolated from production runtime

## Important Directories

- `docs/current/` — What IS implemented
- `docs/target/` — What is planned (future)
- `docs/adr/` — Architecture Decision Records
- `docs/roadmap/` — Product roadmap
- `research/` — Papers, experiments, references (isolated)
