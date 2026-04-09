---
name: build
description: Build the workspace or specific crate. Use when asked to build, compile, or check compilation.
user-invocable: true
allowed-tools: Bash(cargo *)
argument-hint: "[crate|ui|desktop|check]"
---

Build the Theo Code workspace.

## Arguments

- No args: `cargo build` (full workspace)
- `check`: `cargo check` (type-check only, faster)
- `ui`: `cd apps/theo-ui && npm run build`
- `desktop`: `cd apps/theo-desktop && cargo tauri build`
- Crate name: `cargo build -p $ARGUMENTS`

## Steps

1. Run the appropriate build command
2. If errors: analyze the error, show file:line, explain the issue
3. If warnings: list them grouped by crate
4. **Run tests**: `cargo test -p <crate>` for affected crate (TDD — build alone is not enough)
5. Report: PASS (build + tests clean) / PASS with N warnings / FAIL with errors

## TDD Gate

Build is NOT complete until tests pass. After a successful build, always run:
```bash
cargo test -p <crate>  # For specific crate builds
cargo test             # For full workspace builds
```

If tests fail → report as FAIL even if build succeeds. Code that compiles but fails tests is broken code.
