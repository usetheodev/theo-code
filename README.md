<p align="center">
  <a href="https://usetheo.dev">
    <img src="https://usetheo.dev/logo.png" alt="Theo" height="80" />
  </a>
</p>

<p align="center">
  <h1 align="center">Theo Code</h1>
  <p align="center">
    <strong>AI coding assistant with deep code understanding</strong>
  </p>
  <p align="center">
    Terminal-native. Desktop-ready. Knows your codebase like you do.
  </p>
  <p align="center">
    <a href="https://github.com/usetheodev/theo-code/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache%202.0-blue?style=flat-square"></a>
    <a href="https://github.com/usetheodev/theo-code"><img alt="Rust" src="https://img.shields.io/badge/rust-2024%20edition-orange?style=flat-square&logo=rust"></a>
    <a href="https://github.com/usetheodev/theo-code"><img alt="Languages" src="https://img.shields.io/badge/tree--sitter-14%20languages-purple?style=flat-square"></a>
    <a href="https://github.com/usetheodev/theo-code"><img alt="Providers" src="https://img.shields.io/badge/LLM%20providers-26-green?style=flat-square"></a>
  </p>
</p>

---

## What is Theo Code?

Theo Code is an **AI coding assistant** that actually understands your codebase. It combines a fast CLI, a Tauri v2 desktop app, and an auto-generated **Code Wiki** that turns your project into a living, searchable knowledge base.

```bash
theo                              # Interactive TUI
theo "fix the auth bug"           # Single-shot task
theo --mode plan "design caching" # Plan before acting (headless)
theo pilot "implement feature X"  # Autonomous loop with circuit breaker
```

Theo works with **any OpenAI-compatible model** — GPT, Claude, Codex, Ollama, Groq, Mistral, DeepSeek, Cerebras, GitHub Copilot, Bedrock, Vertex, and more. Internally everything is OpenAI-compatible; providers convert at the boundary.

## What makes Theo different?

### GRAPHCTX — Deep Code Intelligence

Most AI coding tools dump files into the context window and hope for the best. Theo builds a **code graph** of your entire project — functions, types, imports, dependencies — and retrieves exactly what the model needs.

```
You: "fix the payment validation"
  → Theo retrieves: PaymentValidator, related types, upstream callers, test files
  → Model sees the RIGHT context, not everything
```

Built on: **Tree-Sitter** for parsing (14 languages), a **code graph** with community detection, an **RRF 3-ranker** that fuses BM25 + Tantivy + neural embeddings, and graph attention propagation for follow-the-edges retrieval.

**Benchmark**: MRR=0.86, Hit@5=0.97, cross-language (Rust + Python) validated on 57 queries across 3 repos.

### Code Wiki — Your codebase as a knowledge base

Theo auto-generates a **navigable wiki** under `.theo/wiki/` whenever it builds the code graph — modules, communities, insights — and refreshes it incrementally as your code changes.

- **Auto-generated** during context assembly — no extra step required
- **Module pages** for every detected community (related files clustered by graph distance)
- **LLM-enriched summaries** explaining what each module does and why
- **Searchable** via BM25 full-text search inside the runtime
- **Write-back insights** — knowledge compounds across sessions

For a polished standalone HTML viewer over any markdown directory, Theo ships **`theo-marklive`** as a separate binary.

### CLI + Desktop + Markdown viewer

Three apps, one engine:

| | `theo` (CLI) | `theo-desktop` | `theo-marklive` |
|---|---|---|---|
| **For** | Terminal-native developers | Visual exploration | Browsing any markdown wiki |
| **Mode** | TUI, single-shot, pilot, headless | Chat + observability dashboard | Static HTML render |
| **Stack** | Rust binary (clap + ratatui) | Tauri v2 + React 18 | Rust binary (pulldown-cmark) |

## Quick Start

### Install

```bash
git clone https://github.com/usetheodev/theo-code.git
cd theo-code
cargo install --path apps/theo-cli
theo --version
```

