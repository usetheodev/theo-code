# Dependency Analysis Report

Generated: 2026-04-03
Analyzer: dependency-analyzer (claude-sonnet-4-6)

---

## Summary

| Metric | Value |
|--------|-------|
| Internal crates (workspace members) | 13 |
| External direct dependencies (Rust) | ~40 unique |
| Total transitive dependencies (Cargo.lock) | 699 |
| Frontend npm direct dependencies | 25 (13 prod + 12 dev) |
| Circular dependencies detected | 0 |
| Architecture rule violations | 5 |
| Unpinned workspace deps (Rust) | 0 (all version-ranged, lock file present) |
| Unpinned npm deps | 25/25 (all use ^ caret ranges) |
| Known vulnerabilities (CVEs) | 0 |
| Audit warnings (unmaintained/unsound) | 21 |
| Direct actionable warnings (non-transitive) | 3 |
| serde_yaml deprecated dependency | 1 |
| fastembed major version behind | 1 (v4.9.1, latest: v5.13.0) |
| Unused workspace declarations | 1 (grep-regex) |

---

## Internal Dependency Graph

```
theo-domain          → (none — pure types, zero deps)
theo-api-contracts   → theo-domain (indirect via serde only — no explicit dep)
theo-engine-graph    → theo-domain (indirect via serde)
theo-engine-parser   → theo-domain (indirect via serde)
theo-engine-retrieval → theo-engine-graph
theo-governance      → theo-domain, theo-engine-graph
theo-infra-auth      → theo-domain (indirect via tokio/serde)
theo-infra-llm       → theo-domain (indirect via tokio/serde)
theo-tooling         → theo-domain
theo-agent-runtime   → theo-domain, theo-tooling, theo-infra-llm, theo-infra-auth
theo-application     → theo-domain, theo-agent-runtime, theo-tooling, theo-api-contracts

apps/theo-cli        → theo-domain, theo-engine-graph, theo-engine-parser,
                       theo-engine-retrieval, theo-governance, theo-tooling,
                       theo-application, theo-agent-runtime, theo-infra-auth,
                       theo-infra-llm
apps/theo-desktop    → theo-agent-runtime, theo-infra-auth, theo-infra-llm,
                       theo-tooling, theo-api-contracts, theo-application
```

### Circular Dependencies

None detected. Cargo enforces this at compile time.

---

## Architecture Rule Violations

The `.claude/rules/architecture.md` defines a strict allowed dependency matrix:

```
theo-domain         → (none)
theo-engine-*       → theo-domain
theo-governance     → theo-domain
theo-infra-*        → theo-domain
theo-tooling        → theo-domain
theo-agent-runtime  → theo-domain, theo-governance
theo-api-contracts  → theo-domain
theo-application    → all crates above
apps/*              → theo-application, theo-api-contracts
```

### Violations Found

| # | Crate | Violation | Actual Dep | Allowed? |
|---|-------|-----------|-----------|----------|
| 1 | `theo-governance` | Imports engine directly | `theo-engine-graph` | No — governance → theo-domain only |
| 2 | `theo-agent-runtime` | Imports infra directly | `theo-infra-llm` | No — runtime → theo-domain, theo-governance only |
| 3 | `theo-agent-runtime` | Imports infra directly | `theo-infra-auth` | No — runtime → theo-domain, theo-governance only |
| 4 | `apps/theo-cli` | App imports engines directly | `theo-engine-graph`, `theo-engine-parser`, `theo-engine-retrieval`, `theo-governance` | No — apps → theo-application, theo-api-contracts only |
| 5 | `apps/theo-cli` | App imports infra directly | `theo-infra-auth`, `theo-infra-llm` | No — apps → theo-application, theo-api-contracts only |

Note: `apps/theo-desktop` imports `theo-infra-auth` and `theo-infra-llm` directly via path references, which technically violates the same rule as violation #5.

---

## External Dependency Inventory

### Direct Dependencies — Rust (Workspace Level)

