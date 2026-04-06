# GRAPHCTX — Code Intelligence Architecture

> Technical reference for Theo Code's code graph system.
> How it works, how it connects to the agent runtime, and how to extend it.

---

## Overview

GRAPHCTX gives the LLM a structural map of the codebase. Instead of dumping raw files into the context window, it builds a graph of symbols (functions, structs, imports), clusters them into semantically coherent communities, and serves the most relevant ones for any given query.

```
Source Files → Tree-Sitter → Symbols/Edges → CodeGraph → Communities → BM25 Ranking → Assembly → LLM
```

**Key numbers (Theo Code repo, 264 source files):**
- Cold build: ~714ms (release)
- Warm query: ~22ms (cached)
- Graph: 5,103 nodes, 9,566 edges, 84 communities
- Context budget: 10,649 tokens per query

---

## Architecture Layers

```
┌─────────────────────────────────────────────────────┐
│  CLI / REPL / Desktop / Agent Session               │  Entrypoints
├─────────────────────────────────────────────────────┤
│  theo-application                                    │  Orchestration
│  ├── GraphContextService (state machine)             │
│  ├── Pipeline (E2E orchestrator)                     │
│  └── extraction.rs (file walker + Tree-Sitter)       │
├─────────────────────────────────────────────────────┤
│  theo-engine-parser    │ theo-engine-graph            │  Engines
│  (Tree-Sitter, 14 langs) │ (CodeGraph, clustering)   │
│                        │                              │
│  theo-engine-retrieval                               │
│  (BM25, assembly, graph attention)                   │
├─────────────────────────────────────────────────────┤
│  theo-domain                                         │  Domain
│  ├── GraphContextProvider trait                       │
│  ├── EXCLUDED_DIRS (source of truth)                 │
│  └── tokens::estimate_tokens()                       │
└─────────────────────────────────────────────────────┘
```

### Dependency Direction

```
theo-domain         → (nothing — pure types)
theo-engine-*       → theo-domain
theo-engine-retrieval → theo-domain, theo-engine-graph
theo-application    → all engines + domain
apps/*              → theo-application (never engines directly)
```

---

## 1. Domain Layer (`theo-domain`)

### GraphContextProvider Trait

The contract that the agent runtime depends on (DIP):

```rust
// crates/theo-domain/src/graph_context.rs

#[async_trait]
pub trait GraphContextProvider: Send + Sync {
    async fn initialize(&self, project_dir: &Path) -> Result<(), GraphContextError>;
    async fn query_context(&self, query: &str, budget_tokens: usize) -> Result<GraphContextResult>;
    fn is_ready(&self) -> bool;
}
```

The runtime only sees this trait. The concrete implementation lives in `theo-application`.

### EXCLUDED_DIRS

Single source of truth for directories to skip during indexing:

```rust
// crates/theo-domain/src/graph_context.rs
pub const EXCLUDED_DIRS: &[&str] = &[
    "target", "node_modules", "vendor", "dist", "build",
    "__pycache__", ".venv", "venv", ".next", ".nuxt",
    ".gradle", ".mvn", "coverage", ".cache", ...
];
```

Used by both `extraction.rs` and `graph_context_service.rs`. Projects can add `.theoignore` for custom exclusions.

### Token Estimation

Unified function used by both compaction and assembly:

```rust
// crates/theo-domain/src/tokens.rs
pub fn estimate_tokens(text: &str) -> usize {
    let char_estimate = text.len() / 4;
    let word_estimate = (word_count as f64 * 1.3) as usize;
    char_estimate.max(word_estimate)
}
```

---

## 2. Engine Layer

### 2.1 Parser (`theo-engine-parser`)

Tree-Sitter-based parser supporting 14 languages at two quality tiers.

**Tier 1 (full extraction)**: 9 languages with dedicated extractors — symbols, imports, references, routes.
**Tier 2 (basic/file-level only)**: 5 languages — Kotlin, Scala, Swift, C, C++ — file nodes only, no symbol detail. Graph quality is significantly lower for these.

**What it extracts per file:**
- **Symbols**: functions, methods, classes, structs, enums, traits, interfaces
- **Imports**: use/import statements with resolved targets
- **References**: calls, extends, implements, type usage
- **Data Models**: structs/classes with field signatures

**Language support:**

