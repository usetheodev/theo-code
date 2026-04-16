# 01 — System Overview

## What is Theo Code?

Standalone AI coding agent — same category as Claude Code, Codex CLI, Cursor, OpenCode. Differentiators:

- **GRAPHCTX** — structural code intelligence via multi-relational code property hypergraph (16 languages, Tree-Sitter, RRF 3-ranker, graph attention propagation)
- **Code Wiki** — Obsidian-like knowledge base auto-generated from code structure with BM25 search, runtime insights, and LLM enrichment

**Theo** (the platform) is the Vercel for developers who have a backend. Theo Code helps users build applications on the Theo platform.

## Architecture Style

Rust workspace with **11 crates** organized into **5 bounded contexts**, plus **5 surface applications** (3 Rust workspace members — `theo-cli`, `theo-desktop`, `theo-marklive` — plus `theo-ui` (npm) and `theo-benchmark` (Python), both isolated from the Rust workspace).

At the system level, Theo should be understood as a **coding-agent harness** in the precise sense defined by the harness-engineering research (`docs/pesquisas/harness-engineering.md`). A harness is everything in the agent system *other than the model*: the guides that steer behavior before it happens, the sensors that detect outcomes after it happens, the context that the model can actually see, and the durable artifacts that allow one session to hand off to the next.

- **Model** provides reasoning and tool-use generation. Stateless across sessions.
- **Runtime** drives incremental execution across turns and sessions. This is the **behavioral harness**.
- **Context engine** makes the codebase legible through structural analysis and retrieval. This is read-only **ambient affordance**: it shapes what the model can know without mutating anything.
- **Repository knowledge** in `docs/` and `.theo/` is the **system of record**. Anything the agent cannot discover in-repo is operationally invisible.
- **Governance + tooling + sensors** provide the **guides (feedforward)** and **sensors (feedback)** that regulate the codebase toward its desired state.

### Guide / Sensor Taxonomy

Following the harness-engineering mental model, every control in Theo is classified along two axes:

| Axis 1 — Direction | Axis 2 — Execution |
|---|---|
| **Guide (feedforward)** — steer before the agent acts | **Computational** — deterministic, fast, cheap (tests, linters, type checkers) |
| **Sensor (feedback)** — observe after the agent acts, inject self-correction signals | **Inferential** — semantic judgment, LLM-as-judge, AI review (slower, non-deterministic) |

Concrete mapping of Theo's controls:

| Control | Direction | Execution | Crate |
|---|---|---|---|
| System prompt, skills, AGENTS.md | Guide | Inferential | `theo-agent-runtime`, repository |
| Sandbox policy, capability gates, denylist | Guide | Computational | `theo-governance`, `theo-tooling` |
| GRAPHCTX context retrieval | Guide | Computational | `theo-engine-*` |
| Convergence / done gate (`cargo test`, git diff) | Sensor | Computational | `theo-agent-runtime` |
| Post-edit hook sensors (`.theo/hooks/edit.verify.sh`) | Sensor | Computational | `theo-agent-runtime` |
| Reflector / Evolution loop | Sensor | Inferential | `theo-agent-runtime` |
| Toxic command sequence analyzer | Sensor | Computational | `theo-governance` |
| Impact report / risk alerts | Sensor | Computational | `theo-governance` + `theo-engine-graph` |
| Wiki linter / drift detection | Sensor | Computational (core) + Inferential (enrichment pipeline) | `theo-engine-retrieval` |

> The research only defines two execution types — **Computational** and **Inferential**. Controls that blend both (e.g. a wiki linter that runs a fast static check plus an optional LLM enrichment pass) are split into two rows rather than collapsed into a fabricated "Hybrid" category. This keeps the taxonomy faithful to Böckeler.

### Regulation Categories

Theo's harness is designed to regulate three distinct dimensions (from `harness-engineering.md`):

