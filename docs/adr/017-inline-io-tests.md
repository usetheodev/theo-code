# ADR-017 v2 — Inline I/O test pattern requirements

**Status:** Accepted
**Date:** 2026-04-29
**Plan:** `docs/plans/code-hygiene-5x5-plan.md` (T5.1, Phase 5)
**Supersedes:** v1 (2026-04-23, file-list allowlist; replaced by codified pattern)

## Context

`scripts/check-inline-io-tests.sh` flags `#[cfg(test)] mod tests { … }`
blocks inside `crates/*/src/` and `apps/*/src/` that touch real I/O
(`std::fs`, `tokio::fs`, `tokio::process::*`, `TcpListener`, …).

The original idea (D2 / `.claude/rules/architecture.md`) was that I/O
tests belong in `crates/<crate>/tests/` because they are integration-
flavored: they exercise FS, network, or external processes. Keeping
them out of `src/` keeps the default `cargo test` quick and makes the
slow-vs-fast partition obvious.

**Problem with v1:** the v1 ADR shipped with a flat
`io-test-allowlist.txt` listing 94 file paths that legitimately *do*
need inline I/O. Every new test file with a `tempfile::TempDir` usage
forced an allowlist update — the same toil that ADR-021 fixed for
`unwrap`/`unsafe`/`panic`.

## Decision

Reverse the polarity: the *pattern* is allowed, not the *file*. An
inline I/O test that follows the codified pattern is **automatically
accepted** by the gate, with no allowlist entry needed. Files that
fail the pattern (e.g., write to `/tmp` directly) still gate-block.

## Pattern: `inline_io_test`

A `#[cfg(test)]` block in `crates/*/src/**/*.rs` or `apps/*/src/**/*.rs`
is allowed to perform real I/O **iff** the file imports one of the
following isolated-tempdir markers somewhere in scope:

- `use tempfile::TempDir;`
- `use tempfile::tempdir;`
- `use tempfile::Builder;` (for `Builder::new().tempdir()`)
- `use tempfile::NamedTempFile;`
- `tempfile::TempDir`, `tempfile::tempdir!`, `tempfile::tempdir()` (literal)

Plus any existing in-project test fixture wrapper that ultimately wraps
`tempfile::*` (e.g., `crate::test_helpers::TestDir`).

**Why tempfile?** It guarantees:
1. Each test gets a fresh, unique directory (`std::env::temp_dir()`
   plus a per-process counter).
2. The directory is removed on drop, even if the test panics (RAII).
3. No shared mutable state between tests in the same module.

That covers the four invariants the plan listed:

1. **Isolated FS root** — every mutation lives under the per-test
   tempdir. `/tmp` direct writes are NOT covered.
2. **Determinism** — `TempDir::new()` returns a unique path per call;
   re-runs cannot collide.
3. **No shared state** — RAII drop cleans up; re-running gives a fresh
   dir.
4. **Test names** — orthogonal; covered by `.claude/rules/testing.md`.

## What the gate enforces

`scripts/check-inline-io-tests.sh` v2 logic:

1. Find every file in `crates/*/src/` and `apps/*/src/` that contains
   `#[cfg(test)]` AND any of the I/O markers (FS, tokio process, raw
   sockets).
2. For each such file, check whether it ALSO contains one of the
   `tempfile::*` markers above OR uses an in-project test fixture
   wrapper.
3. If yes → recognized pattern, allow.
4. If no → violation. The fix is one of:
   - Add `use tempfile::TempDir;` and switch the test to a per-test
     tempdir.
   - Move the test to `crates/<crate>/tests/` (out of `src/`).
   - Document why the test must NOT use tempfile in an ADR-017
     extension and add the file to the path allowlist (last resort).

## Anti-patterns this rejects

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn writes_a_file() {
        // /tmp direct usage — NOT allowed (no per-test isolation)
        std::fs::write("/tmp/test.txt", b"hi").unwrap();
        // ... assert ...
    }
}
```

```rust
#[cfg(test)]
mod tests {
    static SHARED_DIR: &str = "tests/fixtures";  // shared mutable state — NOT allowed

    #[test]
    fn test_one() {
        std::fs::write(format!("{SHARED_DIR}/x"), b"1").unwrap();
    }

    #[test]
    fn test_two() {
        std::fs::write(format!("{SHARED_DIR}/x"), b"2").unwrap();  // race vs test_one
    }
}
```

## What gets allowed automatically (post-v2)

Every existing v1 allowlist entry has been re-validated; ~95 % of the
94 files use `tempfile::TempDir` already. The handful that don't either:
- Use a project-internal `TestDir` wrapper (which wraps `tempfile`)
- Are pure sentinel-string tests (e.g., `crates/theo-cli/src/render/`
  that read embedded `include_str!` content — no real I/O after all,
  but caught by the heuristic)

## Consequences

- **No more file-list maintenance.** Adding a 50th test file with
  `tempfile::TempDir` no longer requires an allowlist entry.
- **The gate catches the *real* problem** — `/tmp` direct writes,
  shared mutable state — instead of being a reviewer-toil generator.
- **`scripts/check-inline-io-tests.sh` is now a pattern validator.**
  Same shape as `check-unwrap.sh` and `check-unsafe.sh` after T2.2.
- **`io-test-allowlist.txt` shrinks to 0 active entries.** Whatever
  remains is documented as deviation in this ADR.

## Cross-references

- `docs/adr/021-recognized-rust-idioms.md` — sister ADR; same
  "patterns not exceptions" treatment for `unwrap` / `unsafe` / `panic`.
- `.claude/rules/recognized-patterns.toml` — machine-readable companion;
  T5.1 added `[[io_test_pattern]]` entries.
- `scripts/check-inline-io-tests.sh` — gate that enforces this ADR.
- `docs/plans/code-hygiene-5x5-plan.md` — Phase 5 (T5.1).
