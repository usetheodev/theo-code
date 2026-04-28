# CLAUDE.md — Theo Code

Instructions for Claude / agents working on this repository. Mirrors the
**verified** state of the system — every number below was re-measured on
**2026-04-28** by re-running the gates listed in [§Honest System State](#honest-system-state).

If a number ever drifts ≥ 5 % from reality, fix the docs *first* — the
project's only honesty contract is that this file does not lie.

---

## What this repo is

Theo Code is an autonomous coding agent written in Rust. It packages four
things into one workspace:

1. **Code intelligence** — Tree-Sitter graph (14 languages) + tantivy text
   index + RRF rank fusion → assembles context windows from large repos.
2. **Agent runtime** — state machine (Plan → Act → Observe → Reflect),
   sub-agent fan-out, budget enforcer, sandboxed tool execution.
3. **Provider abstraction** — 26 LLM provider specs (Anthropic, OpenAI,
   xAI, Mistral, Groq, Cohere, Vertex, Bedrock, Ollama, vLLM, …) sharing a
   single streaming/retry/converter pipeline.
4. **Surfaces** — `theo` CLI (17 subcommands), Tauri desktop, Vite UI, and
   a Python benchmark harness in `apps/theo-benchmark/`.

---

## Read-first project rules

The four files in `.claude/rules/` are **inquebráveis** — they encode
decisions that block CI, not preferences.

| File | What it pins |
|---|---|
| `.claude/rules/architecture.md` | Crate dependency contract (ADR-010). `apps/*` may only depend on `theo-application` + `theo-api-contracts`. `theo-domain` depends on nothing. Enforced by `make check-arch`. |
| `.claude/rules/tdd.md` | RED → GREEN → REFACTOR. Every bug fix needs a regression test *before* the fix. |
| `.claude/rules/testing.md` | AAA pattern, descriptive names, deterministic, independent. Flaky test = P0. |
| `.claude/rules/rust-conventions.md` | `thiserror` per crate, no `unwrap`/`expect` in production paths, `anyhow` only in binaries, newtypes for IDs. |

There are also **8 enforcement allowlists** in `.claude/rules/*.txt` (size,
complexity, unwrap, unsafe, panic, secret, IO-test, architecture-contract).
Each entry has a sunset date. Adding a new entry requires an ADR pointer.

---

## Honest System State

**Verified on 2026-04-28.** Reproduce with the commands in each row.

### Build & test

| Metric | Value | Command |
|---|---|---|
| Workspace builds | ✅ exit 0 | `cargo build --workspace --exclude theo-code-desktop` |
| `cargo test` (no-fail-fast) | **5247 PASS / 0 FAIL / 24 IGNORED** in 69 suites | `cargo test --workspace --exclude theo-code-desktop --lib --tests --no-fail-fast` |
| Quarantined | `test_pre4_ac_2_adr_008_exists_and_signed` is `#[ignore]`d — it expects `docs/adr/008-theo-infra-memory.md`, planned pre-condition for the not-yet-merged agent-memory work (`outputs/agent-memory-plan.md`). Tracked as CLEAN-A4 in `docs/plans/cleanup-2026-04-28.md`. |
| `cargo clippy -D warnings --all-targets` | ✅ 0 warnings (16 crates) | `cargo clippy --workspace --all-targets --no-deps -- -D warnings` |

### Quality gates

| Gate | Status | Notes |
|---|---|---|
| `make check-arch` (T1.5) | ✅ **0 violations** in 16 crates scanned | ADR-010 dependency direction respected |
| `make check-sizes` (T4.6) | ✅ 52 oversize files, **all allowlisted, 0 NEW, 0 EXPIRED** | Sunsets currently 2026-07-23 |
| `make check-sota-dod --quick` | ✅ **12 / 12 PASS, 2 SKIP** | DoD #10 (SWE-Bench) and #11 (T1+T2 tier coverage) skip — both require paid LLM API |
| `make check-unwrap` (strict) | ❌ **85 violations + 36 allowlisted** | Strict gate fails standalone; SOTA DoD does *not* block on this. Each allowlisted site has a sunset. |
| `make check-unsafe` (strict) | ❌ 119 unsafe sites, **66 missing `// SAFETY:`** | Same posture: tracked, allowlisted, not in SOTA DoD blocking set. |
| `make check-secrets` | ✅ 0 leaks (gitleaks fallback) | |

### Capabilities (counted from source, not docs)

| Capability | Count | Source of truth |
|---|---|---|
| Cargo workspace members | **15 lib crates + 3 binary apps** | `Cargo.toml` `[workspace.members]` |
| Crates outside workspace (present in tree) | 1 (`crates/theo-compat-harness`) | Has Cargo.toml but not in workspace.members |
| Apps outside Rust workspace | 2 (`apps/theo-benchmark` Python, `apps/theo-ui` TypeScript/Vite) | No Cargo.toml |
| LLM provider specs | **26** | `crates/theo-infra-llm/src/provider/catalog/*.rs` (`grep -c 'pub const [A-Z_]\+: ProviderSpec'`) |
| Tree-sitter languages | **14** | c, cpp, c-sharp, go, java, javascript, kotlin-ng, php, python, ruby, rust, scala, swift, typescript |
| CLI top-level subcommands | **17** | `theo --help` |
| Distinct tool IDs (production) | **72** | `grep -rE 'fn id\(&self\)' crates/theo-tooling/src` minus `bad`/`invalid` test fakes |
| Audit scripts (`scripts/check-*.sh`) | **22** | CI-enforced via `make audit` |

### Sidecar status (from `docs/audit/maturity-gap-analysis-2026-04-27.md`)

- **LSP ✅ VALIDATED** — E2E with rust-analyzer 1.95.0; the `lsp_*` tools are wired through `LspSessionManager::from_path()` when `create_default_registry_with_project` is used.
- **Browser 🟠 partial** — Playwright sidecar bundled via `include_str!`, but Chromium needs OS libs to actually render. Dispatch path verified.
- **DAP ⚪ untested E2E** — 11 `debug_*` tools registered but no smoke test against `lldb-vscode` / `debugpy` / `dlv` yet. Gap 6.1 (CRITICAL) in maturity analysis.
- **Computer Use ⚪ skip** — `computer_action` tool registered, no platform driver wired.

### Maturity score

`docs/audit/maturity-gap-analysis-2026-04-27.md` reports **3.2 / 5** with a
roadmap to 4.0 (then 5.0). Every gap is enumerated there with severity,
effort, and concrete action — read it before proposing big changes.

---

## Architecture

### Cargo workspace

```
crates/
├── theo-domain                  pure types, state machines, zero deps
├── theo-engine-graph            code graph construction, clustering
├── theo-engine-parser           Tree-Sitter extraction (14 langs)
├── theo-engine-retrieval        BM25 + RRF + context assembly
├── theo-governance              policy engine, sandbox cascade
├── theo-isolation               bwrap / landlock / noop fallback
├── theo-infra-llm               26 provider specs, streaming, retry
├── theo-infra-auth              OAuth PKCE, device flow, env keys
├── theo-infra-mcp               Model Context Protocol client
├── theo-infra-memory            (in-progress; ADR-008 still pending)
├── theo-test-memory-fixtures    fixtures for memory tests
├── theo-tooling                 72 production tools + registry
├── theo-agent-runtime           agent loop, sub-agents, observability
├── theo-api-contracts           serializable DTOs for IPC
└── theo-application             use-cases, facade, CLI runtime re-exports

apps/
├── theo-cli         (pkg name: `theo`)   CLI binary
├── theo-marklive                         markdown live renderer
└── theo-desktop                          Tauri shell (excluded from cargo test — GTK deps)
```

### Dependency direction (from `.claude/rules/architecture.md`, enforced by `check-arch-contract.sh`)

```
theo-domain              → (nothing)
theo-engine-graph        → theo-domain
theo-engine-parser       → theo-domain
theo-engine-retrieval    → theo-domain, theo-engine-graph, theo-engine-parser   (ADR-011)
theo-governance          → theo-domain
theo-infra-*             → theo-domain
theo-tooling             → theo-domain
theo-agent-runtime       → theo-domain, theo-governance,
                           theo-infra-llm, theo-infra-auth, theo-tooling        (ADR-016)
theo-api-contracts       → theo-domain
theo-application         → all crates above
apps/*                   → theo-application, theo-api-contracts                 (ADR-010 / T3.3)
```

**Apps NEVER import engine/infra crates directly.** Lower-layer types
reach apps through `theo_application::cli_runtime::*` re-exports.

---

## Common commands

### Build & test

```bash
make build                         # cargo build --workspace
make test                          # cargo test --workspace
make fmt                           # cargo fmt --all
make lint                          # cargo clippy --workspace --all-targets -- -D warnings

cargo test -p theo-domain          # single crate (leaf — minimizes rebuild)
cargo test --workspace --exclude theo-code-desktop --no-fail-fast  # skip GTK app
```

### Audit / gates

```bash
make audit                         # all 8 audit techniques (composite, may skip on missing tools)
make audit-tools                   # install tarpaulin, cargo-audit, semgrep, gitleaks, etc.
make check-sota-dod                # full SOTA Tier 1+2 DoD report (runs cargo test)
make check-sota-dod-quick          # same without cargo test (~50s)

make check-arch                    # ADR-010 dependency direction
make check-unwrap                  # production unwrap/expect (strict, fails until allowlist resolved)
make check-unsafe                  # // SAFETY: comments on every unsafe block
make check-sizes                   # T4.6 file/function size limits w/ sunsets
make check-secrets                 # gitleaks (or grep fallback)
```

CI workflow `.github/workflows/audit.yml` runs every gate on every PR.

### CLI exploration

```bash
cargo run -p theo --bin theo -- --help              # 17 subcommands
cargo run -p theo --bin theo -- init                # initialize .theo/theo.md
cargo run -p theo --bin theo -- pilot "fix bug X"   # autonomous loop
cargo run -p theo --bin theo -- context --task "..." # GRAPHCTX assembly
cargo run -p theo --bin theo -- memory lint         # memory subsystem
cargo run -p theo --bin theo -- dashboard           # observability HTTP
```

Binary name is **`theo`**, not `theo-cli`. The package in
`apps/theo-cli/Cargo.toml` is `name = "theo"`.

---

## TDD workflow (inquebrável — `.claude/rules/tdd.md`)

```
1. RED      — write the failing test that proves the behavior is missing
2. GREEN    — write the minimum code that makes it pass
3. REFACTOR — clean up while keeping every test green
```

For bug fixes: write the regression test **before** the fix. Confirm it
fails. Apply the fix. Confirm it passes.

Acceptance criteria for "task complete":

```bash
cargo test -p <affected-crate>     # all green
make check-arch                    # 0 violations
make check-sizes                   # 0 NEW / 0 EXPIRED
```

If any of those fail, the task is **not done**.

---

## Pitfalls / things to know

1. **`theo-desktop` is excluded from CI tests** — needs `libgtk-3-dev`/Tauri
   system deps. Use `--exclude theo-code-desktop` in every cargo command.
   Gap 1.2 in the maturity analysis tracks this.

2. **`theo-compat-harness` is NOT in the workspace.** It exists as a
   separate Cargo project pointing at vendored `commands` + `tools`
   directories. Don't try to add it to `[workspace.members]` without
   resolving its external deps first.

3. **`apps/theo-benchmark/` is Python**, not Rust. It runs against the
   `theo` binary (built first by `cargo build`) and exists outside the
   Rust workspace. Smoke runs land in `apps/theo-benchmark/reports/`.

4. **`apps/theo-ui/` is TypeScript/Vite**, also outside the workspace.
   `npm install && npm test` from inside that directory.

5. **The 1 known test failure** (`test_pre4_ac_2_adr_008_exists_and_signed`)
   is a *planned pre-condition* for the agent-memory work
   (`outputs/agent-memory-plan.md`). It expects an ADR at
   `docs/adr/008-theo-infra-memory.md` that hasn't been written yet.
   Don't "fix" it by stubbing the file — write the real ADR or skip the
   test until the memory subsystem ships.

6. **`check-unwrap` and `check-unsafe` strict gates fail standalone.**
   This is intentional: the SOTA DoD references the *allowlist + sunset*
   posture. To resolve a violation, either fix it or add an entry to the
   allowlist with an ADR reference and a sunset date. Don't disable the
   gates.

7. **The default tool registry has fewer tools than the sources expose.**
   `create_default_registry()` hard-codes ~44 tools; the project-aware
   variant `create_default_registry_with_project()` swaps stubs for real
   LSP/browser/docs-search managers. **The headless CLI used to call the
   wrong one — Gap 1.1 (HIGH) in the maturity analysis tracks the missing
   E2E test for that path.**

8. **Allowlist files have sunsets, not amnesty.** Every entry in
   `.claude/rules/*-allowlist.txt` has a date column. `check-*` scripts
   fail if a sunset is in the past. Renewing without progress is a
   hygiene failure.

---

## Where to look

| You want to | Look in |
|---|---|
| Understand a design decision | `docs/adr/D{1..16}.md` (D1–D16) |
| See current plans / phases | `docs/plans/` |
| See what was already audited | `docs/audit/` (latest: `maturity-gap-analysis-2026-04-27.md`) |
| Reference open-source projects we study | `referencias/` (gitignored — clone yourself) |
| See empirical bench results | `apps/theo-benchmark/reports/smoke-*.sota.md` |
| See per-agent navigation map (older) | `.theo/AGENTS.md` (mirror of this file, may drift — this CLAUDE.md is canonical) |
| See the project's rules tree | `.claude/rules/` |

---

## When in doubt

The **95 % rule** from the global rules applies here too: if you don't
have 95 %+ confidence about a change, **stop and ask**. Specifically:

- Adding a new dependency? Run `make check-arch` after wiring it up.
- Touching a public API? Add a contract test in the affected crate's
  `tests/` directory.
- Removing a feature? Confirm no allowlist entry, no ADR, and no test
  references it first (`grep -rn '<feature>' crates/ apps/ docs/`).
- Touching `theo-domain`? Be aware that **everything rebuilds**. Move
  leaf-first to minimize cascade.

When the task says "honest", produce a number you can reproduce. When
the task says "check", run the gate — don't infer the result.
