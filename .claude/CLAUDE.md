# Theo Code

AI coding assistant in Rust. Ships as a CLI binary (`theo`), a Tauri v2 desktop app, and a standalone markdown-wiki viewer (`theo-marklive`). Built around two differentiators: **GRAPHCTX** (structural code intelligence — code graph + Tree-Sitter parser + RRF retrieval) and an auto-generated **Code Wiki** under `.theo/wiki/`.

## Workspace

Cargo workspace, Rust 2024 edition, license **Apache 2.0** (per `LICENSE`). Resolver = 3.

```
crates/
  theo-domain                  # Pure types, traits, errors. ZERO workspace deps.
  theo-engine-graph            # Code graph (Tree-Sitter: rust, python, ts, js)
  theo-engine-parser           # AST parser + symbol extraction
                               #   (rust, python, ts, js, c, c#, c++, go, java,
                               #    kotlin, php, ruby, scala, swift)
  theo-engine-retrieval        # RRF 3-ranker (BM25 + Tantivy + embeddings)
  theo-governance              # Policy engine
  theo-isolation               # Sandbox primitives (bwrap, landlock, sandbox-exec)
  theo-infra-mcp               # MCP discovery + dispatcher
  theo-infra-llm               # 26 LLM providers (OA-compatible internally)
  theo-infra-auth              # OAuth PKCE, RFC 8628 device flow, token store
  theo-infra-memory            # Memory provider impls (Tantivy-backed)
  theo-tooling                 # 21 default tools + sandbox executor
  theo-agent-runtime           # Agent loop, state machine, sub-agents, hooks
  theo-api-contracts           # DTOs and event types
  theo-application             # Use cases + cli_runtime re-exports (apps' API)
  theo-test-memory-fixtures    # Shared memory fixtures for cross-crate tests

apps/
  theo-cli                     # `theo` binary — CLI + interactive TUI
  theo-desktop                 # Tauri v2 (Rust backend + React frontend)
  theo-marklive                # Standalone markdown→HTML wiki viewer
  theo-ui                      # React 18 + TS + Tailwind + Radix (frontend bundle)
  theo-benchmark               # Python benchmarking harness (NOT in cargo workspace)
```

Apps in the cargo workspace: `theo-cli`, `theo-desktop`, `theo-marklive`. `theo-ui` is a React project; `theo-benchmark` is a Python harness — both isolated from the cargo build graph.

## Build & Test

```bash
cargo build                          # Build cargo workspace
cargo test                           # Run all tests
cargo test -p theo-agent-runtime     # Specific crate
cd apps/theo-desktop && cargo tauri dev  # Desktop dev
cd apps/theo-ui && npm run dev       # React frontend dev server
bash scripts/check-arch-contract.sh  # Architecture gate (zero deps allowed beyond contract)
```

## Dependency Contract (INVIOLABLE)

Each line is the **upper bound** of workspace deps. Enforced by `scripts/check-arch-contract.sh`.

```
theo-domain                → (nothing)
theo-engine-graph          → theo-domain
theo-engine-parser         → theo-domain
theo-engine-retrieval      → theo-domain, theo-engine-graph, theo-engine-parser
theo-governance            → theo-domain
theo-isolation             → theo-domain
theo-infra-mcp             → theo-domain
theo-infra-llm             → theo-domain
theo-infra-auth            → theo-domain
theo-infra-memory          → theo-domain, theo-engine-retrieval
theo-tooling               → theo-domain
theo-agent-runtime         → theo-domain, theo-governance,
                              theo-infra-llm, theo-infra-auth, theo-tooling,
                              theo-isolation, theo-infra-mcp           (ADR-016/021/022)
theo-api-contracts         → theo-domain
theo-application           → all crates above
theo-test-memory-fixtures  → theo-domain, theo-infra-memory
apps/*                     → theo-application, theo-api-contracts, theo-domain
```

ADR-023 (sunset) confirms apps no longer depend on `theo-agent-runtime` directly — they consume it through `theo_application::cli_runtime` re-exports.

## Conventions

- **Code language**: English (variables, functions, types, technical comments).
- **Communication**: Português Brasil para troca com o usuário.
- **Rust edition**: 2024.
- **TDD obrigatório**: RED → GREEN → REFACTOR. Teste primeiro, código depois. Sem exceções.
- **Tests**: Arrange-Act-Assert. Required for business logic. Bug fixes need a regression test BEFORE the fix.
- **Errors**: Typed with `thiserror` in libraries. `anyhow` only in binaries. Never swallow errors silently.
- **Workspace deps**: Declared once in root `Cargo.toml` `[workspace.dependencies]`; crates use `dep.workspace = true`.
- **Shared types**: Import from `theo-domain`.
- **LLM protocol**: Everything OpenAI-compatible internally; providers convert at the boundary.
- **No `unwrap()` / `expect()` in production paths**: use `?` or typed errors.
- **No backwards-compat hacks** unless the user explicitly asks for them.

