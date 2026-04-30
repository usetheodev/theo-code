# Theo Code

See @README.md for product overview and workspace layout.
See @apps/theo-ui/README.md for frontend-only build/test commands.
See @crates/theo-agent-runtime/README.md for runtime invariants and guarded behaviors.

## Architecture

- `apps/*` are surfaces only. They should depend on `theo-application` and `theo-api-contracts`, not directly on lower crates.
- `theo-application` is the orchestration boundary for apps. Put app-facing use cases and narrow facades here.
- `theo-domain` holds pure domain types and contracts. Keep it dependency-light.
- `theo-engine-parser` extracts structure from source code via Tree-Sitter (14 languages).
- `theo-engine-graph` builds the code graph and clustering/community data.
- `theo-engine-retrieval` turns graph and lexical signals into search, context assembly, and impact inputs.
- `theo-engine-wiki` generates, stores, and queries the Code Wiki (page skeletons, hashing, lint).
- `theo-agent-runtime` runs the agent loop, tool dispatch, subagents, checkpoints, memory lifecycle, and observability.
- `theo-tooling` owns production tool implementations and the default registry.
- `theo-governance` owns policy/capability decisions (capability gates, sandbox cascade).
- `theo-isolation` owns worktree-based sub-agent isolation, port allocation, and safety rules.
- `theo-infra-llm` owns the 26-provider abstraction (streaming, retry, converter pipeline).
- `theo-infra-auth` owns OAuth PKCE, device flow, and env-key authentication.
- `theo-infra-mcp` owns the Model Context Protocol client.
- `theo-infra-memory` owns memory persistence and retrieval (ADR-008 pending).
- `theo-api-contracts` holds serializable DTOs for IPC between apps and crates.
- `theo-test-memory-fixtures` provides shared fixtures for memory subsystem tests.
- `theo-compat-harness` is excluded from workspace (orphaned, preserved for git-history reference).

## Workflow

- Prefer targeted commands first: `cargo test -p <crate>`, `cargo test -p <crate> <test_name>`, `cargo clippy -p <crate> --all-targets -- -D warnings`.
- Run `cargo fmt --all` after Rust edits.
- When touching architecture boundaries, run `make check-arch`.
- Before finishing a Rust change, run the narrowest relevant test plus `cargo clippy -p <affected-crate> --all-targets -- -D warnings`.
- After Rust changes, also run `make check-unwrap` and `make check-panic` on the affected crate paths to catch new production `unwrap()`/`panic!()` introductions.
- When adding `unsafe` blocks, ensure a `// SAFETY:` comment exists within 8 lines above; validate with `make check-unsafe`.
- When creating or modifying files, check file size stays under 800 LOC (Rust) / 400 LOC (TS) with `make check-sizes`.
- For PRs that touch `crates/` or `apps/`, update `CHANGELOG.md` under `[Unreleased]`; `make check-changelog` enforces this.
- For broad or risky changes, run `make check-sota-dod-quick` or `make check-sota-dod`.
- Frontend work lives in `apps/theo-ui`: use `npm test`, `npm run build`, and `npm run audit:circ` there.

## Code Style

- Rust edition is 2024. Match existing crate/module structure instead of inventing parallel abstractions.
- Use `tracing` for production diagnostics; do not introduce `eprintln!` in production Rust paths.
- Do not silently discard `Result`s from state-changing operations.
- Respect existing boundary rules: if an app needs lower-layer functionality, expose it through `theo-application` instead of importing the lower crate directly.
- Prefer `workspace = true` dependency declarations when using workspace crates or shared third-party deps.
- In React/TypeScript, keep strict typing green; `npm run build` must stay clean.

## Repository Gates

### Core gates (run frequently)

- `make check-arch` — enforces crate dependency direction (ADR-010).
- `make lint` — workspace clippy with warnings as errors.
- `make test` — Rust workspace tests.
- `make check-unwrap` — no `.unwrap()`/`.expect()` in production paths (T2.5).
- `make check-panic` — no `panic!()`/`todo!()`/`unimplemented!()` in production paths (T2.6).
- `make check-unsafe` — every `unsafe` block has a `// SAFETY:` comment (T2.9).
- `make check-sizes` — file LOC limits: 800 (Rust), 400 (TS) (T4.6).
- `make check-changelog` — CHANGELOG.md `[Unreleased]` updated when code changes (T6.5).

### Extended gates (CI and audits)

- `make check-secrets` — pattern-based secret scan with grep fallback (T6.2).
- `make check-io-tests` — detects misclassified I/O tests in `src/` (T5.2).
- `make check-complexity` — function LOC ceiling per crate via `clippy::too_many_lines` (threshold 100).
- `make check-sota-dod` / `make check-sota-dod-quick` — composite SOTA Tier 1+2 DoD.
- `make check-bench-preflight` — validates benchmark infra (scenarios + harness).
- `make check-allowlist-paths` — structural audit: every allowlist path resolves.
- `make check-workspace-deps` — every `[workspace.dependencies]` entry is used by ≥1 crate.
- `make check-env-var-coverage` — every documented `THEO_*` env var is read in source.
- `make audit` — runs all 8 audit techniques; expensive, do not run for small edits.

### Notes

- `apps/theo-desktop` depends on the built `apps/theo-ui/dist` bundle.
- Each `check-*` gate has a `--report` variant that prints results without failing.

## Common Pitfalls

- Do not bypass `theo-application` from `apps/*`.
- Do not split agent-runtime invariants across ad hoc helpers without checking existing runtime invariants first.
- Do not add wall-clock-derived IDs where the codebase already standardized on UUID v4.
- Do not widen a fix into unrelated refactors in this repository; many crates are under active audit/remediation.
