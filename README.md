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
    <a href="https://github.com/usetheodev/theo-code"><img alt="Languages" src="https://img.shields.io/badge/languages-16-purple?style=flat-square"></a>
  </p>
</p>

---

## What is Theo Code?

Theo Code is an **AI coding assistant** that actually understands your codebase. It combines a fast CLI, a desktop app, and a **Code Wiki** that turns your project into a living, searchable knowledge base — like Obsidian, but auto-generated from your code.

```bash
theo                              # Interactive REPL
theo "fix the auth bug"           # Single-shot task
theo --mode plan "design caching" # Plan before acting
theo pilot "implement feature X"  # Fully autonomous loop
```

Theo works with **any OpenAI-compatible model** — GPT, Claude, Codex, Ollama, Groq, Mistral, and 20+ more.

## What makes Theo different?

### GRAPHCTX — Deep Code Intelligence

Most AI coding tools dump files into the context window and hope for the best. Theo builds a **code graph** of your entire project — functions, types, imports, dependencies — and retrieves exactly what the model needs.

```
You: "fix the payment validation"
  → Theo retrieves: PaymentValidator, related types, upstream callers, test files
  → Model sees the RIGHT context, not everything
```

Built on: Tree-Sitter (16 languages), code graph with community detection, RRF 3-ranker (BM25 + Tantivy + neural embeddings), graph attention propagation.

**Benchmark**: MRR=0.86, Hit@5=0.97, cross-language (Rust + Python) validated on 57 queries across 3 repos.

### Code Wiki — Your codebase as a knowledge base

Theo auto-generates a **navigable wiki** from your code — modules, types, dependencies, architecture — updated as your code changes. Think Obsidian for your codebase.

- **Auto-generated pages** for every module, crate, and significant type
- **Dependency graphs** showing how components connect
- **LLM-enriched summaries** explaining what each module does and why
- **Searchable** via BM25 full-text search
- **Write-back** — the wiki learns and compounds knowledge over time

```bash
theo wiki                         # Generate wiki for current project
theo wiki --serve                 # Browse in your browser
```

### CLI + Desktop

Two interfaces, same engine:

| | CLI | Desktop |
|---|---|---|
| **For** | Terminal-native developers | Visual exploration |
| **Mode** | REPL, single-shot, pilot | Chat + Code Wiki browser |
| **Stack** | Rust binary | Tauri v2 + React |
| **Speed** | Instant startup | Native performance |

## Quick Start

### Install

```bash
# From source
git clone https://github.com/usetheodev/theo-code.git
cd theo-code
cargo install --path apps/theo-cli

# Verify
theo --version
```

### Configure

```bash
# Option 1: OpenAI API key
export OPENAI_API_KEY=sk-...

# Option 2: Ollama (local, free)
ollama serve  # Theo auto-detects localhost:11434

# Option 3: Any OpenAI-compatible endpoint
export OPENAI_API_KEY=your-key
theo --provider groq "fix the bug"
```

### First Run

```bash
cd your-project
theo init          # AI-powered project analysis + wiki generation
theo "add input validation to the create endpoint"
```

## Features

### Agent Modes

```bash
theo                              # Agent mode — full autonomy
theo --mode plan "design X"       # Plan mode — think before acting
theo --mode ask "explain auth"    # Ask mode — questions only
theo pilot "implement feature X"  # Pilot — autonomous loop with circuit breaker
```

### 21+ Built-in Tools

| Category | Tools |
|---|---|
| **Core** | `bash` (sandboxed), `read`, `write`, `edit`, `grep`, `glob`, `apply_patch` |
| **Intelligence** | `codebase_context`, `webfetch`, `think`, `reflect`, `memory` |
| **Git** | `git_status`, `git_diff`, `git_log`, `git_commit` |
| **HTTP** | `http_get`, `http_post` (SSRF-protected) |
| **Meta** | `batch` (parallel), `subagent`, `skill`, `done` |

### Sub-Agents

Delegate work to specialized sub-agents that run in parallel:

```
Main Agent: "fix the bug and add tests"
  → spawns explorer (reads code, finds root cause)
  → spawns implementer (applies fix)
  → spawns verifier (runs tests)
```

### Sandbox

Every `bash` command runs sandboxed — bwrap > landlock > noop cascade. PID isolation, network control, env sanitization, command validation.

### Session Persistence & Context Compaction

Sessions persist across restarts. Long conversations don't degrade — Theo compresses old messages while preserving critical context.

## Architecture

```
┌─────────────────────────────────────────────────┐
│           theo-cli    /    theo-desktop          │
│          (terminal)       (Tauri v2 + React)     │
├─────────────────────────────────────────────────┤
│              theo-application                    │
│         (use cases, GRAPHCTX service)            │
├──────────────┬───────────────┬──────────────────┤
│ Code Intel   │ Agent Runtime │   Governance     │
│ engine-graph │ agent-runtime │   governance     │
│ engine-parser│               │                  │
│ engine-retr. │               │                  │
├──────────────┴───────────────┴──────────────────┤
│                Infrastructure                    │
│    theo-infra-llm (25 providers, streaming)      │
│    theo-infra-auth (OAuth PKCE, Device Flow)     │
│    theo-tooling (21 tools, sandbox)              │
├─────────────────────────────────────────────────┤
│              theo-domain                         │
│       Pure types, traits, errors (zero deps)     │
└─────────────────────────────────────────────────┘
```

**11 crates**, 4 bounded contexts, strict dependency rules. `theo-domain` has zero dependencies. Apps talk to `theo-application`, never to engines directly.

## Supported Languages

16 languages via Tree-Sitter:

TypeScript, JavaScript, Python, Rust, Go, Java, Kotlin, Scala, C, C++, C#, PHP, Ruby, Swift — with framework-specific extractors for Express, FastAPI, Flask, Django, Spring Boot, ASP.NET, Laravel, and Rails.

## LLM Providers

25 providers out of the box. Internally everything is OpenAI-compatible — providers convert at the boundary.

| Provider | Auth |
|---|---|
| OpenAI (GPT-4o, o1, o3, Codex) | API key / OAuth PKCE |
| Anthropic (Claude) | API key |
| Ollama (local) | None |
| Groq, Mistral, Together, DeepSeek, Fireworks... | API key |
| GitHub Copilot | Device Flow |
| Any OA-compatible endpoint | `--provider` flag |

## Development

```bash
cargo build                          # Build workspace
cargo test                           # Run all tests
cargo test -p theo-engine-graph      # Test specific crate
cd apps/theo-desktop && cargo tauri dev  # Desktop app
cd apps/theo-ui && npm run dev       # Frontend dev server
```

## Contributing

1. `theo-domain` has **zero dependencies** on other crates
2. Apps talk to `theo-application`, never to engines directly
3. Every logic change needs tests (Arrange-Act-Assert)
4. `cargo test` must pass with zero failures
5. Code in English, communication in Portuguese or English

## License

Licensed under the [Apache License 2.0](LICENSE).

---

<p align="center">
  <sub>Built by the <a href="https://usetheo.dev">Theo</a> team. AI that understands your code, not just reads it.</sub>
</p>