1. **Maintainability harness** — internal code quality, duplication, complexity, test coverage. Covered by: tooling lints, governance audit, wiki linter, sensor hooks. Strongest category — the most existing tooling.
2. **Architecture fitness harness** — dependency direction, module boundaries, layering, performance/observability requirements. Covered by: domain dependency rules, Tool trait contracts, GRAPHCTX impact analysis, governance risk alerts. *This is where Theo enforces the inviolable dependency graph.*
3. **Behaviour harness** — does the application functionally do what the user needs? Covered (partially) by: done gate (`cargo test`), convergence evaluator, benchmark harness, feature lists (planned). *Acknowledged open problem across the industry.*

```
┌─────────────────────────────────────────────────────────────┐
│                    Surface Applications                      │
│  theo-cli · theo-desktop · theo-ui · theo-marklive · bench  │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│              Application Layer (theo-application)            │
│         Use cases · Pipeline · GraphContextService           │
│              theo-api-contracts (DTOs/Events)                │
└──┬───────────┬───────────┬──────────┬───────────┬───────────┘
   │           │           │          │           │
   ▼           ▼           ▼          ▼           ▼
┌──────┐ ┌─────────┐ ┌─────────┐ ┌────────┐ ┌──────────┐
│Agent │ │  Code   │ │ Infra-  │ │Tooling │ │Governance│
│Runtime│ │  Intel  │ │structure│ │& Sand- │ │ (Policy) │
│      │ │         │ │         │ │  box   │ │          │
│agent-│ │engine-  │ │infra-   │ │tooling │ │governance│
│runtime│ │graph    │ │llm      │ │        │ │          │
│      │ │engine-  │ │infra-   │ │        │ │          │
│      │ │parser   │ │auth     │ │        │ │          │
│      │ │engine-  │ │         │ │        │ │          │
│      │ │retrieval│ │         │ │        │ │          │
└──┬───┘ └────┬────┘ └────┬────┘ └───┬────┘ └────┬─────┘
   │          │           │          │            │
   └──────────┴───────────┴──────────┴────────────┘
                          │
              ┌───────────▼───────────┐
              │   theo-domain         │
              │   Pure types, traits  │
              │   ZERO crate deps     │
              └───────────────────────┘
```

## Bounded Contexts

### 1. Domain Core (`theo-domain`)
Pure types, traits, error enums, state machines. Zero internal crate dependencies. Every other crate imports from here. Defines the contracts that bind the system together: `Tool` trait, `GraphContextProvider` trait, `StateMachine` trait, `DomainEvent` types.

### 2. Code Intelligence (`theo-engine-*`)
Read-only analysis of source code. Three crates in a strict pipeline:
- **Parser** → AST extraction via Tree-Sitter (16 languages), symbol table, import resolution
- **Graph** → Multi-relational code property hypergraph with community detection
- **Retrieval** → BM25 + dense embeddings + graph attention + cross-encoder reranking + Code Wiki

### 3. Agent Runtime (`theo-agent-runtime`)
The brain. Orchestrates LLM calls, tool execution, state machines, compaction, convergence detection, sensors, evolution loop, sub-agents, and the autonomous pilot loop. ~40 modules. This is the **behavioral harness** that turns a stateless model into a long-running coding worker.

### 4. Infrastructure (`theo-infra-*`)
Concrete implementations behind domain traits:
- **LLM** — HTTP client for 25+ providers, all OA-compatible internally
- **Auth** — OAuth PKCE, device flow (RFC 8628), token storage for 8 providers

### 5. Governance (`theo-governance`)
Lightweight policy engine in the critical path. Risk assessment, sandbox policy generation, toxic command sequence detection, session quality metrics, audit trail. In harness-engineering terms, this is where Theo encodes many of its **guides** and **sensors**.

## Repository Knowledge Model

Theo follows a **repository-as-system-of-record** model (from `docs/pesquisas/harness-engineering-openai.md`): repository-local artifacts are not ancillary prose, they are *inputs to the runtime*. What the agent cannot discover in-repo is operationally invisible.

### Map, Not Encyclopedia