| Family | Languages | Extractor |
|---|---|---|
| Rust | `.rs` | `extractors/rust.rs` |
| Python | `.py` | `extractors/python.rs` |
| TypeScript/JS | `.ts`, `.tsx`, `.js`, `.jsx` | `extractors/typescript.rs` |
| Go | `.go` | `extractors/go.rs` |
| Java/Kotlin | `.java`, `.kt` | `extractors/java.rs` |
| Ruby | `.rb` | `extractors/ruby.rs` |
| PHP | `.php` | `extractors/php.rs` |
| C# | `.cs` | `extractors/csharp.rs` |
| C/C++ | `.c`, `.cpp`, `.h` | `extractors/generic.rs` |
| Shell/YAML/TOML | `.sh`, `.yaml`, `.toml` | `extractors/generic.rs` |

**Parallelism**: Uses `rayon::par_iter` for concurrent file parsing. Thread-local parser cache avoids re-initialization.

### 2.2 Graph (`theo-engine-graph`)

The core data structure: `CodeGraph`.

**Node types:**
- `File` — one per source file
- `Symbol` — function, struct, trait, etc.
- `Import` — use/import statement
- `Type` — data model (struct with fields)
- `Test` — test function

**Edge types and weights:**

| Edge | Weight | Meaning |
|---|---|---|
| `Contains` | 1.0 | File contains symbol |
| `Calls` | 1.0 | Symbol calls another |
| `Imports` | 1.0 | File imports module |
| `Inherits` | 1.0 | Class extends/implements |
| `TypeDepends` | 0.8 | Uses a type |
| `Tests` | 0.7 | Test covers symbol |
| `CoChanges` | decay(age) | Files changed together in git |
| `References` | 1.0 | Generic reference |

**Co-change temporal decay:**

```
weight = exp(-lambda * days_since)
lambda = 0.01 → half-life ≈ 70 days
```

Recent co-changes weight more. Edges indexed via `HashMap<(src,tgt), idx>` for O(1) lookup.

#### Clustering

Three algorithms available:

| Algorithm | Complexity | Use case |
|---|---|---|
| **Louvain** | O(E) per pass | Symbol-level clustering |
| **Leiden** | O(E) per pass | Connected communities guarantee |
| **FileLeiden** | O(E) per pass | File-level (production default) |
| **LPA Seeded** | O(E) per pass | Subdivide mega-communities |

**Production flow (`hierarchical_cluster`):**

```
1. FileLeiden (resolution=0.5) → level-0 communities
2. merge_small_communities(min_size=3) → merge singletons
3. For each community > 30 members:
     → lpa_seeded(dir_seed_labels) → subdivide by crate/directory
     → Replace mega-community with subcommunities
4. name_community() → "theo-agent-runtime (30)" format
```

**Louvain O(E) optimization**: Pre-computed adjacency list + degree cache + incremental `comm_total_degree`. Iterates neighbors only (not all N nodes).

#### Persistence

Binary format via `bincode`:
- `graph.bin` — full CodeGraph
- `clusters.bin` — Vec<Community>
- `summaries.bin` — HashMap<String, CommunitySummary>
- `graph.manifest.json` — content hash for cache validation

**Cache validation**: `compute_project_hash()` hashes sorted `(path, mtime)` pairs. No false hits or misses.

### 2.3 Retrieval (`theo-engine-retrieval`)

#### MultiSignalScorer

Combines 4 signals (neural OFF, default) or 6 (neural ON):

| Signal | Weight (neural OFF) | Weight (neural ON) | What it measures |
|---|---|---|---|
| **BM25** | 0.30 | 0.25 | Term frequency relevance |
| **Semantic** | — | 0.20 | Neural embedding similarity |
| **File boost** | 0.25 | 0.20 | Best symbol name match ratio |
| **Graph attention** | 0.25 | 0.15 | Structural proximity (2-hop propagation) |
| **Centrality** | 0.10 | 0.10 | PageRank centrality |
| **Recency** | 0.10 | 0.10 | Git recency bonus |

Neural embeddings (AllMiniLML6V2, 384-dim) are opt-in via `THEO_NEURAL=1`. Default uses BM25 + graph signals only (~1s total vs ~29s with neural).

#### BM25

Standard BM25 with parameters k1=1.2, b=0.75. Tokenizer splits camelCase and snake_case identifiers:

```
verifyJwtToken → ["verify", "jwt", "token"]
handle_auth_request → ["handle", "auth", "request"]
```

This is genuinely better than plain grep for code search.

#### Graph Attention

Propagates relevance scores through the graph:

```
1. Score seed nodes (BM25 match on symbol names)
2. For 2 hops: propagate score * 0.5 to neighbors via edges
3. Aggregate scores per community
```

This finds structurally related code that doesn't match the query terms.

#### Assembly (Greedy Knapsack)

