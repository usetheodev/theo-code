---
paths:
  - "crates/**/*.rs"
  - "apps/theo-cli/**/*.rs"
  - "apps/theo-desktop/**/*.rs"
---

# Rust Conventions

## Error Handling
- Use `thiserror` for library-facing error types; binary-only glue may use `anyhow`.
- `unwrap()` / `expect()` in production code are exceptional and must match recognized patterns or a gate-approved allowlist entry.
- Errors must carry context: what happened, which entity, what was expected.
- Prefer typed errors over stringly-typed propagation across crate boundaries.

## Types
- Shared types live in `theo-domain`. Import from there.
- Add public domain contracts to `theo-domain`; keep app/runtime-only details out of it.
- Use `#[non_exhaustive]` when a public enum is intended to grow across surfaces.

## Dependencies
- Declare workspace deps in root `Cargo.toml` `[workspace.dependencies]`.
- Each crate uses `dep.workspace = true` in its own `Cargo.toml`.
- Optional/heavy capabilities should stay feature-gated when that pattern already exists in the crate.

## Testing
- This repo uses both inline `#[cfg(test)]` modules and sibling `*_tests.rs` files. Follow the local pattern of the module you are editing.
- Integration tests belong in `tests/` when they exercise crate boundaries or workflows.
- Use descriptive test names and deterministic fixtures.

## Async
- `tokio` as the async runtime. Use `#[tokio::test]` for async tests.
- Prefer `tokio::spawn` over `std::thread::spawn`.
- Use `tokio::select!` for concurrent operations, not busy polling.

## Style
- `cargo fmt` and `cargo clippy` must pass with zero warnings.
- Code in English. Comments in English for technical context.
- Imports: std first, external crates second, internal crates third.
- Use `tracing` in production paths; do not add new `eprintln!` outside CLI-style process entrypoints or explicit user-facing binaries.
- Every `unsafe` block needs a `// SAFETY:` justification unless a gate-recognized pattern explicitly covers it.