A single giant `AGENTS.md` fails in predictable ways — it crowds out real task context, becomes non-guidance when everything is "important," rots instantly, and cannot be mechanically verified. Theo follows the OpenAI pattern of treating `AGENTS.md` (and equivalent root files) as a **table of contents**, pointing at deeper sources of truth.

### Layout

```
AGENTS.md / CLAUDE.md / theo.md   ← short (~100 lines), injected as map
docs/
├── architecture/                  ← stable structural boundaries (this reference)
├── adr/                           ← Architecture Decision Records
├── roadmap/                       ← product roadmap
├── plans/                         ← active and completed execution plans
├── current/                       ← what IS implemented
├── target/                        ← what is planned
└── pesquisas/                     ← papers and external references (isolated)

.theo/                             ← operational artifacts (runtime-owned)
├── graph.bin                      ← persisted code graph
├── wiki/                          ← auto-generated knowledge base
├── state/{run_id}/session.jsonl   ← append-only session history
├── plans/                         ← active plans (write-gated in Plan mode)
├── hooks/                         ← pre/post/sensor shell hooks
└── audit.jsonl                    ← append-only sandbox audit trail
```

### Progressive Disclosure

The runtime assembles context in layers — small stable entry points first, deeper sources navigated on demand. See `04-agent-runtime.md` → System Prompt Assembly for the exact ordering. This prevents the "one big file" failure while still giving agents a navigable knowledge graph.

### Consequence: "What the Agent Cannot See Does Not Exist"

OpenAI's `harness-engineering-openai.md` §5 frames this as the agent's *operational visibility* boundary. Theo derives three hard rules from it:

1. **No external docs as load-bearing context.** If a decision matters for the agent, it lives under `docs/` or `.theo/`. Google Docs, Slack threads, and tickets are explicitly out of the trust boundary.
2. **Typed boundaries, not duck-typed strings.** Contracts shared across crates (DTOs, events) must be in `theo-api-contracts` or `theo-domain` — not reconstructed from JSON shape inference. This is the "[parse, don't validate](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/)" rule OpenAI enforces at the domain boundary.
3. **Prefer reimplementation over opaque deps.** When a third-party library hides behavior the agent cannot inspect, rewriting a minimal internal version is sometimes cheaper than working around its opacity. The OpenAI team made this tradeoff explicitly (`§5`, p-limit example).

## Key Invariants

| # | Invariant | Enforced by |
|---|---|---|
| 1 | `theo-domain` never depends on other crates | `Cargo.toml` |
| 2 | Apps never import engine/infra directly | `Cargo.toml` + review |
| 3 | Circular dependencies forbidden | `cargo` resolver |
| 4 | Every task state change validated by `StateMachine` | `TaskManager` |
| 5 | Every state transition generates a persisted `DomainEvent` | `RunEngine`, `TaskManager` |
| 6 | Every execution has a unique `RunId` | `AgentRunEngine::new()` |
| 7 | Snapshots have checksum validation | `RunSnapshot::compute_checksum()` |
| 8 | Budget enforced before every iteration | `BudgetEnforcer::check()` |
| 9 | Sandbox mandatory for bash: bwrap > landlock > noop cascade | `sandbox::create_executor()` |

## Data Flow — Single Agent Turn

