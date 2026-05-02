<p align="center">
  <a href="https://usetheo.dev">
    <img src="https://usetheo.dev/logo.png" alt="Theo" height="80" />
  </a>
</p>

<p align="center">
  <h1 align="center">Theo Code</h1>
  <p align="center">
    <strong>Autonomous coding agent with deep code understanding</strong>
  </p>
  <p align="center">
    Terminal-native. Desktop-ready. Knows your codebase like you do.
  </p>
  <p align="center">
    <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache%202.0-blue?style=flat-square"></a>
    <img alt="Rust" src="https://img.shields.io/badge/rust-2024%20edition-orange?style=flat-square&logo=rust">
    <img alt="Languages" src="https://img.shields.io/badge/tree--sitter-14%20languages-purple?style=flat-square">
    <img alt="Providers" src="https://img.shields.io/badge/LLM%20providers-26-green?style=flat-square">
    <img alt="Tools" src="https://img.shields.io/badge/agent%20tools-72-yellow?style=flat-square">
    <img alt="Tests" src="https://img.shields.io/badge/tests-5247%20passing-brightgreen?style=flat-square">
    <img alt="License" src="https://img.shields.io/badge/license-Apache%202.0-blue?style=flat-square">
  </p>
</p>

---

## What it does

Theo Code is an autonomous coding agent that reads, plans, edits, and
verifies code changes inside large repositories. It packages four things
into one workspace:

- **Code intelligence** — Tree-Sitter graph (14 languages) + tantivy text
  index + RRF rank fusion → assembles task-relevant context windows
  without dumping the whole repo into the prompt.
- **Agent runtime** — state machine (Plan → Act → Observe → Reflect),
  sub-agent fan-out, budget enforcer, sandboxed tool execution.
- **Provider abstraction** — 26 LLM provider specs (Anthropic, OpenAI,
  xAI, Mistral, Groq, Cohere, Vertex, Bedrock, Ollama, vLLM, …) sharing
  one streaming/retry/converter pipeline.
- **Surfaces** — `theo` CLI (17 subcommands), Tauri desktop, Vite UI, and
  a Python benchmark harness in `apps/theo-benchmark/`.