| Package | Version Pinned | Latest | Status | License | Notes |
|---------|---------------|--------|--------|---------|-------|
| tokio | `"1"` (minor) | 1.51.0 | Current | MIT | Core async runtime |
| serde | `"1"` (minor) | 1.0.228 | Current | MIT/Apache-2.0 | Serialization framework |
| serde_json | `"1"` (minor) | 1.0.149 | Current | MIT/Apache-2.0 | JSON support |
| thiserror | `"2"` (minor) | 2.0.18 | Current | MIT/Apache-2.0 | Error derive macro |
| async-trait | `"0.1"` (patch) | 0.1.89 | Current | MIT/Apache-2.0 | Async trait support |
| reqwest | `"0.12"` (patch) | 0.12.28 | Current | MIT/Apache-2.0 | HTTP client |
| regex | `"1"` (minor) | 1.12.3 | Current | MIT/Apache-2.0 | Regex engine |
| walkdir | `"2"` (minor) | 2.5.0 | Current | MIT/Unlicense | Directory traversal |
| ignore | `"0.4"` (patch) | 0.4.25 | Current | MIT/Unlicense | Gitignore traversal |
| similar | `"2"` (minor) | 2.7.0 | Current | Apache-2.0 | Diff library |
| glob | `"0.3"` (patch) | 0.3.3 | Current | MIT/Apache-2.0 | Glob patterns |
| landlock | `"0.4"` (patch) | 0.4.4 | Current | MIT/Apache-2.0 | Linux sandboxing |
| libc | `"0.2"` (patch) | 0.2.183 | Current | MIT/Apache-2.0 | C bindings |
| rustyline | `"14"` (minor) | 14.0.0 | Current | MIT | CLI readline |
| tempfile | `"3"` (minor) | 3.27.0 | Current | MIT/Apache-2.0 | Temp files |
| **toml** | `"0.8"` (patch) | 0.8.2 | Current | MIT/Apache-2.0 | TOML parsing — **verified in workspace** |
| **grep-regex** | `"0.1"` (patch) | 0.1.x | **UNUSED** | MIT/Unlicense | Declared in workspace, never referenced |

### Direct Dependencies — Crate-Level (Not in Workspace)

| Package | Crate | Version | Latest | Status | License | Notes |
|---------|-------|---------|--------|--------|---------|-------|
| tree-sitter | engine-graph, engine-parser | `"0.26"` | 0.26.8 | Current | MIT | Parser runtime |
| tree-sitter-rust | engine-graph, engine-parser | `"0.24"` | ~0.24 | Current | MIT | Language grammar |
| tree-sitter-python | engine-graph, engine-parser | `"0.25"` | ~0.25 | MIT | Current | Language grammar |
| tree-sitter-typescript | engine-graph, engine-parser | `"0.23.2"` | ~0.23 | MIT | Current | Language grammar |
| tree-sitter-javascript | engine-graph, engine-parser | `"0.25"` | ~0.25 | MIT | Current | Language grammar |
| tree-sitter-c | engine-parser | `"0.24"` | ~0.24 | MIT | Current | Language grammar |
| tree-sitter-go | engine-parser | `"0.25"` | ~0.25 | MIT | Current | Language grammar |
| tree-sitter-java | engine-parser | `"0.23.5"` | ~0.23 | MIT | Current | Language grammar |
| tree-sitter-kotlin-ng | engine-parser | `"1.1"` | ~1.1 | MIT | Current | Language grammar |
| tree-sitter-php | engine-parser | `"0.24.2"` | ~0.24 | MIT | Current | Language grammar |
| tree-sitter-ruby | engine-parser | `"0.23.1"` | ~0.23 | MIT | Current | Language grammar |
| tree-sitter-scala | engine-parser | `"0.24"` | ~0.24 | MIT | Current | Language grammar |
| tree-sitter-swift | engine-parser | `"0.7.1"` | ~0.7 | MIT | Current | Language grammar |
| tree-sitter-cpp | engine-parser | `"0.23.4"` | ~0.23 | MIT | Current | Language grammar |
| tree-sitter-c-sharp | engine-parser | `"0.23.1"` | ~0.23 | MIT | Current | Language grammar |
| rayon | engine-graph, engine-parser, theo-cli | `"1"` | 1.11.0 | MIT/Apache-2.0 | Current | Parallelism |
| **bincode** | engine-graph, theo-cli | `"1"` | **3.0.0** | **MAJOR BEHIND** | MIT | RUSTSEC-2025-0141: unmaintained |
| **fastembed** | engine-retrieval | `"4"` | **5.13.0** | **MAJOR BEHIND** | Apache-2.0 | Pulls ONNX runtime + tokenizers |
| sha2 | engine-parser, infra-auth | `"0.10"` | 0.10.9 | MIT/Apache-2.0 | Current | Cryptographic hashing |
| **serde_yaml** | engine-parser | `"0.9"` | **DEPRECATED** | Warning | MIT/Apache-2.0 | Self-marked deprecated on crates.io (0.9.34+deprecated) |
| futures | agent-runtime, infra-llm | `"0.3"` | 0.3.32 | MIT/Apache-2.0 | Current | Async combinators |
| bytes | infra-llm | `"1"` | 1.11.1 | MIT | Current | Byte buffers |
| base64 | infra-auth | `"0.22"` | 0.22.1 | MIT/Apache-2.0 | Current | Base64 encoding |
| rand | infra-auth | `"0.9"` | 0.9.2 | MIT/Apache-2.0 | Current (0.10.0 just released) |
| dirs | infra-auth | `"6"` | 6.0.0 | MIT/Apache-2.0 | Current | Platform dirs |
| ort | (via fastembed) | `2.0.0-rc.9` | 2.0.0-rc.12 | MIT | Pre-release RC | ONNX Runtime binding |
| tauri | theo-desktop | `"2"` | 2.10.3 | MIT/Apache-2.0 | Current | Desktop framework |
| tauri-plugin-shell | theo-desktop | `"2"` | 2.3.5 | MIT/Apache-2.0 | Current | Shell commands plugin |
| tauri-plugin-dialog | theo-desktop | `"2"` | 2.6.0 | MIT/Apache-2.0 | Current | Dialog plugin |

