# 08 — Application Layer (`theo-application` + `theo-api-contracts`)

The orchestration layer. Aggregates all bounded contexts into end-to-end use cases. The **only crate allowed to import everything**. No surface application should call engine/infra crates directly.

Conceptually, this layer turns Theo's bounded contexts into a coherent **outer harness**: it wires runtime, context engine, governance, tooling, and knowledge artifacts into one operational system.

Dependencies: `theo-domain`, `theo-agent-runtime`, `theo-tooling`, `theo-api-contracts`, `theo-engine-graph`, `theo-engine-parser`, `theo-engine-retrieval`, `theo-governance`, `theo-infra-llm`, `tokio`, `rayon`, `bincode`, `blake3`.

---

## theo-application

### Module Map

```
theo-application/src/
└── use_cases/
    ├── mod.rs                  # Module root
    ├── run_agent_session.rs    # Primary entry point
    ├── pipeline.rs             # GRAPHCTX E2E orchestrator
    ├── graph_context_service.rs # GraphContextProvider implementation
    ├── context_assembler.rs    # Deterministic context assembly
    ├── extraction.rs           # File/symbol extraction from disk
    ├── conversion.rs           # Type conversion helpers
    ├── impact.rs               # BFS impact traversal on code graph
    ├── wiki_backend_impl.rs    # WikiBackend implementation
    ├── wiki_enrichment.rs      # Wiki page enrichment with code links
    └── wiki_highlevel.rs       # LLM-generated overview pages
```

### Primary Entry Point

```rust
pub async fn run_agent_session(
    config: AgentConfig,
    task: &str,
    project_dir: &Path,
    event_listener: Arc<dyn EventListener>,
) -> Result<AgentResult, RunSessionError>;
```

This function:
1. Validates config (API key, project dir)
2. Initializes GRAPHCTX in background (unless `THEO_NO_GRAPHCTX=1`)
3. Creates default tool registry
4. Constructs and runs `AgentLoop`
5. Returns `AgentResult`

It is also the seam where repository-local knowledge becomes runtime behavior.

### Pipeline — GRAPHCTX E2E Orchestrator

The `Pipeline` struct orchestrates the full code intelligence flow:

```
Source directory
    │
    ▼
Pipeline::build_from_directory()
    │
    ├─→ Walk files (ignore .gitignore, EXCLUDED_DIRS)
    ├─→ Parse each file (rayon parallel, 16 languages)
    ├─→ Build graph (4-pass bridge algorithm)
    ├─→ Add git co-changes (temporal decay)
    ├─→ Cluster communities (Leiden, configurable resolution)
    ├─→ Generate community summaries (for BM25 indexing)
    └─→ Save graph + clusters to disk
    │
    ▼
Pipeline::assemble_context(query, budget)
    │
    ├─→ BM25 search over community summaries
    ├─→ Graph attention propagation
    ├─→ File-level retrieval + scoring
    └─→ Greedy knapsack assembly within budget
    │
    ▼
ContextPayload { blocks, total_tokens, budget_tokens }
```

#### Pipeline Config

```rust
pub struct PipelineConfig {
    pub token_budget: usize,           // default: 8000
    pub max_git_commits: usize,        // default: 500
    pub max_files_per_commit: usize,   // default: 20
    pub impact_bfs_depth: usize,       // default: 3
    pub graph_cache_path: Option<PathBuf>,
}
```

#### Incremental Updates

`Pipeline::update_file()` supports incremental graph updates:
- Removes old nodes/edges for the changed file
- Re-parses and re-inserts
- Triggers re-clustering if community membership changed
- Regenerates affected summaries
- Returns `UpdateResult` with timing breakdown

### GraphContextService

Concrete implementation of `GraphContextProvider` trait (from `theo-domain`). The bridge between the agent runtime and the code intelligence engine.

```rust
pub struct GraphContextService {
    pipeline: Mutex<Option<Pipeline>>,
    ready: AtomicBool,
    building: AtomicBool,
}
```

- **Background initialization**: `initialize()` spawns a `tokio::task` with 60s timeout
- **Thread-safe**: `Mutex` for pipeline access, atomic flags for status
- **Feature-gated**: Optional tantivy/dense-retrieval/reranker depending on build flags

This is the application-layer bridge between the behavioral harness and the read-only context engine.

### Context Assembler

Deterministic 4-rule context composer with feedback scoring:

```rust
pub struct ContextAssembler {
    ema_scores: HashMap<String, f64>,    // Exponential Moving Average per source
    repetition_penalty: f64,             // Penalize recently-used sources
}
```

Rules:
1. **Budget allocation**: Split between task overhead, execution context, structural context
2. **EMA feedback**: Sources that led to successful tool calls get boosted
3. **Repetition penalty**: Same source in consecutive turns gets penalized
4. **Greedy selection**: Highest-scoring blocks first, within budget