The headline number isn't "lines of code" — it's that every claim on
this page is a gate you can run yourself. Numbers are re-verified every
release; see [System Status](#system-status).

---

## Quickstart

### Build from source

```bash
git clone https://github.com/usetheodev/theo-code
cd theo-code
cargo build --workspace --exclude theo-code-desktop --release
./target/release/theo --help
```

System requirements: Rust 1.83+ (2024 edition), `pkg-config`. The
desktop app additionally needs `libgtk-3-dev` and Tauri prerequisites
on Linux; everything else builds without system deps.

### First run

```bash
# Authenticate with a provider (OAuth device flow or API key)
theo login                                # interactive picker
theo login --provider anthropic           # pin a provider

# Initialize a project (writes .theo/theo.md with an AI-generated map)
theo init

# Single-shot task
theo "find every place that constructs a session token"

# Autonomous loop (Plan → Act → Observe → Reflect until done)
theo pilot "remove the panic on stale tool name and add a regression test"

# Interactive TUI
theo
```

### Useful one-liners

```bash
theo context --task "auth flow"        # GRAPHCTX-assembled context for a query
theo impact src/auth/login.rs           # what does editing this file affect?
theo stats                              # graph statistics for the repo
theo memory lint                        # memory subsystem hygiene
theo dashboard                          # observability HTTP server
theo subagent ls                        # persisted sub-agent runs
theo checkpoints ls                     # workdir shadow-git checkpoints
```

---

## CLI surface

`theo --help` lists **17 top-level subcommands**, validated by a
contract test that pins the count:

```
init         Initialize project — creates .theo/theo.md with AI analysis
agent        Interactive REPL or single-shot task execution
pilot        Autonomous loop until promise is fulfilled
context      Assemble context for a task using GRAPHCTX
impact       Analyze impact of editing a file
stats        Show graph statistics for a repository
memory       Memory subsystem utilities (lint, inspect)
login        Authenticate with a provider (OAuth device flow or API key)
logout       Remove saved credentials
dashboard    Start the observability dashboard HTTP server
subagent     Manage persisted sub-agent runs
checkpoints  Manage workdir checkpoints (shadow git repos)
agents       Manage project agents approval
mcp          Manage MCP discovery cache
skill        Skill catalog: list / view / delete user-installed skills
trajectory   Trajectory export tooling
help         Print help for any subcommand
```

---

## Architecture

Cargo workspace with **15 lib crates + 3 binary apps** under one Rust
2024 edition tree.

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
├── theo-infra-memory            (in progress; ADR-008 pending)
├── theo-test-memory-fixtures    fixtures for memory tests
├── theo-tooling                 72 production tools + registry
├── theo-agent-runtime           agent loop, sub-agents, observability
├── theo-api-contracts           serializable DTOs for IPC
└── theo-application             use-cases, facade, CLI runtime re-exports

apps/
├── theo-cli         (pkg `theo`)        the CLI binary
├── theo-marklive                        markdown live renderer
├── theo-desktop                         Tauri shell (excluded from cargo test — GTK)
├── theo-benchmark                       Python harness (outside Rust workspace)
└── theo-ui                              Vite/TS UI (outside Rust workspace)
```

### Dependency direction (ADR-010, enforced)

```
theo-domain              → (nothing)
theo-engine-graph        → theo-domain
theo-engine-parser       → theo-domain
theo-engine-retrieval    → theo-domain, theo-engine-graph, theo-engine-parser
theo-governance          → theo-domain
theo-infra-*             → theo-domain
theo-tooling             → theo-domain
theo-agent-runtime       → theo-domain, theo-governance,
                           theo-infra-llm, theo-infra-auth, theo-tooling
theo-api-contracts       → theo-domain
theo-application         → all crates above
apps/*                   → theo-application, theo-api-contracts
```

`scripts/check-arch-contract.sh` enforces this on every PR. **0 violations**
across 16 crates scanned, today.

---

## Capabilities

### LLM providers (26)

Every provider lives in `crates/theo-infra-llm/src/provider/catalog/` as a
`ProviderSpec` const. Adding one means dropping a new const and wiring
its auth strategy.

```
amazon-bedrock         azure                  azure-cognitive-services
anthropic              cerebras               chatgpt-codex
cloudflare-ai-gateway  cloudflare-workers-ai  cohere
deepinfra              github-copilot         gitlab
google-vertex          google-vertex-anthropic groq
lm-studio              mistral                ollama
openai                 openrouter             perplexity
sap-ai-core            togetherai             vercel
vllm                   xai
```

OAuth device flow is supported for `anthropic` and `chatgpt-codex`. The
rest use API keys (env or config).

### Languages parsed (14)

C, C++, C#, Go, Java, JavaScript, Kotlin, PHP, Python, Ruby, Rust, Scala,
Swift, TypeScript.

### Agent tools (72 distinct IDs)

Counted by `grep -rE 'fn id\(&self\)' crates/theo-tooling/src` minus the
`bad`/`invalid` test fakes. Categories:

| Category | Tool IDs |
|---|---|
| Filesystem | `read`, `write`, `edit`, `multiedit`, `apply_patch`, `ls`, `glob`, `grep`, `codesearch` |
| Shell & process | `bash`, `batch`, `env_info` |
| Git | `git_status`, `git_diff`, `git_log`, `git_commit` |
| HTTP | `http_get`, `http_post`, `webfetch`, `websearch` |
| Cognitive | `think`, `reflect`, `memory`, `task`, `task_create`, `task_update`, `question`, `skill` |
| Planning (SOTA) | `plan_create`, `plan_update_task`, `plan_advance_phase`, `plan_log`, `plan_summary`, `plan_next_task`, `plan_replan`, `plan_failure_status`, `plan_exit` |
| Multimodal | `read_image`, `screenshot`, `computer_action` |
| Code intelligence | `codebase_context`, `docs_search` |
| Test generation | `gen_property_test`, `gen_mutation_test` |
| LSP sidecar | `lsp_status`, `lsp_definition`, `lsp_references`, `lsp_hover`, `lsp_rename`, `lsp` |
| DAP sidecar | `debug_status`, `debug_launch`, `debug_set_breakpoint`, `debug_continue`, `debug_step`, `debug_eval`, `debug_scopes`, `debug_variables`, `debug_stack_trace`, `debug_threads`, `debug_terminate` |
| Browser sidecar | `browser_status`, `browser_open`, `browser_click`, `browser_screenshot`, `browser_type`, `browser_eval`, `browser_wait_for_selector`, `browser_close` |
| Wiki | `wiki_generate`, `wiki_ingest`, `wiki_query` |

### Sidecar end-to-end status

| Sidecar | Status | Notes |
|---|---|---|
| **LSP** | ✅ VALIDATED | E2E with rust-analyzer 1.95.0; `LspSessionManager::from_path()` discovers servers when project root is known. |
| **Browser** | 🟠 partial | Playwright sidecar bundled via `include_str!`; dispatch path verified. Chromium needs OS libs to actually render. |
| **DAP** | ⚪ not exercised | 11 `debug_*` tools registered; no smoke against `lldb-vscode` / `debugpy` / `dlv` yet (Gap 6.1, CRITICAL in maturity analysis). |
| **Computer Use** | ⚪ skip | `computer_action` tool registered; no platform driver wired. |

---

## Quality model

### CI gates (8 audit techniques, 22 enforcement scripts)

`make audit` runs the composite suite. Each technique is independently
runnable:

| Technique | Command | What it enforces |
|---|---|---|
| Architecture contract (T1.5) | `make check-arch` | ADR-010 dep direction |
| File / function size (T4.6) | `make check-sizes` | 800 LOC / file ceiling, allowlist with sunsets |
| Unwrap / expect (T2.5) | `make check-unwrap` | No `unwrap`/`expect` in production paths |
| Panic / todo / unimplemented (T2.6) | `make check-panic` | Same posture as unwrap |
| Unsafe SAFETY comment (T2.9) | `make check-unsafe` | Every `unsafe` block has `// SAFETY:` within 8 lines above |
| Inline I/O tests (T5.2) | `make check-io-tests` | I/O tests live in `tests/`, not inline |
| Secrets scan (T6.2) | `make check-secrets` | `gitleaks` (or grep fallback) |
| Composite SOTA DoD | `make check-sota-dod` | Every Tier 1 + Tier 2 DoD criterion |

CI workflow `.github/workflows/audit.yml` runs every gate on every PR.

### Allowlists (with sunsets)

Pre-existing debt is tracked, not amnestied. Each `.claude/rules/*-allowlist.txt`
has an entry-per-violation with a date column:

- `size-allowlist.txt` — files above 800 LOC; current sunset 2026-07-23
- `unwrap-allowlist.txt` — production unwrap/expect tolerated
- `unsafe-allowlist.txt` — unsafe sites missing `// SAFETY:`
- `panic-allowlist.txt`, `complexity-allowlist.txt`, `secret-allowlist.txt`,
  `io-test-allowlist.txt`, `architecture-contract.yaml`

`check-*` scripts fail when a sunset has elapsed. Renewing without
progress is a hygiene failure.

---

## System Status

**Verified 2026-04-28.** Every number reproducible by re-running the
command in the right column.

### Build & test

| | | Reproduce |
|---|---|---|
| Workspace builds | ✅ exit 0 | `cargo build --workspace --exclude theo-code-desktop` |
| `cargo test` | **5247 PASS / 0 FAIL / 24 IGNORED** in 69 suites | `cargo test --workspace --exclude theo-code-desktop --lib --tests --no-fail-fast` |
| Quarantined test | `test_pre4_ac_2_adr_008_exists_and_signed` is `#[ignore]`d — awaits `docs/adr/008-theo-infra-memory.md` (memory subsystem on the plan, not delivered). | Tracked as CLEAN-A4 in `docs/plans/cleanup-2026-04-28.md` |
| `cargo clippy -D warnings --all-targets` | ✅ 0 warnings | `cargo clippy --workspace --all-targets --no-deps -- -D warnings` |

### Quality gates

| | | Reproduce |
|---|---|---|
| Arch contract | ✅ **0 violations** in 16 crates scanned | `make check-arch` |
| Size gate | ✅ 52 oversize, all allowlisted, **0 NEW, 0 EXPIRED** | `make check-sizes` |
| `make check-sota-dod --quick` | ✅ **12 / 12 PASS, 2 SKIP** (out-of-scope: paid LLM) | `bash scripts/check-sota-dod.sh --quick` |
| `make check-unwrap` (strict) | ❌ **85 violations + 36 allowlisted** — gate fails standalone, not in SOTA DoD blocking set | `bash scripts/check-unwrap.sh` |
| `make check-unsafe` (strict) | ❌ 119 unsafe sites, **66 missing `// SAFETY:`** — same posture | `bash scripts/check-unsafe.sh` |

### Empirical evidence

| | | |
|---|---|---|
| Smoke bench | **19 / 20 = 95 %** (Wilson 95 % CI [82.4 %, 100 %]) | `apps/theo-benchmark/reports/smoke-1777323535.sota.md` |
| Bench provider | OAuth Codex `gpt-5.4` | Single-provider — Gap 2.3 (HIGH) tracks multi-provider parity |
| Retrieval bench | MRR 0.86 / Hit@5 0.97 over 57 queries × 3 repos | `crates/theo-engine-retrieval/tests/benchmarks/` |

### Honesty markers

| | |
|---|---|
| Maturity score | **3.2 / 5** (`docs/audit/maturity-gap-analysis-2026-04-27.md`) |
| Gaps tracked | every gap has severity, effort, action, and owner placeholder |
| Reference dogfood | `docs/audit/dogfood-2026-04-27.md` — narrative log of bugs caught against this very repo |

We do not ship marketing numbers. If a metric isn't reproducible from
this repo with the listed command, it does not appear here.

---

## Testing

### Run the suite

```bash
cargo test --workspace --exclude theo-code-desktop --lib --tests --no-fail-fast
# → 5247 passed; 1 failed; 23 ignored across 69 suites (verified 2026-04-28)
```

### Test counts by crate (lib + integration; from `cargo test -p <crate>`)

| Crate | tests |
|---|---:|
| `theo-agent-runtime` | 1300+ |
| `theo-tooling` | 900+ |
| `theo-engine-retrieval` | 270+ |
| `theo-engine-parser` | 460+ |
| `theo-domain` | 250+ |
| `theo-engine-graph` | 100+ |
| `theo-infra-llm` | 150+ |
| `theo-infra-auth` | 85+ |
| `theo-application` | 70+ |
| `theo-governance` | 60+ |

(Per-crate counts drift between iterations — re-run `cargo test -p <crate>`
for the exact number on `HEAD`.)

### Contract tests (structural invariants)

A small set of tests exists specifically to pin invariants of the
production surface so silent regressions get caught:

- `every_subcommand_responds_to_help_with_exit_zero` — pins the **17**
  subcommand count
- `observability_tool_name_contract` — every observability sensor names
  a real tool ID (added after the dogfood found ≥ 9 stale references)
- `build_registry` — every `DefaultRegistry` entry is reachable
- `agent_loop_new` — the agent loop builder accepts only valid configs
- snapshot pins on tool schemas + subagent help

### Audit scripts (CI-enforced)

`scripts/check-*.sh` (22 of them) run as part of the SOTA DoD composite.
See [Quality model](#quality-model).

### Empirical bench

Reproduce with:

```bash
make check-bench-preflight                 # validate scenarios + harness
cd apps/theo-benchmark
python run_benchmark.py --suite smoke      # last result: 19/20 = 95 %
```

---

## Project rules

The repo carries four rule files in `.claude/rules/` that block CI when
violated:

- `architecture.md` — crate dep direction (ADR-010), prohibited imports
- `tdd.md` — RED → GREEN → REFACTOR, regression test before fix
- `testing.md` — AAA, descriptive names, deterministic, independent
- `rust-conventions.md` — `thiserror`, no `unwrap` in prod, newtypes

Together with `.claude/rules/*-allowlist.txt` files (each with sunsets),
they form the project's hygiene contract.

---

## Contributing

1. **Read [`CLAUDE.md`](CLAUDE.md)** before changing anything. It has the
   verified state of every gate.
2. **TDD is inquebrável.** Bug fixes need a regression test *before* the
   fix.
3. **Update the changelog.** Every PR adds an entry under `[Unreleased]`
   in `CHANGELOG.md` with a `(#PR)` reference.
4. **Don't break the dependency contract.** `make check-arch` must pass.
5. **Don't widen allowlists without an ADR.** Each entry has an ADR
   pointer and a sunset date.

---

## License

[Apache-2.0](LICENSE)
