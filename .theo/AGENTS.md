# Theo-Code Agent Navigation Map

Quick-reference for AI agents working on this codebase. This is a **map**, not a manual.
For detailed architecture, see `theo-architecture.md` in the autoloop repo.

## Workspace (14 crates, 11 in eval scope)

| Crate | Package Name | Responsibility | Tests | How to Test |
|---|---|---|---|---|
| `crates/theo-domain` | `theo-domain` | Pure types, state machines, zero deps | 251 | `cargo test -p theo-domain` |
| `crates/theo-engine-parser` | `theo-engine-parser` | Tree-sitter extraction, 16 languages | 468 | `cargo test -p theo-engine-parser` |
| `crates/theo-agent-runtime` | `theo-agent-runtime` | Agent loop, pilot mode, convergence | 359 | `cargo test -p theo-agent-runtime` |
| `crates/theo-engine-retrieval` | `theo-engine-retrieval` | Search, ranking, context assembly | 274 | `cargo test -p theo-engine-retrieval` |
| `crates/theo-infra-llm` | `theo-infra-llm` | 25 LLM providers, streaming, retry | 156 | `cargo test -p theo-infra-llm` |
| `crates/theo-tooling` | `theo-tooling` | 40+ tools, registry, schemas | 144 | `cargo test -p theo-tooling` |
| `crates/theo-engine-graph` | `theo-engine-graph` | Code graph construction, clustering | 103 | `cargo test -p theo-engine-graph` |
| `crates/theo-infra-auth` | `theo-infra-auth` | OAuth PKCE, device flow, env vars | 87 | `cargo test -p theo-infra-auth` |
| `crates/theo-application` | `theo-application` | Use cases, pipeline, wiki backend | 70 | `cargo test -p theo-application` |
| `crates/theo-governance` | `theo-governance` | Sandbox, policy, impact analysis | 64 | `cargo test -p theo-governance` |
| `crates/theo-api-contracts` | `theo-api-contracts` | Serializable DTOs | 0 | `cargo test -p theo-api-contracts` |
| `apps/theo-cli` | `theo` | CLI binary | — | `cargo test -p theo` |
| `apps/theo-marklive` | `theo-marklive` | Markdown live renderer | 4 | `cargo test -p theo-marklive` |

## Do NOT Touch

- `apps/theo-desktop/` — Needs Tauri system deps, not in eval scope
- `apps/theo-benchmark/` — Benchmark isolation
- `.claude/CLAUDE.md` — Project instructions

## Dependency Order (leaf → root)

```
theo-domain (leaf, CAUTION: rebuilds everything)
  └─ theo-governance, theo-api-contracts
      └─ theo-engine-parser
          └─ theo-engine-graph
              └─ theo-engine-retrieval
                  └─ theo-tooling, theo-infra-llm, theo-infra-auth
                      └─ theo-agent-runtime
                          └─ theo-application
                              └─ theo-cli (root)
```

**Rule**: Work leaf-first to minimize rebuild cascading.

## Key Invariants

1. All state transitions are atomic (reject invalid transitions)
2. Terminal states reject ALL transitions
3. Tool must not modify messages array during execution
4. Sandbox policy checked before every tool execution
5. Every execution has unique RunId
6. Budget enforcer records iteration BEFORE budget check

## Quality Tests

- `crates/theo-governance/tests/boundary_test.rs` — 5 architectural boundary tests
- `crates/theo-governance/tests/structural_hygiene.rs` — Structural code quality tests

## Quick Commands

```bash
cargo test --workspace --exclude theo-code-desktop    # All tests
cargo clippy --workspace --exclude theo-code-desktop   # All clippy
cargo test -p theo-domain                              # Single crate
grep -r '.unwrap()' crates/*/src/ | wc -l              # Unwrap count
```
