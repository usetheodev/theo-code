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

## Development

```bash
cargo build                          # Build cargo workspace
cargo test                           # Run all tests
cargo test -p theo-engine-graph      # Specific crate
bash scripts/check-arch-contract.sh  # Architecture gate
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
