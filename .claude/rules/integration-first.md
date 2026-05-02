---
paths:
  - "crates/**/*.rs"
  - "apps/**/*.rs"
  - "apps/theo-ui/**/*.{ts,tsx}"
---

# Integration-First Development

## The Rule

A feature is NOT done until it is **wired, tested, and reachable**. Building
code that compiles but sits unused is a defect, not progress.

## Before Writing Code

1. **Trace the call path** — identify WHERE in the system this feature will
   be called from. Start from the user-facing surface (CLI, UI, agent loop)
   and work inward.
2. **Identify the integration test** — find which existing test exercises
   the path, or plan a new one.
3. **Check the dependency direction** — `apps/* → theo-application → crates/*`.
   If an app needs something from an engine crate, expose it through
   `theo-application`.

## After Writing Code

1. `cargo test -p <affected-crate>` — must pass, no exceptions.
2. `cargo test -p theo-application` — the integration boundary must stay green.
3. Verify the feature is **reachable** from at least one surface (CLI, UI,
   agent loop, or benchmark harness).
4. `cargo clippy -p <affected-crate> --all-targets -- -D warnings` — clean.

## Integration Checklist by Feature Type

| Feature type | Must be wired in | Test to verify |
|---|---|---|
| New tool | `DefaultRegistry` in theo-tooling | `build_registry` test |
| New domain type | Used in at least one use-case in theo-application | `cargo test -p theo-application` |
| New engine capability | Exposed through theo-application facade | Integration test |
| New CLI subcommand | Router + help test | `every_subcommand_responds_to_help_with_exit_zero` |
| New API contract | Consumed by at least one app | Contract test |
| New LLM provider | `ProviderSpec` in catalog + auth wiring | Provider catalog test |
| New UI component | Imported and rendered in a route/page | `npm test` |

## Anti-Patterns (FORBIDDEN)

- Creating a `pub fn`/`pub struct` that nothing calls — orphaned code.
- Implementing a tool but not adding it to `DefaultRegistry`.
- Adding a crate dependency in `Cargo.toml` without using it.
- Writing only unit tests and declaring integration "done" without running
  `cargo test -p theo-application`.
- Finishing work without running `cargo test -p <crate>` for every crate
  you touched.

## Real-World Test Obligation

When a feature touches the LLM pipeline (providers, agent loop, tool dispatch),
a real end-to-end test with an actual provider (OAuth Codex or equivalent) is
the gold standard. If a real provider test is not feasible in the current
session, explicitly state this limitation and document what manual verification
the user should perform.