### Frontend npm Dependencies (apps/theo-ui)

| Package | Version Spec | Pinned? | Notes |
|---------|-------------|---------|-------|
| @radix-ui/react-* (5 packages) | `^1.x` | No (caret) | UI primitives — MIT |
| @tauri-apps/api | `^2` | No (major) | Tauri bridge — MIT |
| @tauri-apps/plugin-dialog | `^2.6.0` | No (caret) | Tauri plugin — MIT |
| framer-motion | `^12.38.0` | No (caret) | Animation — MIT |
| lucide-react | `^1.7.0` | No (caret) | Icons — ISC |
| react + react-dom | `^18.3.1` | No (caret) | Core UI — MIT |
| react-router + react-router-dom | `^6.30.3` | No (caret) | Routing — MIT |
| vite | `^6.0.0` | No (caret) | Build tool — MIT |
| typescript | `^5.6.3` | No (caret) | Type checking — Apache-2.0 |
| tailwindcss | `^3.4.19` | No (caret) | Styling — MIT |

npm audit result: 0 vulnerabilities (clean).
Lock file: `package-lock.json` present — builds are reproducible despite caret ranges.

---

## Transitive Dependency Tree

Total: 699 crates in Cargo.lock.

The primary contributor to transitive depth is `fastembed` in `theo-engine-retrieval`:

```
theo-engine-retrieval
  → fastembed v4.9.1
      → ort v2.0.0-rc.9 (ONNX Runtime — RC release, heavy native binary)
      → tokenizers v0.21.4 (HuggingFace — significant dep tree)
      → hf-hub v0.4.3 (downloads models at runtime)
      → image v0.25.10 (full image processing stack)
      → ndarray v0.16.1 (numerical arrays)
```

This single dependency (`fastembed`) accounts for an estimated 200-300 of the 699 total transitive deps, including the full ONNX Runtime native library linkage.

**Version conflict duplicates detected (cargo tree --duplicates):**

| Package | Versions Present | Root Cause |
|---------|-----------------|-----------|
| base64 | 0.13.1, 0.22.1 | fastembed/tokenizers uses old version |
| getrandom | 0.1.16, 0.2.17, 0.3.4, 0.4.2 | Four different versions! Major fragmentation |
| rand | 0.7.3, 0.8.5, 0.9.2 | Three versions — fastembed pulls older ones |
| rand_chacha | 0.2.2, 0.3.1, 0.9.0 | Same cause as rand |
| rand_core | 0.5.1, 0.6.4, 0.9.5 | Same cause as rand |
| syn | 1.0.109, 2.0.117 | Proc-macro ecosystem transition (normal) |
| thiserror | 1.0.69, 2.0.18 | Some transitive deps use thiserror v1 |
| indexmap | 1.9.3, 2.13.0 | Same ecosystem transition pattern |
| nom | 7.1.3, 8.0.0 | Two parser versions |
| toml | 0.8.2, 0.9.12 | Workspace uses 0.8, some transitive use 0.9 |
| winnow | 0.5.40, 0.7.15, 1.0.1 | Three versions of toml's parser |
| bitflags | 1.3.2, 2.11.0 | Expected — ecosystem-wide migration |

