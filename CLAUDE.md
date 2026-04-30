# Theo Code

See @README.md for product overview and workspace layout.
See @apps/theo-ui/README.md for frontend-only build/test commands.
See @crates/theo-agent-runtime/README.md for runtime invariants and guarded behaviors.

## Architecture

- `apps/*` are surfaces only. They should depend on `theo-application` and `theo-api-contracts`, not directly on lower crates.
- `theo-application` is the orchestration boundary for apps. Put app-facing use cases and narrow facades here.
- `theo-domain` holds pure domain types and contracts. Keep it dependency-light.
- `theo-engine-parser` extracts structure from source code via Tree-Sitter.
- `theo-engine-graph` builds the code graph and clustering/community data.
- `theo-engine-retrieval` turns graph and lexical signals into search, context assembly, and impact inputs.
- `theo-agent-runtime` runs the agent loop, tool dispatch, subagents, checkpoints, memory lifecycle, and observability.
- `theo-tooling` owns production tool implementations and the default registry.
- `theo-governance` owns policy/capability decisions; `theo-isolation` owns sandbox/worktree isolation.
- `theo-infra-llm`, `theo-infra-auth`, `theo-infra-mcp`, and `theo-infra-memory` own external integrations and persistence concerns.

## Workflow

- Prefer targeted commands first: `cargo test -p <crate>`, `cargo test -p <crate> <test_name>`, `cargo clippy -p <crate> --all-targets -- -D warnings`.
- Run `cargo fmt --all` after Rust edits.
- When touching architecture boundaries, run `make check-arch`.
- Before finishing a Rust change, run the narrowest relevant test plus `cargo clippy -p <affected-crate> --all-targets -- -D warnings`.
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

- `make check-arch` enforces crate dependency direction.
- `make lint` runs workspace clippy with warnings as errors.
- `make test` runs the Rust workspace tests.
- `make audit` is the expensive full audit suite; do not run it by default for small edits.
- `apps/theo-desktop` depends on the built `apps/theo-ui/dist` bundle.

## Common Pitfalls

- Do not bypass `theo-application` from `apps/*`.
- Do not split agent-runtime invariants across ad hoc helpers without checking existing runtime invariants first.
- Do not add wall-clock-derived IDs where the codebase already standardized on UUID v4.
- Do not widen a fix into unrelated refactors in this repository; many crates are under active audit/remediation.
