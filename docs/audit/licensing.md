# Licensing Policy (T3.2)

Declared in `deny.toml` at the workspace root. Evaluated by `cargo deny check`
(install via `make audit-tools`).

## Allowed licenses

Permissive licenses accepted without exception:

| License | SPDX id | Notes |
| --- | --- | --- |
| MIT | MIT | Default for this workspace (`license = "MIT"` in root `Cargo.toml`). |
| Apache 2.0 | Apache-2.0 | Dual-license candidate for future publication. |
| Apache 2.0 with LLVM exception | Apache-2.0 WITH LLVM-exception | LLVM-derived toolchain deps. |
| BSD 2-Clause | BSD-2-Clause | |
| BSD 3-Clause | BSD-3-Clause | |
| ISC | ISC | |
| Unicode | Unicode-DFS-2016, Unicode-3.0 | Unicode tables. |
| Zlib | Zlib | |
| Public domain / CC0 | CC0-1.0 | |
| MPL 2.0 | MPL-2.0 | Acceptable because our downstream distribution does not link statically against copyleft code. |
| Unlicense | Unlicense | |
| 0BSD | 0BSD | |
| CDLA Permissive | CDLA-Permissive-2.0 | Used by ML model metadata (fastembed). |
| Boost Software License | BSL-1.0 | `clipboard-win` (transitive via `rustyline`). |
| NCSA | NCSA | `libfuzzer-sys` (transitive via `fastembed`). Compound: `(MIT OR Apache-2.0) AND NCSA`. |

Adding a new license to the allow list **requires a PR with justification**.
Copyleft (GPL/AGPL/LGPL) and SSPL are **not** on the list and will fail the gate.

## Advisory policy

| Knob | Setting | Rationale |
| --- | --- | --- |
| `unmaintained` | `workspace` | Only workspace-direct unmaintained deps fail CI. Transitive unmaintained crates (Tauri GTK3, `yaml-rust` via syntect, `bincode` via ratatui, unic-\* via idna) are surfaced as warnings. |
| `version` | 2 | Current cargo-deny advisories schema. |

Specific `RUSTSEC-*` ignores live in the `[advisories].ignore` array; each
must carry a reason + remediation plan. Today's ignores are 100% transitive
third-party deprecations that the workspace cannot fix without replacing
toplevel deps (Tauri 2.x, syntect, ratatui).

## Bans policy

| Knob | Setting | Rationale |
| --- | --- | --- |
| `multiple-versions` | `warn` | Tauri's dep graph pulls many old versions. Flip to `deny` once Tauri upgrades or is replaced. |
| `wildcards` | `warn` | Our workspace crates depend on each other via `path = "…"`, which cargo-deny classifies as wildcards. Flip to `deny` once every internal crate either declares `version = "…"` or marks itself `publish = false`. |
| `allow-wildcard-paths` | `true` | Shield private (non-published) crates from the wildcard ban. |

## Sources policy

Crates may only come from `https://github.com/rust-lang/crates.io-index`.
Unknown git registries trigger a warning (`unknown-git = "warn"`). Adding a
new git dep requires an explicit entry under `[sources].allow-git`.

## Running locally

```bash
# Install cargo-deny if missing.
make audit-tools

# Full gate.
cargo deny check

# Per-section (for focused debugging).
cargo deny check advisories
cargo deny check bans
cargo deny check licenses
cargo deny check sources
```

## CI integration

`make audit` already invokes `cargo deny check` when the binary is present.
CI wiring in `.github/workflows/ci.yml` should add a dedicated job that
installs cargo-deny (e.g. via `EmbarkStudios/cargo-deny-action@v2`) and runs
the full gate on every PR. Pending — tracked under T3.2 DoD.