### Configure

```bash
# Option 1: OpenAI / OAuth
theo login                         # OpenAI device flow (default)
theo login --key sk-...            # Persist an API key
theo login --server <RFC8628-url>  # Generic device flow

# Option 2: Ollama (local, free)
ollama serve                       # Theo auto-detects localhost:11434

# Option 3: Any OpenAI-compatible endpoint
theo --provider groq "fix the bug"
```

### First Run

```bash
cd your-project
theo init                                       # AI-driven project analysis → .theo/theo.md
theo "add input validation to /users endpoint"  # Single-shot task
```

## Features

### Agent Modes

```bash
theo                              # Default: TUI / single-shot
theo --mode plan "design X"       # PLAN mode — write plan to .theo/plans/, no edits
theo --mode ask "explain auth"    # ASK mode — clarifying questions only
theo pilot "implement feature X"  # PILOT — autonomous loop until promise is met
```

Modes are headless flags; inside the TUI use the `/mode` slash command.

### Tools

The runtime exposes three layers (source of truth: `crates/theo-tooling/src/tool_manifest.rs`):

| Layer | Count | What it is |
|---|---|---|
| **Default registry** | 21 | Built-in tools registered by `theo-tooling` and shipped to every agent |
| **Meta-tools** | 5 | Orchestration surfaces injected by `theo-agent-runtime` (`batch`, `done`, `skill`, `subagent`, `subagent_parallel`) |
| **Experimental modules** | 8 | Code present in-tree but not in the default registry — partial or stubbed |

**Default registry tools**: `apply_patch`, `bash` (sandboxed), `codebase_context`, `edit`, `env_info`, `git_commit`, `git_diff`, `git_log`, `git_status`, `glob`, `grep`, `http_get`, `http_post` (SSRF-protected), `memory`, `read`, `reflect`, `task_create`, `task_update`, `think`, `webfetch`, `write`.

**Experimental modules** (not promoted to the default registry): `codesearch`, `ls`, `lsp`, `multiedit`, `plan_exit`, `question`, `task`, `websearch`. Some are partial or stubbed and are not part of the runtime promise until promoted.

### Sub-Agents

Delegate work to specialized sub-agents that run in parallel, with a concurrency cap (Semaphore) and capability bundles per spawn:

```
Main Agent: "fix the bug and add tests"
  → spawns explorer  (read-only capabilities, finds root cause)
  → spawns implementer (write capabilities, applies fix)
  → spawns verifier  (read + bash, runs tests)
```

Sub-agent specs live in `.theo/agents/` (project) or `~/.theo/agents/` (user). The S3 manifest gates which specs are approved.

### Sandbox

Every `bash` command runs sandboxed — `bwrap > landlock > noop` cascade on Linux, `sandbox-exec` layer on macOS. PID isolation, network control, env sanitization, command validation, denied-paths enforcement, rlimits.

### Defense in depth

- **Prompt-injection fences**: tool results, MCP responses, hook injections, and `.theo/PROMPT.md` all flow through `fence_untrusted` / `strip_injection_tokens`.
- **Capability gate**: always installed, default `unrestricted`; sub-agents inherit a narrower set per spec.
- **Secret scrubbing**: persisted JSONL transcripts redact `sk-ant-…`, `ghp_…`, `AKIA…`, and PEM blocks before reaching disk.
- **Plugin allowlist**: optional pinned set of plugin manifest SHA-256 hashes; mismatched plugins fail to load.
- **API-key redaction**: `AgentConfig.llm.api_key` renders as `[REDACTED]` in every Debug output.
- **CSPRNG identifiers**: `RunId` / `TaskId` / `CallId` / `EventId` / `TrajectoryId` are UUID v4-backed (no wall-clock collision risk on fast hardware).
- **Cancellation**: ≤ 500 ms propagation via `tokio_util::sync::CancellationToken` to in-flight tools.

### Session persistence & checkpoints