---

## Vulnerability Report

### CVE Findings

None. `cargo audit` reports **0 security vulnerabilities** (no CVEs).

### Unmaintained / Unsound Warnings (21 total)

| Crate | RUSTSEC ID | Type | Root Cause Dep | Actionable? |
|-------|-----------|------|---------------|-------------|
| atk, atk-sys | RUSTSEC-2024-0413/0416 | unmaintained | GTK3 via tauri/wry (Linux) | No — wait for Tauri GTK4 migration |
| gdk, gdk-sys, gdkwayland-sys, gdkx11, gdkx11-sys | RUSTSEC-2024-0411/0412/0417/0418 | unmaintained | GTK3 via tauri/wry | No — same as above |
| gtk, gtk-sys, gtk3-macros | RUSTSEC-2024-0415/0419/0420 | unmaintained | GTK3 via tauri/wry | No — same as above |
| glib | RUSTSEC-2024-0429 | **unsound** | GTK3 via tauri/wry | No — wait for Tauri |
| **bincode v1.3.3** | RUSTSEC-2025-0141 | unmaintained | theo-cli + theo-engine-graph direct dep | **YES — migrate to bincode v3** |
| fxhash | RUSTSEC-2025-0057 | unmaintained | transitive via tokenizers/fastembed | No — indirect |
| number_prefix | RUSTSEC-2025-0119 | unmaintained | transitive (likely indicatif progress bar) | No — indirect |
| paste | RUSTSEC-2024-0436 | unmaintained | transitive | No — indirect |
| proc-macro-error | RUSTSEC-2024-0370 | unmaintained | transitive | No — indirect |
| unic-char-property, unic-char-range, unic-common, unic-ucd-ident, unic-ucd-version | RUSTSEC-2025-0075/0080/0081/0098/0100 | unmaintained | transitive via unicode processing | No — indirect |

**Bottom line:** 14 of 21 warnings are the GTK3 cluster (single root: Tauri on Linux). 5 are unicode crates from fastembed's tokenizer chain. Only `bincode` is a direct actionable finding.

---

## License Compatibility

All dependencies use permissive licenses (MIT, Apache-2.0, Unlicense, ISC, BSD).

| License | Count (estimated) | Compatible with MIT project? |
|---------|------------------|------------------------------|
| MIT | ~400 | Yes |
| Apache-2.0 | ~200 | Yes (attribution required in NOTICE) |
| MIT / Apache-2.0 (dual) | ~80 | Yes |
| Unlicense | ~5 | Yes |
| ISC | ~10 | Yes |

No GPL, AGPL, LGPL, SSPL, or Commons Clause dependencies detected. License posture is clean.

---

## Health Assessment

### Healthy Dependencies (no action needed)

- tokio, serde, serde_json, thiserror, async-trait, reqwest, regex, walkdir, ignore, similar, glob, landlock, libc, rustyline, tempfile, futures, bytes, base64, dirs, sha2
- All tree-sitter language grammars
- All Tauri v2 crates (active development, regular releases)
- All @radix-ui and React frontend deps
- toml 0.8 (confirmed in workspace, used by theo-agent-runtime and theo-engine-parser)

### Warning Dependencies (monitor)

| Dependency | Risk | Reason | Action |
|-----------|------|--------|--------|
| fastembed v4.9.1 | Medium | One major version behind (v5.13.0). Pulls ONNX Runtime RC (v2.0.0-rc.9, latest rc.12), which is pre-release software in production. | Evaluate fastembed v5 migration; it may have breaking API changes. |
| serde_yaml v0.9 | Medium | The crate self-published as `0.9.34+deprecated`. The maintainer explicitly deprecated it and recommends migrating. | Migrate the one usage in `theo-engine-parser/src/workspace/detect.rs` to `serde-saphyr` or `yaml-rust2`. |
| ort v2.0.0-rc.9 | Low-Medium | Pre-release RC in production. Latest is rc.12. No stable release yet. | Controlled by fastembed — update fastembed to update ort. |
| rand v0.9.2 | Low | rand v0.10.0 was just released. Minor API changes expected. | Low urgency; update when convenient. |

### Critical Dependencies (action required)