```
User input
    │
    ▼
AgentLoop::run()
    │
    ▼
AgentRunEngine::execute_with_history()
    │
    ├─→ System prompt assembly (project context, memories, docs, skills, boot, GRAPHCTX)
    │
    ▼
┌─── Main Loop ──────────────────────────────────────────┐
│                                                         │
│  1. Budget check (Invariant 8)                         │
│  2. Sensor drain → inject feedback as system messages  │
│  3. Context loop injection (phase nudge)               │
│  4. Compaction (if >80% context used)                  │
│  5. LLM call (streaming, with retry)                   │
│  6. Parse response:                                     │
│     ├─ Text only → converge (check follow-up queue)    │
│     └─ Tool calls → execute phase                      │
│  7. For each tool call:                                │
│     ├─ Meta-tools: done, subagent, skill, batch        │
│     ├─ Pre-hook (blocking)                             │
│     ├─ Plan mode guard                                 │
│     ├─ Tool execution via ToolCallManager              │
│     ├─ State persistence (StateManager)                │
│     ├─ Sensor fire (if write tool + success)           │
│     ├─ Post-hook (fire-and-forget)                     │
│     └─ Doom loop detection                             │
│  8. Steering queue check                               │
│  9. Snapshot save (if configured)                      │
│ 10. Transition → Replanning, loop back to 1            │
│                                                         │
└─────────────────────────────────────────────────────────┘
    │
    ▼
record_session_exit() → metrics, episodes, progress
    │
    ▼
AgentResult { success, summary, files_edited, tokens, ... }
```

## Long-Running Session Contract

From `docs/pesquisas/effective-harnesses-for-long-running-agents.md` (Anthropic): long-running agents fail in two predictable patterns — trying to one-shot the whole task (running out of context mid-implementation) and declaring victory too early (a later session sees progress and marks done). The Anthropic mitigation is an **initializer agent** (first run, sets up environment) plus a **coding agent** (every subsequent run, makes incremental progress).

Theo codifies four session-level behaviors that every runtime invocation must satisfy:

1. **Get bearings quickly** — each session recovers project state from durable artifacts (`session.jsonl`, git log, plans, episode summaries, wiki) before planning. The runtime never relies on prompt continuation.
2. **Work incrementally** — prefer bounded tasks, explicit replanning, and one feature at a time over one-shot mega-edits. Enforced by budget limits and convergence checks.
3. **Leave a clean state** — every run ends with verification (`cargo test`, convergence evaluator), a snapshot, session artifacts, and progress notes so the next run can resume without archaeology.
4. **Use real feedback loops** — structural retrieval, tests, sensors, and runtime observations are first-class inputs to the next turn's decisions.

### Initializer vs Coding Prompts

Theo's equivalents (though the runtime itself is one codepath — they are different *entry points*):

| Role | Entry | Responsibility |
|---|---|---|
| Initializer | `theo init` (CLI) | Generate `.theo/theo.md` project context, seed initial plans and docs, derive conventions from code. Writes durable artifacts for future sessions. |
| Coding | `theo`, `theo pilot`, `theo --prompt ...` | Read bearings, pick a bounded unit of work, execute with verification, write progress artifacts. |

This separation is the same "different prompt for the very first context window" pattern from the Claude 4 prompting guide.

## Technology Stack

| Layer | Technology |
|---|---|
| Language | Rust 2024 edition |
| Async runtime | Tokio |
| Parsing | Tree-Sitter (16 grammars) |
| Search | BM25 (custom) + Tantivy (optional) |
| Embeddings | fastembed (ONNX, CPU) |
| Reranking | Jina Reranker v2 Base Multilingual |
| Serialization | serde + serde_json, bincode (graph), JSONL (sessions) |
| Errors | thiserror (typed), anyhow (CLI only) |
| HTTP | reqwest |
| Desktop | Tauri v2 |
| Frontend | React 18 + TypeScript + Tailwind + Radix UI |
| Sandbox | bubblewrap (Linux namespaces) > landlock > macOS sandbox-exec |
| Hashing | blake3 (content), SHA-256 (PKCE) |
| Graph persistence | bincode to `.theo/graph.bin` |
| Session persistence | JSONL append-only to `.theo/state/{run_id}/session.jsonl` |

## Build & Test

```bash
cargo build                              # Full workspace (needs libglib for desktop)
cargo build --workspace --exclude theo-desktop  # Without desktop
cargo test -p theo-domain                # Specific crate
cargo test --workspace --exclude theo-desktop --exclude theo-marklive  # All core
cd apps/theo-ui && npm run dev           # Frontend dev server
cd apps/theo-desktop && cargo tauri dev  # Desktop dev
```