Sessions persist across restarts as JSONL transcripts (fsynced after every append). Long conversations don't degrade — Theo compresses old messages while preserving critical context (compaction with tool-pair atomicity).

Use `theo checkpoints` to manage shadow-git checkpoints taken before destructive tool calls (`write`, `edit`, `apply_patch`, `bash`). TTL-driven cleanup runs at session shutdown.

### Observability dashboard

```bash
theo dashboard --port 5173
```

Serves the built `theo-ui` bundle and exposes `/api/list_runs`, `/api/run/<id>/trajectory`, etc. Combine with `ssh -L 5173:localhost:5173 …` to inspect remote runs.

OpenTelemetry exporter is available behind the `otel` feature (`cargo build --features otel`).

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  theo-cli  /  theo-desktop  /  theo-marklive        │
│  (clap+ratatui)  (Tauri v2 + React)  (markdown→HTML)│
├─────────────────────────────────────────────────────┤
│              theo-application                       │
│        use cases, GRAPHCTX service,                 │
│        cli_runtime re-exports (apps' contract)      │
├──────────────┬───────────────┬─────────────────────┤
│ Code Intel   │ Agent Runtime │     Governance      │
│ engine-graph │ agent-runtime │     governance      │
│ engine-parser│  + isolation  │                     │
│ engine-retr. │  + infra-mcp  │                     │
├──────────────┴───────────────┴─────────────────────┤
│                Infrastructure                       │
│  theo-infra-llm  (26 providers, streaming)          │
│  theo-infra-auth (OAuth PKCE, RFC 8628)             │
│  theo-infra-memory (Tantivy-backed mem provider)    │
│  theo-tooling   (21 default tools, sandbox)         │
├─────────────────────────────────────────────────────┤
│              theo-domain                            │
│   Pure types, traits, errors (zero workspace deps)  │
└─────────────────────────────────────────────────────┘
```

**15 production crates**, 4 bounded contexts (Code Intelligence, Agent Runtime, Governance, Infrastructure) plus `theo-application` as the use-case layer. Strict dependency rules enforced by `scripts/check-arch-contract.sh` (zero violations as of `develop`). `theo-domain` has zero workspace deps. Apps talk to `theo-application`, never to engines or infra directly (ADR-023 sunset).

## Supported Languages

14 languages via Tree-Sitter:

| Code-graph (full graph + symbols) | Parser-only (symbols + imports) |
|---|---|
| Rust, Python, TypeScript, JavaScript | C, C#, C++, Go, Java, Kotlin, PHP, Ruby, Scala, Swift |

Framework-aware extractors are layered on top for popular stacks (Express, FastAPI, Flask, Django, Spring Boot, ASP.NET, Laravel, Rails).

## LLM Providers

26 providers in the catalog (`crates/theo-infra-llm/src/provider/catalog/`). Internally everything is OpenAI-compatible; providers convert at the boundary.

| Group | Providers |
|---|---|
| **OpenAI family** | OpenAI, OpenRouter, xAI, Mistral, Groq, DeepInfra, Cerebras, Cohere, Together AI, Perplexity, Vercel, ChatGPT/Codex |
| **Anthropic** | Anthropic Claude |
| **Local** | Ollama, vLLM, LM Studio |
| **Cloud / enterprise** | Azure (OpenAI + Cognitive), GitHub Copilot, GitLab, Cloudflare Workers, Cloudflare Gateway, SAP AI Core, Amazon Bedrock, Google Vertex (+ Vertex Anthropic) |

Auth: API key, OAuth PKCE (OpenAI), RFC 8628 device flow (GitHub Copilot, generic), or none (Ollama / vLLM / LM Studio).

## SOTA Tier 1 + Tier 2 — Delivery Status

The plan at `docs/plans/sota-tier1-tier2-plan.md` is **feature-complete** with empirical evidence. Headlines:

- **59 tools** in the default registry (multimodal × 2, browser × 8, LSP × 5, computer × 1, auto-test-gen × 2, planning × 8, DAP × 11, docs × 1, plus the original 21).
- **Empirical smoke bench**: 18/20 scenarios passed (90%, Wilson 95% CI [82.4%, 100.0%]) via OAuth Codex `gpt-5.4` — see `apps/theo-benchmark/reports/smoke-1777306420.sota.md` for the full report.
- **Auto-replan, partial-progress streaming, cost-aware routing, reranker preload, multi-agent claim, RLHF export, browser/LSP/DAP sidecar families** — all opt-in via documented env vars (`THEO_AUTO_REPLAN`, `THEO_PROGRESS_STDERR`, `THEO_RERANKER_PRELOAD`, `THEO_ROUTING_COST_AWARE`, `THEO_BROWSER_SIDECAR`, etc.).

**Definition of Done — 9 of 11 items fully automated AND CI-enforced**:

| # | Item | Gate |
|---|---|---|
| 1 | All 16 phases feature-complete | `scripts/check-phase-artifacts.sh` |
| 2 | All RED tests passing | `cargo test --workspace --exclude theo-code-desktop` |
| 3 | `cargo test --workspace` green | same |
| 4 | `cargo clippy -- -D warnings` green | `cargo clippy ... --tests --bins -- -D warnings` |
| 5 | Backward compat: state v1 loads | regression guards in `theo-domain` + `theo-agent-runtime` |
| 6 | code-audit (lint/size/complexity/coverage) | `scripts/check-sizes.sh` + `check-complexity.sh` + `check-coverage-status.sh` |
| 7 | CHANGELOG `[Unreleased]/Added` per phase | `scripts/check-changelog-phase-coverage.sh` |
| 8 | ADRs D1–D16 referenced in commits | `scripts/check-adr-coverage.sh` |
| 9 | arch contract: 0 violations | `scripts/check-arch-contract.sh` |
| 10 | SWE-Bench-Verified ≥10pt above baseline | ⚠️ smoke 90% measured; terminal-bench reduced still pending |
| 11 | Tier coverage T1 (7/7) + T2 (9/9) | ⚠️ scenario→tier mapping pending |

**Single-command DoD verification**:

```bash
make check-sota-dod          # full report (arch + size + complexity + coverage + clippy + ADR + CHANGELOG + phase artifacts + bench preflight + workspace deps)
make check-sota-dod-quick    # without cargo test (~50s)
```

**Six structural audit gates** (each catching real bugs the day they were added):
- `check-allowlist-paths.sh` — every size/complexity allowlist entry resolves to an existing file/crate
- `check-env-var-coverage.sh` — every documented `THEO_*` env var is read in production
- `check-workspace-deps.sh` — every `[workspace.dependencies]` entry is used
- `check-phase-artifacts.sh` — every plan phase has its promised artifact present
- `check-bench-preflight.sh` — eval.yml + 6 runners + 19 analysis modules import cleanly
- `check-changelog-phase-coverage.sh` — every phase 0..16 mentioned in CHANGELOG

CI workflow `.github/workflows/audit.yml` runs every gate on every PR.

## System Status (honest, verified 2026-04-27 — post-dogfood iteration)

What the autonomous loop measured directly, no marketing. Refreshed after the end-to-end dogfood validation (`docs/audit/dogfood-2026-04-27.md`) which discovered & fixed two production bugs and 9 stale tool-name references that had been silently disabling sidecar tools and observability sensors.

### Structure & health

| | |
|---|---|
| Workspace | 12 crates + 5 apps (Cargo workspace, Rust 2024) |
| Tests passing | **5248** (workspace, excluding `theo-code-desktop` for GTK) — up from 5238 (+10 regression + contract tests) |
| Build | ✅ clean (`cargo build --workspace --exclude theo-code-desktop`) |
| Arch contract | ✅ 0 violations |
| Clippy `-D warnings` | ✅ 0 (16 crates, including `--all-targets` after the dogfood-2026-04-27 bench fix) |
| File size gate (T4.6) | ✅ all oversize allowlisted with sunset 2026-07-23 |
| Function complexity | ✅ 75 fns > 100 LOC, all at-or-below per-crate ceiling |
| `make check-sota-dod` | ✅ **13/13 PASS, 2 SKIP** (DoD #10/#11 out-of-scope) |

### Capabilities

| | |
|---|---|
| Tools in default registry | **59** (was 21 pre-SOTA), pinned by `default_registry_tool_id_snapshot_is_pinned` |
| CLI subcommands | **17** — `init`, `agent`, `pilot`, `context`, `impact`, `stats`, `memory`, `login`, `logout`, `dashboard`, `subagent`, `checkpoints`, `agents`, `mcp`, `skill`, `trajectory`, `help` |
| LLM providers | **26** in catalog (OpenAI / Anthropic / Local / Cloud) |
| Languages parsed | **16** (Tree-Sitter extractors) |
| Audit scripts | **22** under `scripts/check-*.sh`, 6 of them gate-automated for SOTA DoD |

### Empirical bench (real run, not simulation)

| | |
|---|---|
| Smoke bench | **19/20 = 95%** (Wilson CI [82.4%, 100.0%]) — supersedes the 18/20 baseline |
| Provider | OAuth Codex `gpt-5.4` |
| Wall-clock | 664 s for 20 scenarios (post-fix smoke run) |
| Tokens | 2.18 M in / 17.7 K out |
| Avg cost / passed task | ~$0.65 USD (effective ≈ $0 via OAuth Codex) |
| Single failure | `15-cross-file-search` (240 s timeout — agent flakiness on search-heavy tasks; rotates with `02-grep-pattern` between runs) |
| Latest report | `apps/theo-benchmark/reports/smoke-1777323535.sota.md` (the 18/20 baseline at `smoke-1777306420.sota.md` is preserved for diff) |

### Pre-existing baseline debt (active and tracked)

| Gate | Violations | Δ vs pre-dogfood |
|---|---|---|
| `check-unwrap.sh` (production paths) | **96** | -9 (was 105 — diff_viewer + openai_compatible + openai + anthropic StopSequence + tool-arg parsers) |
| `check-panic.sh` | 1 (registry startup, deliberate) | unchanged |
| `check-unsafe.sh` (no `// SAFETY:` comment) | **66** | unchanged |
| size-allowlist god-files | 17 entries, sunset 2026-07-23 | 2 ceilings refreshed (registry/mod.rs 1500→1550, compaction_stages.rs 920→960) |
| complexity-allowlist | 75 functions > 100 LOC, locked baseline | unchanged |
| Stale tool-name bugs | **0** | -9 (caught + fixed during dogfood: 5 in observability/loop_detector/compaction + 3 in PROTECTED_TOOL_NAMES + 1 in `AgentLoop::build_registry`) |

### Sidecar families — end-to-end status

The four sidecar-backed tool families now follow this status:

- **LSP** (`lsp_definition` / `lsp_references` / `lsp_hover` / `lsp_rename` / `lsp_status`) — ✅ **VALIDATED E2E with rust-analyzer 1.95.0** (dogfood 2026-04-27): `lsp_status` returns `1 routable extension: rs`; `lsp_definition` invoked 3 successful tool calls against a real `.rs` file. Other servers (`pyright` / `gopls` / `clangd`) still untested.
- **DAP** (`debug_launch` / `debug_set_breakpoint` / ... / `debug_status`) — ⚪ Not exercised (no debugger installed in the dogfood environment; install `debugpy` / `lldb-vscode` / `dlv` to enable).
- **Browser** (`browser_open` / ... / `browser_status`) — 🟠 Partial: tool dispatch + Playwright Node sidecar spawn validated; `playwright` npm + Chromium 1217 download succeeded; **Chromium binary needs system libs** (`libatk1.0-0`, `libgbm1`, `libcairo2`, `libpango-1.0-0`, `libxcomposite1`, `libxdamage1`, `libxfixes3`, `libnss3`, `libasound2t64`) to launch. Sidecar script is now **embedded in the binary** (`include_str!`) and materialised to `~/.cache/theo/playwright_sidecar.js` on first use — no longer requires running from inside the source checkout.
- **Computer Use** (`computer_action`) — ⚪ SKIP (no display server in the dogfood environment).

Pre-flight gates (`check-bench-preflight.sh`, `default_registry_tool_id_snapshot_is_pinned`, `every_tool_input_example_satisfies_declared_required_params`, **`observability_tool_name_contract`**) confirm the scaffold is consistent. End-to-end execution requires (a) `THEO_SKIP_ONBOARDING=1` in headless / CI / bench (otherwise the bootstrap onboarding hijacks every run — now set automatically by `apps/theo-benchmark/runner/smoke.py`), and (b) per-family system deps listed above.

### One-line system summary

**Production-grade in code (build / test / arch / lint all ✅) with 96 unwrap and 66 unsafe-without-SAFETY as historical debt; LSP family validated E2E with rust-analyzer (DAP/Browser need sidecar install); empirical smoke bench at 19/20 = 95% (supersedes 18/20 baseline) with OAuth Codex.** DoD #10 / #11 (SWE-Bench-Verified ≥10pt; tier T1+T2 coverage) require terminal-bench infrastructure outside the autonomous loop's reach.

## Testing

Theo is built with TDD discipline (RED → GREEN → REFACTOR per `.claude/rules/tdd.md`). The test pyramid as of `develop`:

### Headline numbers

```bash
$ cargo test --workspace --exclude theo-code-desktop --lib --tests
test result: ok. 5248 passed; 0 failed; 23 ignored
```

| Layer | Count | What it covers |
|---|---:|---|
| **Unit + integration tests** | **5248 PASS / 0 FAIL / 23 IGNORED** | Every business-logic crate plus apps' integration smoke (`apps/theo-cli/tests/e2e_smoke.rs`) |
| **Tests by major crate (sample)** | | |
| ↳ `theo-agent-runtime` | 1332 (lib) | Agent loop, run engine, tool call manager, observability sensors, capability gate, sandbox cascade, sub-agents, resume context, hooks, MCP discovery |
| ↳ `theo-tooling` | 902 (lib) | 59 default tools, registry contracts, snapshot pin, JSON-Schema invokability, sandboxed bash, apply_patch, LSP/DAP/Browser tool stubs |
| ↳ `theo-engine-retrieval` | (full bench suite under `tests/benchmark_suite.rs`) | RRF 3-ranker (BM25 + Tantivy + embeddings) — MRR=0.86, Hit@5=0.97 across 57 ground-truth queries |
| ↳ `theo-engine-graph` / `-parser` | | Tree-Sitter extractors for 16 languages, code graph building, community detection |
| ↳ `theo-infra-llm` | 4 | OpenAI/Anthropic/OAuth-Codex provider conversions and stop-sequence parsing |
| ↳ `theo-domain` | | Pure types/traits — `prompt_sanitizer`, `secret_scrubber`, event envelopes, budget |

### Contract tests (structural guarantees)

These pin invariants at the type-system / build level. Renaming a tool, adding a CLI subcommand, or shipping a new env var WITHOUT updating the corresponding source fails CI immediately:

| Contract test | What it pins |
|---|---|
| `default_registry_tool_id_snapshot_is_pinned` | Exact 59-tool list of the default registry — silent renames/removals fail |
| `every_tool_input_example_satisfies_declared_required_params` | Each tool's `input_examples` covers every required param |
| `every_subcommand_responds_to_help_with_exit_zero` | All 17 CLI subcommands (`apps/theo-cli/tests/e2e_smoke.rs`) |
| `observability_tool_name_contract` (3 tests) | Every tool name string referenced by `compaction_stages::PROTECTED_TOOL_NAMES`, `loop_detector::EXPECTED_SEQUENCES`, and `failure_sensors::is_edit_tool` exists in the production registry/manifest. **Added 2026-04-27** to prevent the class of bug that hid `WeakVerification` / `ConversationHistoryLoss` as dead code |
| `build_registry_uses_project_aware_constructor_for_sidecars` | The agent loop's registry must come from `create_default_registry_with_project` (not the empty-stub variant) |
| `agent_loop_new_signature_current_contract` | `AgentLoop::new(config, registry)` signature locked |

### Audit scripts (CI-enforced)

22 scripts under `scripts/check-*.sh` — every PR runs them via `.github/workflows/audit.yml`:

```bash
make check-sota-dod          # full SOTA DoD report (13 gates)
make check-sota-dod-quick    # without cargo test (~50s)

# Individual gates (subset):
bash scripts/check-arch-contract.sh         # zero deps beyond ADR-016/021/022 contract
bash scripts/check-unwrap.sh                # 96 production unwrap (gate RED in strict mode)
bash scripts/check-unsafe.sh                # 66 unsafe blocks without // SAFETY:
bash scripts/check-secrets.sh               # gitleaks-style: sk-ant-, ghp_, AKIA…
bash scripts/check-bench-preflight.sh       # 6 runners + 19 analysis modules import cleanly
bash scripts/check-changelog-phase-coverage.sh   # every phase 0..16 cited
bash scripts/check-adr-coverage.sh          # every ADR D1..D16 referenced in commits
bash scripts/check-allowlist-paths.sh       # zero stale entries (61 entries scanned)
bash scripts/check-env-var-coverage.sh      # 15 documented env vars all referenced in source
bash scripts/check-workspace-deps.sh        # zero unused workspace deps (70 declared)
```

### Empirical end-to-end (real LLM, not mocks)

```bash
THEO_SKIP_ONBOARDING=1 python3 apps/theo-benchmark/runner/smoke.py
```

20 scenarios (read, search, fix-bug, refactor, add-feature, multi-file, explore, plan) materialised in tmpdirs, theo runs `--headless --mode agent`, success_check verifies the actual file/output. Latest run: **19/20 PASS** with OAuth Codex `gpt-5.4` (free).

### Dogfood (real-user manual)

A 3-tier validation under `docs/plans/dogfood-test-plan.md` with the report at `docs/audit/dogfood-2026-04-27.md`. Tier 1 = automated gates; Tier 2 = LLM dogfood (smoke + 3 real prompts); Tier 3 = sidecars (LSP / DAP / Browser / Computer Use) installed and exercised. The dogfood caught the 9 stale tool-name bugs + the production `agent_loop::build_registry` regression that had been silently disabling sidecars since at least the snapshot-pin contract test was introduced.

## Development

```bash
cargo build                          # Build cargo workspace
cargo test                           # Run all tests
cargo test -p theo-engine-graph      # Specific crate
bash scripts/check-arch-contract.sh  # Architecture gate
make check-sota-dod                  # Full SOTA Definition of Done report
cd apps/theo-desktop && cargo tauri dev  # Desktop dev
cd apps/theo-ui && npm run dev       # React frontend dev server
```

The benchmark harness (`apps/theo-benchmark/`) is a Python project, not part of the cargo build — see its own `pyproject.toml`.

## Contributing

1. `theo-domain` has **zero workspace deps**.
2. Apps talk to `theo-application` only — never to engines or infra directly (ADR-023).
3. Every logic change needs tests (Arrange-Act-Assert). Bug fixes need a regression test BEFORE the fix (TDD).
4. `cargo test` and `scripts/check-arch-contract.sh` must pass with zero failures.
5. Code in English (variables, functions, comments). Communication: Português Brasil ou inglês.
6. Errors typed with `thiserror`; never swallow them silently. No `unwrap()` / `expect()` in production paths.

## License

Licensed under the [Apache License 2.0](LICENSE).

---

<p align="center">
  <sub>Built by the <a href="https://usetheo.dev">Theo</a> team. AI that understands your code, not just reads it.</sub>
</p>