```rust
// crates/theo-engine-retrieval/src/assembly.rs
pub fn assemble_greedy(scored, graph, budget_tokens) -> ContextPayload {
    // 1. Filter: skip communities with <2 members (singletons)
    // 2. Build content: file paths + symbol signatures (deduped)
    // 3. Sort by density = score / tokens
    // 4. Greedy fill until budget exhausted
    // 5. Return items + exploration hints for excluded communities
}
```

**Output format** (what the LLM sees):

```markdown
# theo-agent-runtime (30)
## crates/theo-agent-runtime/src/run_engine.rs
pub struct AgentRunEngine { ... }
pub async fn execute(&mut self) -> AgentResult
fn louvain_phase1(assignment, node_ids, weight_map) -> bool

## crates/theo-agent-runtime/src/agent_loop.rs
pub struct AgentLoop { ... }
pub async fn run(&self, task: &str, project_dir: &Path) -> AgentResult
```

---

## 3. Orchestration Layer (`theo-application`)

### GraphContextService

State machine that manages the graph lifecycle:

```
Uninitialized ──initialize()──→ Building ──success──→ Ready
                                    │                    │
                                    └──failure──→ Failed  │
                                                         │
                                    query_context() ◄────┘
```

**Key behaviors:**
- `initialize()` returns immediately — build runs in `tokio::spawn_blocking`
- Cache checked synchronously first (content-hash validation)
- Queries during `Building` return empty context (graceful degradation)
- Build timeout: 60 seconds
- Double-init prevented via `AtomicBool` flag

### Pipeline

The E2E orchestrator connecting all engines:

```rust
// crates/theo-application/src/use_cases/pipeline.rs

impl Pipeline {
    pub fn build_graph(&mut self, files: &[FileData]) -> BridgeStats;
    pub fn add_git_cochanges(&mut self, repo_root: &Path) -> CoChangeStats;
    pub fn cluster(&mut self) -> &[Community];   // lazy scorer
    pub fn assemble_context(&mut self, query: &str) -> ContextPayload;
    pub fn update_file(&mut self, repo_root: &Path, file: &str) -> UpdateResult;
    pub fn impact_analysis(&self, file_path: &str) -> ImpactReport;
}
```

**Scorer is lazy**: `cluster()` does NOT build the MultiSignalScorer (avoids 28s fastembed). The scorer is built on first `assemble_context()` call.

**Incremental update**: `update_file()` removes old nodes/edges, re-parses the single file, merges back, and patches communities. Full re-cluster only if >10% edges changed.

### Extraction

File walker with 3 layers of protection:

```
Layer 1: .gitignore (via ignore crate, .git_ignore(true))
Layer 2: .gitignore fallback (.add_ignore() when .git/ absent)
Layer 3: EXCLUDED_DIRS hardcoded (theo-domain source of truth)
Layer 4: .theoignore custom (project-specific exclusions)
```

---

## 4. Integration with Agent Runtime

### How the LLM accesses GRAPHCTX

The `CodebaseContextTool` is registered in the tool registry:

```rust
// crates/theo-tooling/src/codebase_context/mod.rs

impl Tool for CodebaseContextTool {
    fn name(&self) -> &str { "codebase_context" }
    fn schema(&self) -> ToolSchema {
        // Parameters: query (string), budget_tokens (optional, default 4000)
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        // 1. Extract query from args
        // 2. Call ctx.graph_context.query_context(query, budget)
        // 3. Format ContextBlocks as markdown
        // 4. Return to LLM
    }
}
```

The LLM calls this tool when it needs codebase orientation:

```
LLM: "I need to understand how authentication works"
     → tool_call: codebase_context { query: "authentication flow" }

GRAPHCTX:
     → BM25 scores communities
     → Top 3: theo-infra-auth (7), theo-tooling (41), theo-domain (14)
     → Assembly: signatures of auth functions
     → Returns structured context

LLM: "I see verify_token() in infra-auth and the SandboxConfig in tooling.
      Let me read those files..."
     → tool_call: read { filePath: "crates/theo-infra-auth/src/lib.rs" }
```

### Initialization Flow

```
1. CLI starts (repl.rs / pilot.rs / run_agent_session.rs)
2. Check THEO_NO_GRAPHCTX env var → skip if set
3. GraphContextService::new() + initialize(project_dir)
4. Background: spawn_blocking → parse + build graph + cluster
5. Agent starts working immediately (GRAPHCTX building in parallel)
6. When LLM calls codebase_context tool:
   - If Ready: serve context from cached graph
   - If Building: return empty (LLM falls back to grep/glob)
   - If Failed: return error message
```

### System Prompt Integration

The agent's system prompt includes:

```
## Harness Context
You operate inside the Theo harness with [...] feedback loops.
Use generic tools (bash, read, write, edit, grep).
```