## Key Invariants

- `theo-domain` NEVER depends on other workspace crates.
- Apps NEVER import `theo-agent-runtime` / engines / infra directly — go through `theo-application`.
- Circular dependencies are forbidden.
- Sandbox is mandatory for `bash`: `bwrap > landlock > noop` cascade.
- Every tool declares `schema()` + `category()` and is registered in `crates/theo-tooling/src/tool_manifest.rs` (source of truth for exposure: `DefaultRegistry`, `MetaTool`, `ExperimentalModule`, `InternalModule`).
- Tool counts at `tool_manifest.rs`: **59 default registry** (post-SOTA Tier 1 + Tier 2), 5 meta-tools, 8 experimental, 2 internal. The exact 59-tool list is pinned by `default_registry_tool_id_snapshot_is_pinned` — silent renames/removals fail the test.
- `AgentConfig` is organized as 7 owned nested sub-configs (`llm`, `loop_cfg`, `context`, `memory`, `routing`, `evolution`, `plugin`) plus 1 leaf field (`checkpoint_ttl_seconds`). Each sub-config ≤10 fields (T3.2 / T4.1).
- `AgentRunEngine` is decomposed into 5 owned contexts (`subagent`, `observability`, `tracking`, `runtime`, `llm`) — no god-object (T3.1).
- Secrets passing through `state_manager::append_message` are scrubbed of: `sk-ant-…`, `ghp_…`, `AKIA…`, and PEM blocks (T4.5 — see `secret_scrubber.rs`).
- Identifiers are CSPRNG-backed via `Uuid::new_v4()` (T4.6 — see `theo_domain::random_u64`).
- Untrusted strings (tool results, MCP results, hook injections, `.theo/PROMPT.md`) flow through `theo_domain::prompt_sanitizer::fence_untrusted` / `strip_injection_tokens` (T2.1/2.2/2.4/2.5).
- `CapabilityGate` is always installed; default capability set is `unrestricted` (T2.3).
- Benchmark/research code (`apps/theo-benchmark`, `research/`) stays isolated from production runtime.
- **SOTA Tier 1 + Tier 2 plan delivered** — `docs/plans/sota-tier1-tier2-plan.md` is feature-complete with empirical evidence. 9 of 11 Global DoD items are fully automated AND CI-enforced via `make check-sota-dod`; the 2 remaining (#10 SWE-Bench-Verified, #11 tier T1+T2 coverage) need terminal-bench infrastructure beyond the autonomous loop. **Smoke baseline locked at 18/20 (90%)** via OAuth Codex `gpt-5.4` — report under `apps/theo-benchmark/reports/smoke-1777306420.sota.md`.
- **CONTENT/STRUCTURAL audit pattern installed across 6 surfaces** — every claimed artifact must have BOTH a CONTENT audit (artifact exists?) AND a STRUCTURAL audit (artifact actually invokable / resolvable?). Surfaces gated:
  1. CLI subcommands — `apps/theo-cli/tests/e2e_smoke.rs::every_subcommand_responds_to_help_with_exit_zero`
  2. Tool JSON Schemas — `every_tool_input_example_satisfies_declared_required_params` (theo-tooling)
  3. Allowlist files — `scripts/check-allowlist-paths.sh`
  4. Env vars — `scripts/check-env-var-coverage.sh`
  5. Workspace deps — `scripts/check-workspace-deps.sh`
  6. Library functions — `cargo test --workspace`
- Six SOTA-DoD gate scripts ship under `scripts/check-*.sh` — each has a self-test in `scripts/check-sota-dod.test.sh` (39 assertions) and is wired into both `make check-sota-dod` and `.github/workflows/audit.yml`.

## Honest System State (verified 2026-04-27, refreshed post-dogfood-iter2)

What the code actually delivers vs. what is still debt. Refresh this section when running `make check-sota-dod` produces a different result.

### Hard numbers
- 12 crates + 5 apps in the workspace; **5248 tests passing / 0 FAIL / 23 IGNORED** under `cargo test --workspace --exclude theo-code-desktop --lib --tests` (was 5238 pre-dogfood; +10 = 7 regression tests for stale-tool-name fixes + 3 contract tests). `theo-agent-runtime --lib`: 1332/1332. `theo-tooling --lib`: 902/902.
- **59 tools** in the default registry; **17 CLI subcommands** (`init`, `agent`, `pilot`, `context`, `impact`, `stats`, `memory`, `login`, `logout`, `dashboard`, `subagent`, `checkpoints`, `agents`, `mcp`, `skill`, `trajectory`, `help`); **26 LLM providers** in catalog; **16 languages** in the Tree-Sitter extractor set; **22 audit scripts** under `scripts/`.
- **Empirical bench**: 18 of 20 (90 %) at baseline `b7fb694`; after the `agent_loop::build_registry` fix + `apps/theo-benchmark/runner/smoke.py` setting `THEO_SKIP_ONBOARDING=1` by default, the smoke pass rate rose to **19 of 20 (95 %, CI Wilson [82.4 %, 100 %])** via OAuth Codex `gpt-5.4`. Single remaining failure: `15-cross-file-search` (240 s timeout — agent flakiness on search-heavy tasks; rotates with `02-grep-pattern` between runs). Avg cost ~$0 / run via OAuth Codex. Latest report: `apps/theo-benchmark/reports/smoke-1777323535.sota.md`.
- **`make check-sota-dod`**: 13/13 gate-able items PASS, 2 SKIP (DoD #10/#11 require terminal-bench infrastructure outside the autonomous loop's reach).

### Pre-existing baseline debt (active, tracked, partially paid this iteration)
- `scripts/check-unwrap.sh` reports **96 unwrap / expect** in production paths (was 105 pre-dogfood; -9 paid in `diff_viewer.rs` + `openai_compatible.rs` + `openai.rs` + `anthropic.rs` StopSequence/tool-arg parsing). Gate is RED in strict mode.
- `scripts/check-panic.sh` reports **1 panic** — the deliberate `panic!("Built-in tool '{id}' has invalid schema: {e}")` startup assertion in `crates/theo-tooling/src/registry/mod.rs`.
- `scripts/check-unsafe.sh` reports **66 unsafe blocks without `// SAFETY:` comments** (mostly env-var mutation in tests + FFI in graph_context_service / observability) — unchanged this iteration.
- 17 god-files in `.claude/rules/size-allowlist.txt` with sunset 2026-07-23 — 2 ceilings refreshed during dogfood (`registry/mod.rs` 1500→1550 for the embedded Playwright sidecar; `compaction_stages.rs` 920→960 for doc-comment + 2 new entries).
- 75 functions over 100 LOC across 8 crates locked in `.claude/rules/complexity-allowlist.txt` baseline — unchanged.
- **Stale tool-name bugs**: 0 (was ≥ 9 silently active pre-dogfood; ALL caught and fixed via the dogfood sweep + `observability_tool_name_contract` integration test that prevents regression).

### What was NOT validated end-to-end
The four sidecar-backed tool families register and return typed errors gracefully when their sidecar is absent. After dogfood validation 2026-04-27 (see `docs/audit/dogfood-2026-04-27.md`):
- **LSP** (`lsp_*` family) — ✅ **VALIDATED E2E with rust-analyzer 1.95.0** (`lsp_status` returns `1 routable extension: rs`; `lsp_definition` invoked 3 successful calls against a real `.rs` file). Other servers (`pyright` / `gopls` / `clangd` / `typescript-language-server`) still untested. **The fix that unlocked LSP is the same one that unlocks DAP/Browser when their sidecars are installed** — see CHANGELOG `[Unreleased] / Fixed` for `AgentLoop::build_registry`.
- **DAP** (`debug_*` family) — ⚪ NOT validated (no debugger installed; system has no `pip3` / `pipx` to install `debugpy`).
- **Browser** (`browser_*` family) — 🟠 PARTIAL: tool dispatch + Playwright sidecar spawn validated; `playwright` npm + Chromium 1217 downloaded (112 MiB); **Chromium binary missing system libs** (`libatk1.0-0`, `libgbm1`, `libcairo2`, `libpango-1.0-0`, `libxcomposite1`, `libxdamage1`, `libxfixes3`, `libnss3`, `libasound2t64`). Sidecar resolution **only works inside the theo source checkout** — external projects need `THEO_BROWSER_SIDECAR` env or a copy at `<project>/.theo/playwright_sidecar.js` (finding F3 of dogfood report).
- **Computer Use** (`computer_action`) — ⚪ SKIP (`$DISPLAY` empty; headless server).

Pre-flight gates (`scripts/check-bench-preflight.sh`, `default_registry_tool_id_snapshot_is_pinned`, `every_tool_input_example_satisfies_declared_required_params`) confirm the scaffold is consistent. End-to-end execution requires (a) `THEO_SKIP_ONBOARDING=1` in headless / CI / bench context — otherwise the bootstrap onboarding hijacks every run (dogfood finding F1) — and (b) the per-family system deps listed above. Re-run `python3 apps/theo-benchmark/runner/smoke.py` after seeding `USER.md` or setting the env var.

### One-line honest summary
Production-grade in code (build / test / arch / lint all ✅) with **96 unwrap** and 66 unsafe-without-SAFETY as historical debt (was 105/66 pre-dogfood); **LSP family validated E2E with rust-analyzer 1.95.0** (DAP/Browser still need sidecar install — Browser sidecar now embedded via `include_str!`); smoke bench at **19/20 = 95 %** (supersedes 18/20 baseline) with OAuth Codex; **9 stale tool-name bugs caught + fixed + structurally guarded** by the new `observability_tool_name_contract` integration test; DoD #10/#11 (SWE-Bench-Verified ≥10pt; tier T1+T2 coverage) require terminal-bench infrastructure outside the autonomous loop's reach.

### Test discipline (verified 2026-04-27)
Run `cargo test --workspace --exclude theo-code-desktop --lib --tests` to get the canonical 5248 number. Key contract tests that pin the production surface:

| Test | Pins |
|---|---|
| `default_registry_tool_id_snapshot_is_pinned` (theo-tooling) | Exact 59-tool list; silent rename/removal fails |
| `every_tool_input_example_satisfies_declared_required_params` | Each tool's `input_examples` covers required params |
| `every_subcommand_responds_to_help_with_exit_zero` | All 17 CLI subcommands |
| `observability_tool_name_contract` (3 tests, theo-agent-runtime) | Cross-checks `compaction_stages::PROTECTED_TOOL_NAMES`, `loop_detector::EXPECTED_SEQUENCES`, `failure_sensors::is_edit_tool` against `create_default_registry_with_project ∪ TOOL_MANIFEST`. Renames must update both sides in lockstep |
| `build_registry_uses_project_aware_constructor_for_sidecars` | The agent loop's runtime registry must come from `create_default_registry_with_project` (catches the empty-stub regression that disabled all sidecars) |
| `list_returns_empty_vec_when_no_snapshots_yet` | `theo checkpoints list` on empty shadow repo prints the canonical "No checkpoints" message instead of leaking a git-internal error |
| `dogfood_*` family in `failure_sensors.rs::tests` (5 tests) | Production tool IDs (`edit` / `write` / `apply_patch` / `read`) are recognised by every observability sensor |

## Runtime Layout

The agent runtime persists state under `.theo/` in the user's project:

```
.theo/
  agents/         # User-defined agent specs (YAML/MD)
  checkpoints/    # Shadow git repos for restorable file mutations
  config.toml     # Project-level overrides (optional)
  hooks/          # User-defined Pre/Post tool hooks (sandboxed)
  memory/         # Persistent memory (mem files + autodream consolidation)
  metrics/        # Per-run metrics JSON
  plans/          # `theo --mode plan` outputs
  PROMPT.md       # Optional pilot promise (read by `theo pilot` if present)
  state/<run_id>/ # JSONL session transcript (fsynced) + state snapshots
  trajectories/   # JSONL trajectories for the observability dashboard
  wiki/           # Auto-generated Code Wiki (modules, communities, insights)
```

## CLI Commands

The `theo` binary exposes these **17 top-level subcommands** (see `apps/theo-cli/src/main.rs`, pinned by `every_subcommand_responds_to_help_with_exit_zero` integration test):

`init`, `agent`, `pilot`, `context`, `impact`, `stats`, `memory`, `login`, `logout`, `dashboard`, `subagent`, `checkpoints`, `agents`, `mcp`, `skill`, `trajectory`, `help`.

Default behaviour (no subcommand) opens the interactive TUI; passing a prompt runs single-shot. The `--headless` flag emits a single JSON line for benchmark/CI pipelines. Modes: `--mode agent|plan|ask` (headless only — TUI uses the `/mode` slash command). The Code Wiki has no dedicated CLI command; it is generated incrementally by `graph_context_service` whenever the code graph is built.

**Headless contract (CI / bench)**: every `theo --headless` invocation emits a single line of JSON with the schema `theo.headless.v4` to stdout. `apps/theo-benchmark/runner/smoke.py` automatically sets `THEO_SKIP_ONBOARDING=1` so the bootstrap onboarding does not hijack scenario prompts (regression guard from dogfood F1).

## Important Directories

- `docs/current/` — what IS implemented
- `docs/target/` — what is planned
- `docs/adr/` — Architecture Decision Records (ADR-001 through ADR-023+)
- `docs/roadmap/` — product roadmap
- `docs/plans/` — remediation/feature plans (e.g. `agent-runtime-remediation-plan.md`)
- `docs/kanban/` — Kanban boards driving plans
- `docs/audit/` — review outputs
- `research/` — papers, experiments, references (isolated from production)
- `referencias/` — third-party reference repos cloned for study (isolated)
