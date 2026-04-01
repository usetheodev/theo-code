# Theo Code

Governance-first autonomous code agent. The LLM decides **what** to do — Theo Code governs **if** it can, **how** to record it, and **when** to stop.

Built in Rust with a Tauri v2 desktop app.

## What is Theo Code?

Theo Code sits on top of AI coding tools (Claude Code, Codex, Cursor) as a **Judge layer** that monitors, explains, and governs agent actions in real-time. It combines:

- **GRAPHCTX** — Multi-language code graph (14 languages via Tree-Sitter) for precise file targeting
- **State Machine** — Deterministic phases that block `done()` until git diff proves real changes
- **Context Loops** — Periodic summaries that prevent agent drift
- **Policy Engine** — Mini-DSL for access control and validation gates (<50ms)

These mechanisms together achieved **50% on SWE-bench Lite** with Qwen3-30B (a 30B model), surpassing GPT-4-based systems like SWE-Agent (18%) and matching larger-model solutions.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  Product Surfaces                │
│         theo-cli  ·  theo-desktop (Tauri v2)     │
├─────────────────────────────────────────────────┤
│              Application Layer                   │
│         theo-application  ·  theo-api-contracts  │
├──────────────┬──────────────┬───────────────────┤
│  Code Intel  │ Agent Runtime│   Governance      │
│  engine-graph│ agent-runtime│   governance      │
│  engine-parse│              │                   │
│  engine-retr │              │                   │
├──────────────┴──────────────┴───────────────────┤
│              Infrastructure                      │
│      theo-infra-llm  ·  theo-infra-auth         │
│              theo-tooling                        │
└─────────────────────────────────────────────────┘
```

### Crates

| Crate | Purpose |
|---|---|
| `theo-domain` | Core types, traits, errors, permissions, sessions |
| `theo-engine-graph` | Code graph via Tree-Sitter — clustering, co-change analysis, persistence |
| `theo-engine-parser` | Multi-language AST parser (14 languages), symbol/import extraction |
| `theo-engine-retrieval` | Semantic search with neural embeddings, TF-IDF, graph attention |
| `theo-governance` | Policy enforcement, impact analysis, metrics, alerting |
| `theo-agent-runtime` | Async agent loop orchestration |
| `theo-infra-llm` | LLM client abstraction (OpenAI-compatible, Anthropic, vLLM) |
| `theo-infra-auth` | OAuth PKCE, device flow, token management |
| `theo-tooling` | Tool registry: apply_patch, bash, edit, glob, grep, LSP, search, webfetch, write |
| `theo-api-contracts` | Shared DTOs and events for surfaces |
| `theo-application` | Use case orchestration (run_agent_session, etc.) |

### Apps

| App | Description |
|---|---|
| `theo-cli` | Command-line interface |
| `theo-desktop` | Tauri v2 desktop app (React 18 + TypeScript + Tailwind) |

## Getting Started

### Prerequisites

- Rust (edition 2024)
- Node.js 18+
- System dependencies for Tauri v2 ([see Tauri docs](https://v2.tauri.app/start/prerequisites/))

### Build

```bash
# Build all crates
cargo build

# Run tests
cargo test

# Launch desktop app
cd apps/theo-desktop
cargo tauri dev
```

## Desktop App

The desktop app provides:

- Chat interface with agent interaction
- Multi-tab views: Agent, Plan, Tests, Review, Security
- Real-time event streaming from the Rust backend
- Project directory configuration
- Auth login flow (browser + device)

**Tech stack:** React 18, React Router, Radix UI, Tailwind CSS, Framer Motion, Vite.

## Supported Languages

Tree-Sitter parsers for: Rust, Python, TypeScript, JavaScript, Go, Java, C, C++, C#, Ruby, PHP, Swift, Kotlin, and more.

## SWE-bench Results

| System | Model | SWE-bench Lite |
|---|---|---|
| SWE-Agent | GPT-4 | 18% |
| Agentless | GPT-4o | 27% |
| Aider | Claude 3.5 | 27% |
| **Theo Code** | **Qwen3-30B** | **50%** |
| OpenHands | Claude 3.5 | 53% |

Theo Code achieves competitive results with a significantly smaller model by combining GRAPHCTX, State Machine, and Context Loops.

## Claude Code Integration

Theo Code integrates with Claude Code via:

1. **Hooks** — 25+ events (PreToolUse, PostToolUse, SessionStart, etc.) for real-time monitoring
2. **StatusLine** — Persistent real-time status display
3. **MCP Server** — Bidirectional tools (theo_explain, theo_review)
4. **Skills** — Distributable commands (/theo:explain, /theo:review)

## License

MIT
