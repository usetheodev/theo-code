<p align="center">
  <h1 align="center">Theo Code</h1>
  <p align="center">
    <strong>Governance-first runtime for AI coding agents</strong>
  </p>
  <p align="center">
    The LLM decides <em>what</em> to do — Theo Code governs <em>if</em> it can, <em>how</em> to record it, and <em>when</em> to stop.
  </p>
  <p align="center">
    <a href="https://github.com/usetheodev/theo-code/actions"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/usetheodev/theo-code/ci.yml?style=flat-square&label=CI"></a>
    <a href="https://github.com/usetheodev/theo-code/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache%202.0-blue?style=flat-square"></a>
    <a href="https://github.com/usetheodev/theo-code"><img alt="Rust" src="https://img.shields.io/badge/rust-2024%20edition-orange?style=flat-square&logo=rust"></a>
    <a href="https://github.com/usetheodev/theo-code"><img alt="Tests" src="https://img.shields.io/badge/tests-2647%20passing-brightgreen?style=flat-square"></a>
  </p>
</p>

---

## Why Theo Code?

AI coding agents (Claude Code, Codex, Cursor) are powerful writers — but they operate without guardrails. They can edit any file, run any command, and mark a task as "done" without proof. **Theo Code is the governance layer that sits on top**, monitoring every action in real-time and enforcing safety policies before damage happens.

Three core mechanisms make this work:

| Mechanism | What it does | Why it matters |
|---|---|---|
| **GRAPHCTX** | Multi-language code graph (16 languages) that gives the agent precise file targets | Agents stop guessing which files to edit |
| **State Machine** | Deterministic phases (LOCATE → EDIT → VERIFY → DONE) with git-diff proof gates | `done()` is blocked until real changes exist |
| **Context Loops** | Periodic summaries of what was done, what failed, and what to do next | Prevents drift in long-running sessions |

These mechanisms together achieved **50% on SWE-bench Lite** with Qwen3-30B — a 30B parameter model outperforming GPT-4-based systems.

## SWE-bench Results

| System | Model | SWE-bench Lite |
|---|---|---|
| SWE-Agent | GPT-4 | 18% |
| Agentless | GPT-4o | 27% |
| Aider | Claude 3.5 Sonnet | 27% |
| **Theo Code** | **Qwen3-30B** | **50%** |
| OpenHands | Claude 3.5 Sonnet | 53% |

> Theo Code achieves competitive results with a **significantly smaller model** by combining structured governance with precise code intelligence — not brute-force token scale.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                    Product Surfaces                       │
│           theo-cli  ·  theo-desktop (Tauri v2)           │
├──────────────────────────────────────────────────────────┤
│                   Application Layer                       │
│          theo-application  ·  theo-api-contracts          │
├───────────────────┬────────────────┬─────────────────────┤
│   Code Intel      │  Agent Runtime │    Governance       │
│   engine-graph    │  agent-runtime │    governance       │
│   engine-parser   │                │                     │
│   engine-retrieval│                │                     │
├───────────────────┴────────────────┴─────────────────────┤
│                    Infrastructure                         │
│         theo-infra-llm  ·  theo-infra-auth               │
│                    theo-tooling                           │
├──────────────────────────────────────────────────────────┤
│                    theo-domain                            │
│            Core types, traits, errors (zero deps)         │
└──────────────────────────────────────────────────────────┘
```

### Crates

<details>
<summary><strong>Code Intelligence Engine</strong> — understands your codebase</summary>

| Crate | Purpose |
|---|---|
| `theo-engine-parser` | Multi-language AST parser (16 languages via Tree-Sitter), symbol extraction, import resolution, type hierarchy analysis, framework-specific extractors for 7 backend families |
| `theo-engine-graph` | Code graph construction, community detection, co-change analysis, clustering, persistence |
| `theo-engine-retrieval` | Semantic search with neural embeddings + TF-IDF fallback, BM25 inverted index, PageRank centrality, graph attention propagation, TurboQuant compression (384-dim → 96 bytes) |

</details>

<details>
<summary><strong>Governance & Safety</strong> — enforces policies before execution</summary>

| Crate | Purpose |
|---|---|
| `theo-governance` | Policy engine with impact analysis (BFS-based), session metrics tracking, risk alerting (Info/Warning/Critical), community-aware change detection, test coverage linking |

</details>

<details>
<summary><strong>Agent Runtime</strong> — orchestrates the agent loop</summary>

| Crate | Purpose |
|---|---|
| `theo-agent-runtime` | Async agent loop orchestration, state machine transitions, context loop emission, decision control plane |

</details>

<details>
<summary><strong>Infrastructure</strong> — connects to the outside world</summary>

| Crate | Purpose |
|---|---|
| `theo-infra-llm` | LLM client abstraction — OpenAI-compatible, Anthropic, vLLM providers with streaming support |
| `theo-infra-auth` | OAuth PKCE, device flow, secure token storage and refresh |
| `theo-tooling` | Tool registry with 20+ tools: apply_patch, bash, edit, glob, grep, LSP, webfetch, codesearch, and more |

</details>

<details>
<summary><strong>Application & Domain</strong> — glues everything together</summary>

| Crate | Purpose |
|---|---|
| `theo-domain` | Core types, traits, errors, permissions, sessions — zero external dependencies |
| `theo-application` | Use case orchestration (run_agent_session, build_project_graph, etc.) |
| `theo-api-contracts` | Shared DTOs and serializable events for all surfaces |

</details>

### Apps

| App | Description |
|---|---|
| `theo-cli` | Command-line interface for headless agent sessions |
| `theo-desktop` | Tauri v2 desktop app — React 18, TypeScript, Tailwind CSS, Radix UI, Framer Motion |

## Supported Languages

Theo Code parses and understands code in **16 languages** via Tree-Sitter:

| Language Family | Languages |
|---|---|
| **JavaScript** | TypeScript, TSX, JavaScript, JSX |
| **JVM** | Java, Kotlin, Scala |
| **Systems** | Rust, C, C++, Go |
| **Scripting** | Python, Ruby, PHP |
| **Mobile** | Swift, C# |

Framework-specific extractors cover **~78% of backend market share**: Express/Koa/Hapi, FastAPI/Flask/Django, Spring Boot, ASP.NET Core, Gin/Echo, Laravel, and Rails. All other frameworks fall through to a generic extractor.

## Getting Started

### Prerequisites

- **Rust** nightly (edition 2024) — install via [rustup](https://rustup.rs)
- **Node.js** 18+ — for the desktop frontend
- **System libraries** for Tauri v2 — see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

### Build

```bash
# Clone the repository
git clone https://github.com/usetheodev/theo-code.git
cd theo-code