| Dependency | Risk | Reason | Recommendation |
|-----------|------|--------|---------------|
| **bincode v1.3.3** | High | RUSTSEC-2025-0141: officially declared unmaintained (Dec 2025). Used directly in `theo-engine-graph` (serialization of the code graph) and `apps/theo-cli`. bincode v3.0.0 is a complete rewrite with breaking API changes. | Migrate to `bincode` v3. The API changed significantly (no longer serde-compatible by default — uses its own derive macros). Evaluate also `postcard` as a lighter alternative. |
| **grep-regex in workspace** | Low | Declared in `[workspace.dependencies]` at root Cargo.toml but never referenced by any crate. Dead declaration adds noise and confusion. | Remove `grep-regex = "0.1"` from `[workspace.dependencies]`. |

---

## Consistency Issues

### toml Crate Inconsistency

The `toml` crate is declared in `[workspace.dependencies]` correctly. However `theo-engine-parser` uses `toml = "0.8"` directly instead of `toml.workspace = true`. This creates two separate dependency declarations for the same thing, making version management harder.

File: `/home/paulo/Projetos/usetheo/theo-code/crates/theo-engine-parser/Cargo.toml` line 30.

### Path vs Workspace References

Several internal crates use `{ path = "../..." }` instead of `.workspace = true` even though they are declared in the workspace:

- `theo-agent-runtime`: `theo-infra-llm = { path = "../theo-infra-llm" }` and `theo-infra-auth = { path = "../theo-infra-auth" }`
- `theo-engine-retrieval`: `theo-engine-graph = { path = "../theo-engine-graph" }`
- `theo-governance`: `theo-engine-graph = { path = "../theo-engine-graph" }`
- `apps/theo-cli`: `theo-infra-auth = { path = "../../crates/theo-infra-auth" }` and `theo-infra-llm`
- `apps/theo-desktop`: same

These should use `.workspace = true` for all internal crates to centralize version management.

---

## Recommendations

### Priority 1 — Immediate (Architecture Integrity)

1. **Enforce the dependency matrix from architecture.md.** Five violations exist where crates import further down the stack than their allowed layer:
   - `theo-governance` must not depend on `theo-engine-graph` directly — instead, the graph traversal should be abstracted behind a domain trait
   - `theo-agent-runtime` must not depend on `theo-infra-llm` or `theo-infra-auth` — these should be injected via traits defined in `theo-domain`
   - `apps/theo-cli` must not import engines, governance, or infra directly — all access must flow through `theo-application`
   - `apps/theo-desktop` must not import `theo-infra-*` directly

### Priority 2 — Short-term (Security & Maintenance)

2. **Migrate bincode 1.x to bincode 3.x.** RUSTSEC-2025-0141. The crate is unmaintained and serialization code for the code graph (`theo-engine-graph`) is a long-lived storage format. A dead serialization library is a security and compatibility risk.

3. **Replace serde_yaml in `theo-engine-parser/src/workspace/detect.rs`.** The crate is self-deprecated. The single usage (parsing pnpm workspace YAML) can be replaced with `serde-saphyr` (the community successor) or with a minimal manual parser since the YAML structure is trivial.

4. **Remove `grep-regex` from workspace dependencies.** It is declared but never used. This creates false documentation about project dependencies.

### Priority 3 — Medium-term (Hygiene)

5. **Normalize all internal crate references to `.workspace = true`.** Currently mixed between `{ path = "..." }` and `.workspace = true`. Use workspace references consistently.

6. **Fix `theo-engine-parser` toml reference** to use `toml.workspace = true` instead of `toml = "0.8"`.

7. **Evaluate fastembed v5 migration.** The major version jump likely brings API changes. Running on v4 means missing fixes and improvements including the ort RC version bumps.

8. **Add `deny.toml` (cargo-deny) to the repository.** This would enforce: license allowlist, duplicate version detection, and advisory blocking as part of CI. Currently there is no automated dependency policy enforcement.

### Priority 4 — Long-term (Architecture)

9. **The GTK3 cluster (14 audit warnings) is a Tauri/wry upstream issue.** The Tauri project is actively migrating to GTK4 on Linux. Monitor Tauri release notes. No action needed now — these are transitive warnings only.

10. **Monitor ort stability.** The ONNX Runtime Rust bindings (`ort`) are still in RC as of this analysis. Before shipping production inference workloads, verify a stable 2.0.0 has been released.