The `codebase_context` tool is available alongside `grep`, `glob`, `read`. The LLM decides when to use structured context (GRAPHCTX) vs direct search (grep).

---

## 5. Performance Profile

| Operation | Time (release) | Complexity |
|---|---|---|
| Cache hit (warm query) | ~22ms | O(files) for hash |
| File parsing (264 files, rayon) | ~200ms | O(total_LOC) |
| Graph building (bridge) | ~100ms | O(symbols + references) |
| Git co-changes (500 commits) | ~150ms | O(commits * files_per_commit) |
| Clustering (FileLeiden + LPA) | ~200ms | O(E) per pass |
| BM25 scoring | ~5ms | O(communities * query_terms) |
| Graph attention (2 hops) | ~5ms | O(E) |
| Greedy assembly | ~3ms | O(communities * log) |
| **Total cold build** | **~714ms** | |
| **Total warm query** | **~22ms** | |

### Optimization History

| Fix | Before | After | Speedup |
|---|---|---|---|
| Louvain O(N^2) → O(E) | 15s | 200ms | 75x |
| Co-change edge index | O(E^2) | O(F^2) | 60x |
| Leiden O(N^2) → O(E) | 8s | 200ms | 40x |
| Lazy scorer (skip fastembed) | 28s | 0ms | inf |
| Content-hash cache | rebuild always | skip if unchanged | inf |
| EXCLUDED_DIRS (skip target/) | 10,704 nodes | 5,103 nodes | 2x |
| Mega-community split (LPA) | "apps+crates (135)" | per-crate communities | quality |

---

## 6. Configuration

```rust
// GraphContextService
const BUILD_TIMEOUT: Duration = Duration::from_secs(60);
const LEIDEN_RESOLUTION: f64 = 1.0;
const MAX_FILES_TO_PARSE: usize = 500;

// Pipeline
PipelineConfig {
    token_budget: 16_384,        // max tokens in assembled context
    max_git_commits: 500,        // co-change history depth
    max_files_per_commit: 20,    // noise filter for large commits
    impact_bfs_depth: 3,         // impact analysis traversal depth
}

// Scorer weights (neural OFF)
BM25: 0.30, File boost: 0.25, Graph attention: 0.25,
Centrality: 0.10, Recency: 0.10

// Environment variables
THEO_NO_GRAPHCTX=1    // Disable GRAPHCTX entirely
THEO_NEURAL=1         // Enable neural embeddings (adds ~28s)
```

---

## 7. Failure Modes

| Scenario | Behavior | Recovery |
|---|---|---|
| Parse error in file X | File skipped, rest of graph builds | Automatic |
| Build timeout (>60s) | State → Failed | Agent uses grep/glob |
| Cache hash mismatch | Full rebuild triggered | Automatic |
| Query during Building | Empty context returned | LLM falls back to grep |
| No .gitignore + no .git | EXCLUDED_DIRS hardcoded protects | Automatic |
| Huge repo (>500 files) | MAX_FILES_TO_PARSE cap, primary language priority | Automatic |

---

## 8. Key Files Reference

| Component | File | Key Functions |
|---|---|---|
| Domain trait | `theo-domain/src/graph_context.rs` | `GraphContextProvider`, `EXCLUDED_DIRS` |
| Token estimation | `theo-domain/src/tokens.rs` | `estimate_tokens()` |
| Service | `theo-application/.../graph_context_service.rs` | `initialize()`, `query_context()` |
| Pipeline | `theo-application/.../pipeline.rs` | `build_graph()`, `cluster()`, `assemble_context()` |
| Extraction | `theo-application/.../extraction.rs` | `extract_repo()` |
| Graph model | `theo-engine-graph/src/model.rs` | `CodeGraph`, `Node`, `Edge` |
| Bridge | `theo-engine-graph/src/bridge.rs` | `build_graph()`, `file_node_id()` |
| Clustering | `theo-engine-graph/src/cluster.rs` | `hierarchical_cluster()`, `lpa_seeded()` |
| Co-changes | `theo-engine-graph/src/cochange.rs` | `update_cochanges()` |
| Persistence | `theo-engine-graph/src/persist.rs` | `save()`, `load()` |
| Search | `theo-engine-retrieval/src/search.rs` | `MultiSignalScorer::build()`, `score()` |
| Assembly | `theo-engine-retrieval/src/assembly.rs` | `assemble_greedy()` |
| Graph attention | `theo-engine-retrieval/src/graph_attention.rs` | `propagate_attention()` |
| Tool | `theo-tooling/src/codebase_context/mod.rs` | `CodebaseContextTool` |