This design aligns with the research view that context management is a control problem: choose the smallest set of artifacts that increases the probability of correct next actions.

### Impact Analysis

```rust
pub fn analyze_impact(
    graph: &CodeGraph,
    communities: &[Community],
    edited_file: &str,
    bfs_depth: usize,
) -> ImpactReport;
```

BFS traversal from edited file through the code graph:
- Finds affected communities
- Identifies tests covering the edit
- Suggests co-change candidates (historically co-modified files)
- Generates risk alerts for high-impact modifications

### Wiki Backend

Concrete implementation of `WikiBackend` trait:
- `query()` — BM25 search over wiki pages in `.theo/wiki/`
- `ingest()` — Feeds runtime execution data (command results, tool outputs) into wiki learnings
- `generate()` — Full wiki regeneration from code graph

## Repository Knowledge as Architecture

Theo treats repository-local documentation as **executable architecture support** — the research principle is that *"what the agent cannot discover in-repo is operationally invisible"* (`docs/pesquisas/harness-engineering-openai.md`). The application layer is where these artifacts stop being "docs" and become inputs to runtime assembly, recovery, and guidance.

### Versioned (checked into git)

| Path | Role | Loaded by |
|---|---|---|
| `AGENTS.md` / `CLAUDE.md` / `theo.md` | **Table of contents** — short map pointing at deeper docs | System prompt assembly |
| `docs/architecture/` | Stable structural boundaries | On-demand, via GRAPHCTX + explicit @mentions |
| `docs/adr/` | Architecture Decision Records with rationale | On-demand |
| `docs/plans/` | Active and completed execution plans with progress logs | Session bootstrap + plan mode |
| `docs/roadmap/` | Product roadmap, consumed by `roadmap.rs` in pilot loop | `PilotLoop` |
| `docs/current/` vs `docs/target/` | What IS vs what's PLANNED — prevents agent hallucinating future state | On-demand |
| `docs/pesquisas/` | External references (papers, articles) — explicitly *isolated* from production | Not auto-loaded |

### Operational (runtime-owned, ignored by git)

| Path | Role | Written by |
|---|---|---|
| `.theo/theo.md` | Project summary generated by `theo init` | `theo-cli init` |
| `.theo/graph.bin` | Persisted code graph | `theo-engine-graph` |
| `.theo/wiki/` | Auto-generated knowledge base (BM25-searchable) | `theo-engine-retrieval` |
| `.theo/wiki/episodes/` | Episode summaries for cross-session memory | `theo-agent-runtime` |
| `.theo/state/{run_id}/session.jsonl` | Append-only session history | `StateManager` |
| `.theo/snapshots/` | Run checkpoints with checksum | `FileSnapshotStore` |
| `.theo/hooks/` | Pre/post/sensor shell hooks | User + `theo init` |
| `.theo/audit.jsonl` | Sandbox audit trail | `theo-governance` |
| `.theo/plans/` | In-progress plan edits (gate-controlled in Plan mode) | Agent in Plan mode |

### Plans as First-Class Artifacts

OpenAI's research treats execution plans as *first-class artifacts*, not scratchpads: *"Ephemeral lightweight plans are used for small changes, while complex work is captured in execution plans with progress and decision logs that are checked into the repository."* Theo mirrors this split:

- **Ephemeral** — `.theo/plans/` (runtime-owned, not versioned) for short work.
- **Durable** — `docs/plans/` and `docs/roadmap/` (versioned) for multi-session efforts. These are the artifacts the `PilotLoop` exits against.

The application layer is the only place in the system that touches both — it wires `docs/plans/` (read during bootstrap) and `.theo/plans/` (written during plan-mode sessions) into a single operational view.

---

## theo-api-contracts

Minimal shared contract layer between apps and the application crate. Contains **only** the frontend event enum. No logic, no infra dependencies — only `serde`.

### Parse-don't-validate at the boundary

OpenAI `harness-engineering-openai.md` §6 makes this an architectural requirement for agent-generated code: *"we require Codex to [parse data shapes at the boundary](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/)"*. The crate enforces the same invariant mechanically:

- Cross-surface contracts are typed once here (`FrontendEvent` today; future additions follow the same pattern).
- Apps consuming these contracts **must** deserialize into the enum — string-shaped ad-hoc JSON exchanges between Rust backend and frontend are disallowed.
- Any new contract goes through this crate or `theo-domain`; apps do not reimplement DTOs locally.

This keeps the agent's "legible surface area" (the shapes it can reason about from the repo alone) bounded and stable.

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FrontendEvent {
    Token { text: String },
    ToolStart { name: String, args: Value },
    ToolEnd { name: String, success: bool, output: String },
    PhaseChange { from: String, to: String },
    Done { success: bool, summary: String },
    Error { message: String },
    LlmCallStart { iteration: usize },
    LlmCallEnd { iteration: usize },
}
```

Used by `theo-desktop` for Tauri event bridge between Rust backend and React frontend.
