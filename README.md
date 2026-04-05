<p align="center">
  <h1 align="center">Theo</h1>
  <p align="center">
    <strong>The harness-first autonomous coding agent</strong>
  </p>
  <p align="center">
    The model writes code. Theo makes it work&hairsp;—&hairsp;sandboxed, context-aware, self-improving.
  </p>
  <p align="center">
    <a href="https://github.com/usetheodev/theo-code/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache%202.0-blue?style=flat-square"></a>
    <a href="https://github.com/usetheodev/theo-code"><img alt="Rust" src="https://img.shields.io/badge/rust-2024%20edition-orange?style=flat-square&logo=rust"></a>
    <a href="https://github.com/usetheodev/theo-code"><img alt="Tests" src="https://img.shields.io/badge/tests-1630%2B%20passing-brightgreen?style=flat-square"></a>
    <a href="https://github.com/usetheodev/theo-code"><img alt="Providers" src="https://img.shields.io/badge/LLM%20providers-25-purple?style=flat-square"></a>
  </p>
</p>

---

## What is Theo?

Theo is an **open-source, terminal-native coding agent** built in Rust. Unlike tools that bet everything on a single model, Theo focuses on what actually determines success: **the harness** — the sandbox, tools, context management, safety, and feedback loops that surround the model.

```bash
theo                              # Interactive REPL
theo "fix the auth bug"           # Single-shot task
theo --mode plan "design caching" # Plan before acting
theo pilot "implement feature X"  # Fully autonomous loop
theo init                         # AI-powered project setup
```

Theo works with **any OpenAI-compatible model** — GPT, Claude, Codex, Ollama, Groq, Mistral, and 20+ more. The model is pluggable. The harness is the product.

## Why Harness Engineering?

> *"Agent = Model + Harness. Most of the gains come from the harness."*
> — Martin Fowler, 2025

Every coding agent uses the same models. What separates the ones that work from the ones that don't is **everything around the model**: how it sees your code, how it's sandboxed, how context is managed, how failures are detected, and how the system improves over time.

Theo is the first coding agent designed **harness-first**:

| Harness Layer | What Theo Does | Why It Matters |
|---|---|---|
| **Sandbox** | bwrap/landlock/seccomp cascade, SSRF blocking, env sanitization | Agent can't escape or exfiltrate data |
| **Code Intelligence** | 16-language parser + code graph + semantic search (on-demand) | Agent sees the right files, not all files |
| **Context Engineering** | Compaction, just-in-time retrieval, context loops, token budgeting | Sessions stay coherent past 20+ turns |
| **Feedback Loops** | Doom loop detection, circuit breaker, heuristic reflector | Agent self-corrects instead of looping forever |
| **Memory** | Cross-session persistence, learnings, trajectory snapshots | Agent resumes where it left off |
| **Safety** | Governance engine, policy evaluation, command validation | Every tool call is assessed before execution |

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

Theo auto-detects your LLM provider:

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

# Initialize project context (AI-powered analysis)
theo init

# Start coding
theo "add input validation to the create endpoint"
```

On first run in any project, Theo automatically creates `.theo/theo.md` with your project's structure, language, and conventions.

## Features

### Agent Modes

```bash
theo                              # Agent mode — full autonomy
theo --mode plan "design X"       # Plan mode — analyze before acting
theo --mode ask "explain auth"    # Ask mode — questions first, action later
```

### Pilot — Autonomous Loop

Theo Pilot runs continuously until a promise is fulfilled, with circuit breaker protection:

```bash
theo pilot "implement user authentication" \
  --complete "all tests pass and login works" \
  --calls 10
```

Features: circuit breaker (stops on repeated failures), dual-exit gate (done signal + git progress), corrective guidance (self-improving via heuristic reflector), roadmap execution.

### Code Intelligence (GRAPHCTX)

Theo's `codebase_context` tool gives the model a structural map of your code — on-demand, not always-on:

```
Agent: "I need to understand the auth system"
  → calls codebase_context("authentication flow")
  → receives: function signatures, struct definitions, module layout
  → edits the RIGHT files with full context
```

Built on: Tree-Sitter (16 languages), MCPH code graph, Leiden community detection, BM25 + neural embeddings, graph attention propagation.

### 21+ Built-in Tools

| Category | Tools |
|---|---|
| **Core** | `bash` (sandboxed), `read`, `write`, `edit`, `grep`, `glob`, `apply_patch` |
| **Intelligence** | `codebase_context`, `webfetch`, `think`, `reflect`, `memory` |
| **Git** | `git_status`, `git_diff`, `git_log`, `git_commit` (with safety checks) |
| **HTTP** | `http_get`, `http_post` (with SSRF protection) |
| **Meta** | `batch` (parallel execution), `subagent`, `skill`, `done` |

### Sub-Agents

Delegate work to specialized sub-agents that run in parallel:

```
Main Agent: "I need to fix the bug and add tests"
  → spawns explorer sub-agent (reads code, finds root cause)
  → spawns implementer sub-agent (applies fix)
  → spawns verifier sub-agent (runs tests)