# Build all crates
cargo build

# Run the full test suite (2,647 tests)
cargo test

# Launch the desktop app in dev mode
cd apps/theo-desktop
cargo tauri dev
```

### Run the CLI

```bash
cargo run -p theo-cli
```

## Claude Code Integration

Theo Code integrates with [Claude Code](https://docs.anthropic.com/en/docs/claude-code) as a governance overlay:

| Integration | Purpose |
|---|---|
| **Hooks** | 25+ events (PreToolUse, PostToolUse, SessionStart, Stop) for real-time monitoring and blocking |
| **StatusLine** | Persistent display updated every 300ms showing governance status |
| **MCP Server** | Bidirectional tools — Claude Code can call `theo_explain` and `theo_review` |
| **Skills** | Distributable commands: `/theo:explain`, `/theo:review` |

The recommended deployment is a **Claude Code plugin** bundling all four integration points into a single distributable package.

## Project Structure

```
theo-code/
├── crates/
│   ├── theo-domain/              # Core types (zero deps)
│   ├── theo-engine-graph/        # Code graph + clustering
│   ├── theo-engine-parser/       # 16-language AST parser
│   ├── theo-engine-retrieval/    # Semantic search + embeddings
│   ├── theo-governance/          # Policy engine + impact analysis
│   ├── theo-agent-runtime/       # Async agent loop
│   ├── theo-infra-llm/           # LLM providers
│   ├── theo-infra-auth/          # OAuth + token management
│   ├── theo-tooling/             # Tool registry (20+ tools)
│   ├── theo-api-contracts/       # Shared DTOs
│   └── theo-application/         # Use case layer
├── apps/
│   ├── theo-cli/                 # CLI binary
│   ├── theo-desktop/             # Tauri v2 backend
│   ├── theo-ui/                  # React frontend
│   └── theo-benchmark/           # Benchmark runner
├── docs/
│   ├── current/                  # What IS implemented
│   ├── target/                   # What is planned
│   ├── adr/                      # Architecture Decision Records
│   └── roadmap/                  # Product roadmap
└── research/                     # Papers + experiments (isolated)
```

## Design Principles

- **Governance is not optional.** Every tool call passes through the Decision Control Plane before execution. No exceptions.
- **Prove it with a diff.** The state machine blocks `done()` until `git diff` shows real changes. No empty completions.
- **Context over tokens.** GRAPHCTX gives the agent the right 5 files instead of dumping 500 files into context.
- **Fail fast, fail loud.** Typed errors with `thiserror`, structured alerting, no silent failures.
- **Boundaries matter.** `theo-domain` has zero dependencies. Apps never import engines directly. No circular dependencies.

## Contributing

We welcome contributions! Here's how to get started:

1. **Fork** the repository
2. **Create a branch** from `main` for your feature or fix
3. **Write tests** — every logic change needs test coverage
4. **Run the suite** — `cargo test` must pass with zero failures
5. **Submit a PR** with a clear description of what and why

### Architecture Rules

- `theo-domain` **never** depends on other crates
- Apps talk to `theo-application`, **never** to engines directly
- Governance is mandatory in the critical path, not post-process
- All errors are typed — no `unwrap()` in production code
- Tests follow Arrange-Act-Assert pattern

### Code Style

- **Code**: English (variables, functions, types, technical comments)
- **Communication**: Portuguese or English
- Rust edition 2024 with `resolver = "3"`

## Roadmap

Theo Code is under active development. Key upcoming milestones:

- [ ] Claude Code plugin (hooks + MCP + StatusLine + skills)
- [ ] Real-time session monitoring dashboard
- [ ] Multi-provider governance (Claude Code, Codex, Cursor)
- [ ] Policy DSL for custom governance rules
- [ ] Sub-agent orchestration with governance gates

See [`docs/roadmap/`](docs/roadmap/) for the full plan.

## License

Licensed under the [Apache License 2.0](LICENSE).

---

<p align="center">
  <sub>Built with Rust, governed by design.</sub>
</p>
