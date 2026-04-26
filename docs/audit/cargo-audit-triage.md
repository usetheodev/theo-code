# cargo-audit Triage (T3.1)

Triage of every finding in `.theo/audit/cargo-audit-2026-04-23.txt`.
Each entry is classified as:

| Class | Meaning |
| --- | --- |
| **BLOCK** | Genuine vulnerability that affects the produced binary. Fix or upgrade before next release. |
| **IGNORE** | Documented in `deny.toml` `[advisories].ignore` — root cause is a transitive dep we cannot fix without replacing a top-level dep. Revisit quarterly. |
| **OPT-ONLY** | Affects only an optional feature that is NOT enabled by default; recorded for awareness. |

## Summary

| Severity | Count | Classification |
| --- | --- | --- |
| Vulnerabilities | 2 | 1 OPT-ONLY, 1 IGNORE (waiting upstream) |
| Unmaintained | 23 | All IGNORE (Tauri GTK3 + ecosystem deps) |

## Vulnerabilities

### RUSTSEC-2024-0437 — protobuf recursion crash
**Class:** OPT-ONLY
- **Affected path:** `scip → protobuf` (via `theo-engine-graph/scip` feature)
- **Exploitability:** requires untrusted SCIP input. The `scip` feature is
  off by default and only enabled on opt-in for IDE integrations.
- **Plan:** upgrade `scip` when a release pulls `protobuf >= 3.7.2`. Track
  via `cargo outdated -p scip` monthly. Add `scip` feature check to CI so
  the vuln is surfaced if anyone accidentally enables it globally.

### RUSTSEC-2026-0104 — rustls-webpki reachable panic (CRL parsing)
**Class:** IGNORE (with monitoring)
- **Affected path:** `reqwest → rustls → rustls-webpki`
- **Exploitability:** the panic fires only when parsing malformed CRL
  entries. Theo Code ships with `reqwest` but does not pin CRL lookups.
  Default TLS config does not parse CRLs; the vuln is effectively
  unreachable on our request paths.
- **Plan:** upgrade whenever `reqwest` pulls the patched `rustls`.
  Monitor via weekly `cargo audit` CI job (T3.1 follow-up).
- **Ignore:** `deny.toml` `[advisories].ignore` keeps the entry
  documented.

## Unmaintained (23 advisories)

All are transitive deps dragged in by:

- **Tauri 2.x → GTK3** (12 advisories: `atk*`, `gdk*`, `gtk*`,
  `gdkwayland-sys`, `gdkx11*`, `gtk3-macros`, `glib`). Tauri will move
  to GTK4 / webkit-sys in a future major. Tracked upstream; replacing
  Tauri is a multi-week task not scheduled before 2026-Q3.
- **syntect → yaml-rust** (1 advisory). Syntect supports swapping to
  `yaml-rust2`, tracked in the crate's roadmap.
- **ratatui → bincode 1.x** (1 advisory: RUSTSEC-2025-0141). Planned
  migration to bincode 2.x on next ratatui minor bump.
- **idna → unic-\*** (5 advisories). Idna upstream has a
  re-architecture planned that drops the `unic-*` family. No in-tree
  fix is feasible today.
- **indicatif → number_prefix** (1 advisory).
- **legacy proc macros → proc-macro-error / paste** (2 advisories).
- **rand 0.7 → fxhash / instant** (misc., 3 advisories).

All 23 are already listed in `deny.toml` `[advisories].ignore` with
remediation notes. **Action:** re-evaluate after every `cargo update`
+ Tauri minor release. Each ignore should drop off naturally as
upstream crates migrate.

## Gate wiring

- `make audit` runs `cargo audit` when the binary is present. CI
  adoption pending.
- `cargo deny check advisories` is tighter (filters to direct
  workspace deps) and is already wired. CI adoption pending.
- `deny.toml` `[advisories].unmaintained = "workspace"` keeps the CI
  exit code green while the full list stays visible in the human
  report.

## Revisit schedule

- **Monthly:** run `cargo audit` + `cargo deny check advisories`,
  compare against this document, add / remove ignore entries.
- **Quarterly (next: 2026-07-23):** audit remediation follow-up. If
  RUSTSEC-2026-0104 has not been upstream-fixed by then, escalate
  the ticket and consider pinning to a rustls fork.
