# Theo-Code Agent Navigation Map

> Mirror condensed do `/CLAUDE.md` na raiz do repo. Em conflito numérico,
> a fonte canônica é `/CLAUDE.md`.

## Workspace (15 lib crates + 3 binary apps, Rust 2024)

| Crate / App | Package | Responsibility | Test Command |
|---|---|---|---|
| `crates/theo-domain` | `theo-domain` | Pure types, state machines, zero deps | `cargo test -p theo-domain` |
| `crates/theo-engine-graph` | `theo-engine-graph` | Code graph construction, clustering | `cargo test -p theo-engine-graph` |
| `crates/theo-engine-parser` | `theo-engine-parser` | Tree-Sitter extraction (14 languages) | `cargo test -p theo-engine-parser` |
| `crates/theo-engine-retrieval` | `theo-engine-retrieval` | BM25 + RRF + context assembly | `cargo test -p theo-engine-retrieval` |
| `crates/theo-governance` | `theo-governance` | Policy engine, sandbox cascade | `cargo test -p theo-governance` |
| `crates/theo-isolation` | `theo-isolation` | bwrap / landlock / noop fallback | `cargo test -p theo-isolation` |
| `crates/theo-infra-llm` | `theo-infra-llm` | 26 provider specs, streaming, retry | `cargo test -p theo-infra-llm` |
| `crates/theo-infra-auth` | `theo-infra-auth` | OAuth PKCE, device flow, env keys | `cargo test -p theo-infra-auth` |
| `crates/theo-infra-mcp` | `theo-infra-mcp` | Model Context Protocol client | `cargo test -p theo-infra-mcp` |
| `crates/theo-infra-memory` | `theo-infra-memory` | (in progress; ADR-008 pending) | `cargo test -p theo-infra-memory` |
| `crates/theo-test-memory-fixtures` | `theo-test-memory-fixtures` | Memory test fixtures | `cargo test -p theo-test-memory-fixtures` |
| `crates/theo-tooling` | `theo-tooling` | 72 production tools + registry | `cargo test -p theo-tooling` |
| `crates/theo-agent-runtime` | `theo-agent-runtime` | Agent loop, sub-agents, observability | `cargo test -p theo-agent-runtime` |
| `crates/theo-api-contracts` | `theo-api-contracts` | Serializable DTOs for IPC | `cargo test -p theo-api-contracts` |
| `crates/theo-application` | `theo-application` | Use-cases, facade, CLI runtime re-exports | `cargo test -p theo-application` |
| `apps/theo-cli` | `theo` | CLI binary (binary name: `theo`) | `cargo test -p theo` |
| `apps/theo-marklive` | `theo-marklive` | Markdown live renderer | `cargo test -p theo-marklive` |
| `apps/theo-desktop` | `theo-code-desktop` | Tauri shell — **excluded from CI tests** (GTK deps) | n/a |

**Outside Cargo workspace:**
- `crates/theo-compat-harness` — separate Cargo project (CLEAN-F1 tracks decision)
- `apps/theo-benchmark` — Python benchmark harness
- `apps/theo-ui` — Vite/TypeScript UI

## Do NOT Touch

- `apps/theo-desktop/` — Excluded from `cargo test --workspace`. Use `--exclude theo-code-desktop`.
- `apps/theo-benchmark/` — Python, runs against the built `theo` binary.
- `apps/theo-ui/` — TypeScript/Vite, separate `npm test`.
- `referencias/` — Vendored open-source repos, gitignored.
- `.theo/audit/`, `.theo/coverage/` — Generated artifacts.

## Dependency Order (leaf → root, ADR-010)

```
theo-domain (leaf, CAUTION: rebuilds everything)
  ├─ theo-governance, theo-api-contracts
  ├─ theo-engine-parser, theo-engine-graph
  │    └─ theo-engine-retrieval (ADR-011)
  ├─ theo-tooling, theo-infra-llm, theo-infra-auth, theo-infra-mcp, theo-infra-memory
  └─ theo-agent-runtime (consumes governance + infra-llm + infra-auth + tooling)
       └─ theo-application
            └─ apps/* (theo-cli, theo-marklive, theo-desktop)
```

**Rule:** Apps NEVER import engine/infra crates directly. Use `theo_application::cli_runtime::*` re-exports.

**Rule:** Work leaf-first to minimize rebuild cascading.

## Key Invariants

1. All state transitions are atomic (reject invalid transitions)
2. Terminal states reject ALL transitions
3. Tool must not modify messages array during execution
4. Sandbox policy checked before every tool execution
5. Every execution has unique RunId
6. Budget enforcer records iteration BEFORE budget check
7. ADR-010 dependency direction enforced by `make check-arch` (0 violations today)

## Quality gates (verified 2026-04-28)

| Gate | Status | Command |
|---|---|---|
| `cargo build --workspace --exclude theo-code-desktop` | ✅ exit 0 | `cargo build ...` |
| `cargo test ... --no-fail-fast` | 5247 PASS / 0 FAIL / 24 IGNORED (after CLEAN-A4) | `cargo test --workspace --exclude theo-code-desktop --lib --tests --no-fail-fast` |
| `make check-arch` | ✅ 0 violations (16 crates) | `bash scripts/check-arch-contract.sh` |
| `make check-sizes` | ✅ 52 oversize, all allowlisted, 0 NEW | `bash scripts/check-sizes.sh` |
| `make check-sota-dod --quick` | ✅ 12/12 PASS, 2 SKIP (paid LLM) | `bash scripts/check-sota-dod.sh --quick` |
| `make check-unwrap` (strict) | ❌ tracked in CLEAN-B1 | `bash scripts/check-unwrap.sh` |
| `make check-unsafe` (strict) | ❌ tracked in CLEAN-B2 | `bash scripts/check-unsafe.sh` |

## Quick Commands

```bash
# Build & test (always exclude desktop)
cargo build --workspace --exclude theo-code-desktop
cargo test --workspace --exclude theo-code-desktop --lib --tests --no-fail-fast

# Per-crate (faster)
cargo test -p theo-domain                   # leaf — fastest
cargo test -p theo-tooling                  # ~900 tests
cargo test -p theo-agent-runtime            # ~1300 tests

# Audit (zero new debt allowed)
make check-arch
make check-sizes
make check-sota-dod-quick
```

## CLI surface (`theo` — 17 subcommands)

```
init, agent, pilot, context, impact, stats, memory,
login, logout, dashboard, subagent, checkpoints,
agents, mcp, skill, trajectory, help
```

Run `cargo run -p theo --bin theo -- --help` for the full list.

## Where to look

| You want to | Look in |
|---|---|
| Canonical agent guide | `/CLAUDE.md` (this file is a mirror) |
| Project rules (inquebráveis) | `.claude/rules/` |
| ADRs D1-D16 (SOTA plan, conceptual) | `docs/plans/sota-tier1-tier2-plan.md` |
| ADRs ADR-NNN (physical files, not all written yet) | `docs/adr/` (pending) |
| Plans / phases | `docs/plans/` |
| Audits | `docs/audit/` |
| Cleanup tasks (current) | `docs/plans/cleanup-2026-04-28.md` |
| Maturity gap analysis | `docs/audit/maturity-gap-analysis-2026-04-27.md` |
| Reference repos (gitignored) | `referencias/` |
