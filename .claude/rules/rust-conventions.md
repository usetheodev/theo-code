---
paths:
  - "crates/**/*.rs"
  - "apps/theo-cli/**/*.rs"
  - "apps/theo-desktop/**/*.rs"
---

# Rust Conventions

## Error Handling
- Use `thiserror` for error types. One error enum per crate.
- Never `unwrap()` or `expect()` in production code. Use `?` operator.
- Errors must carry context: what happened, which entity, what was expected.
- `anyhow` only in binary targets (CLI), never in libraries.

## Types
- Shared types live in `theo-domain`. Import from there.
- Newtypes for domain identifiers: `FileId(u32)`, `SymbolId(u32)`.
- `#[non_exhaustive]` on public enums that may grow.

## Dependencies
- Declare workspace deps in root `Cargo.toml` `[workspace.dependencies]`.
- Each crate uses `dep.workspace = true` in its own `Cargo.toml`.
- Feature flags for optional heavy deps (embeddings, GPU).

## Testing
- Tests in the same file for unit tests (`#[cfg(test)] mod tests`).
- Integration tests in `tests/` directory of each crate.
- Use `#[test]` with descriptive names: `test_retrieval_returns_empty_for_unknown_symbol`.
- Arrange-Act-Assert pattern. One assertion focus per test.

## Async
- `tokio` as the async runtime. Use `#[tokio::test]` for async tests.
- Prefer `tokio::spawn` over `std::thread::spawn`.
- Use `tokio::select!` for concurrent operations, not busy polling.

## Style
- `cargo fmt` and `cargo clippy` must pass with zero warnings.
- Code in English. Comments in English for technical context.
- Imports: std first, external crates second, internal crates third.