```

3-layer recursive spawning prevention: schema stripping + prompt isolation + capability gate.

### Skills

10 bundled skills for common workflows: `commit`, `test`, `review`, `build`, `explain`, `fix`, `refactor`, `pr`, `doc`, `deps`. Extensible with project-specific skills in `.theo/skills/`.

### Session Persistence

Conversations persist across terminal restarts. Theo remembers what you were working on:

```
[theo] Restored session (12 messages)
```

### Context Compaction

Long sessions don't degrade. Theo automatically compresses old messages while preserving critical information — system messages, recent context, and tool call pairs stay intact.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   theo (CLI)                     │
│              theo-desktop (Tauri v2)             │
├─────────────────────────────────────────────────┤
│              theo-application                    │
│         (use cases, GRAPHCTX service)            │
├──────────────┬───────────────┬──────────────────┤
│ Code Intel   │ Agent Runtime │   Governance     │
│ engine-graph │ agent-runtime │   governance     │
│ engine-parser│ (loop, pilot, │   (policies,     │
│ engine-retr. │  reflector)   │    impact)       │
├──────────────┴───────────────┴──────────────────┤
│                Infrastructure                    │
│    theo-infra-llm (25 providers, streaming)      │
│    theo-infra-auth (OAuth PKCE, Device Flow)     │
│    theo-tooling (21 tools, sandbox, plugins)     │
├─────────────────────────────────────────────────┤
│              theo-domain                         │
│       Pure types, traits, errors (zero deps)     │
└─────────────────────────────────────────────────┘
```

### Bounded Contexts

- **Code Intelligence** — `engine-graph`, `engine-parser`, `engine-retrieval`: read-only analysis of source code
- **Agent Runtime** — `agent-runtime`: orchestrates LLM + tools + governance in an async loop
- **Governance** — `governance`: policy engine, impact analysis, risk assessment
- **Infrastructure** — `infra-llm`, `infra-auth`, `tooling`: external connections behind traits
- **Domain** — `theo-domain`: pure types shared across all contexts (zero dependencies)

### Key Invariants

- `theo-domain` **never** depends on other crates
- Apps talk to `theo-application`, **never** to engines directly
- Every tool call passes through the Decision Control Plane
- `done()` is blocked until `git diff` shows real changes
- Sandbox is mandatory for `bash` — bwrap > landlock > noop cascade

## LLM Providers

Theo supports **25 providers** out of the box. Internally everything is OpenAI-compatible — providers convert at the boundary.

| Provider | Auth | Auto-detect |
|---|---|---|
| OpenAI (GPT-4o, o1, o3) | API key | `OPENAI_API_KEY` |
| OpenAI Codex (gpt-5.3-codex) | OAuth PKCE | Automatic |
| Anthropic (Claude 4) | API key | `ANTHROPIC_API_KEY` |
| Ollama (local) | None | `localhost:11434` |
| Groq, Mistral, Together, DeepSeek, Fireworks... | API key | Env var |
| GitHub Copilot | Device Flow | Automatic |
| Any OA-compatible endpoint | Configurable | `--provider` flag |

## Supported Languages

Theo parses **16 languages** via Tree-Sitter with framework-specific extractors:

| Language | Frameworks |
|---|---|
| TypeScript/JavaScript | Express, Koa, Hapi |
| Python | FastAPI, Flask, Django |
| Java/Kotlin/Scala | Spring Boot |
| Rust, Go, C, C++ | Generic extraction |
| C# | ASP.NET Core |
| PHP | Laravel |
| Ruby | Rails |
| Swift | Generic extraction |

## Project Configuration

```
your-project/
└── .theo/
    ├── theo.md              # Project context (auto-generated by `theo init`)
    ├── system-prompt.md     # Custom system prompt (optional)
    ├── config.toml          # Pilot and agent configuration
    ├── skills/              # Project-specific skills
    ├── hooks/               # Pre/post tool execution hooks
    ├── plans/               # Roadmaps for pilot execution
    └── .gitignore           # Excludes generated files (graph, learnings, snapshots)
```

## Development

### Build

```bash
cargo build                    # Build workspace
cargo test                     # Run all 1630+ tests
cargo test -p theo-agent-runtime  # Test a specific crate
```

### Run

```bash
cargo run --bin theo           # Run from source
cargo install --path apps/theo-cli  # Install globally
```

## Contributing

We welcome contributions! Architecture rules:

1. `theo-domain` has **zero dependencies** on other crates
2. Apps talk to `theo-application`, never to engines directly
3. Every logic change needs test coverage (Arrange-Act-Assert)
4. `cargo test` must pass with zero failures and zero warnings
5. Code in English, communication in English or Portuguese

## Harness Engineering Philosophy

Theo is built on the principle that **the harness matters more than the model**. We follow the industry consensus from OpenAI, Anthropic, and Martin Fowler:

- **Generic tools over specialized tools** — the model already knows bash, read, write
- **Context engineering over context stuffing** — better tokens, not more tokens
- **Computational sensors over inferential sensors** — tests and linters before AI review
- **Environment legibility** — the agent should understand its surroundings instantly
- **Self-improvement** — the harness learns from every failure

Read the full technical document: [`docs/current/harness-engineering.md`](docs/current/harness-engineering.md)

## License

Licensed under the [Apache License 2.0](LICENSE).

---

<p align="center">
  <sub>The model is commodity. The harness is the product.</sub>
</p>
