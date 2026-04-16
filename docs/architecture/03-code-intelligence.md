# 03 — Code Intelligence (`theo-engine-*`)

Read-only analysis of source code. Three crates forming a strict pipeline: **Parser → Graph → Retrieval**. This is the **GRAPHCTX** system — Theo Code's primary differentiator and Theo's implementation of a **context engine** in the sense of `docs/pesquisas/context-engine.md`.

In harness-engineering terms, the entire code intelligence bounded context is a **computational feedforward guide**: it increases the probability that the model gets it right in the first attempt by making the codebase legible before the model acts. It is deterministic, fast, and inspectable. It does **not** observe the agent's actions or produce feedback signals — those responsibilities live in `theo-governance` and the runtime's sensor system.

### Hard Constraints

The context engine is intentionally constrained:

- It **reads** and models the project.
- It **assembles** relevant context for the runtime, within a token budget.
- It **detects** structure, dependencies, clusters, and likely impact.
- It **does not** execute commands, edit files, or make business decisions.
- It **does not** call the LLM (except the optional wiki enrichment, which is a separate offline pipeline).

This is the same boundary the research draws (from `docs/pesquisas/context-engine.md`, a Python-era spec **adapted to the Rust workspace** — the prose stays authoritative, the implementation details are superseded by the module map below):

> *"Não executa código, não modifica arquivos, não faz inferências sobre business logic, não integra com LLM (apenas prepara contexto)."*

### Why Structural (Not Just Semantic)

Pure embedding-based retrieval collapses on large codebases because neighboring tokens don't imply neighboring *behavior*. GRAPHCTX layers lexical (BM25), semantic (dense vectors + rerank), and **structural** signals (call graph, imports, community clustering, git co-change) so the runtime can pick context that is actually load-bearing for the task, not just textually similar. This is Theo's primary leverage over pure RAG-based coding agents.

```
Source files
    │
    ▼
┌─────────────────────┐
│  theo-engine-parser │  Tree-Sitter AST → CodeModel IR
│  16 languages       │  Symbol table, import resolution
└─────────┬───────────┘
          │ FileData DTOs
          ▼
┌─────────────────────┐
│  theo-engine-graph  │  Multi-relational code property hypergraph
│  Community detect.  │  Louvain/Leiden/LPA clustering
│  Git co-changes     │  Temporal decay weighting
└─────────┬───────────┘
          │ CodeGraph + Communities
          ▼
┌─────────────────────┐
│ theo-engine-retrieval│  BM25 + Dense + Graph Attention + Reranker
│  Context assembly   │  Token-budget-aware greedy knapsack
│  Code Wiki          │  Auto-generated knowledge base
└─────────────────────┘
```

---

## theo-engine-parser

### Purpose
Parses source files using Tree-Sitter, extracts a rich Code Model IR (symbols, imports, references, call graphs, data models, routes), resolves cross-file references via a two-level symbol table, detects workspace layouts, and classifies file roles.

### Supported Languages (16)

TypeScript, TSX, JavaScript, JSX, Python, Java, Kotlin, C#, Go, PHP, Ruby, Rust, C, C++, Swift, Scala

### Key Types

| Type | Role |
|---|---|
| `CodeModel` | Top-level IR for a single file |
| `Symbol` | Function/class/method with name, kind, visibility, signature, doc, line range |
| `Reference` | Cross-symbol reference with `ResolutionMethod` + confidence score |
| `ImportInfo` | Import path, resolved module, imported symbols |
| `DataModel` | Entity/schema/DTO with typed fields |
| `SymbolTable` | Two-level (per-file + global) name resolution |
| `FileExtraction` | `CodeModel` + parse metadata |

### Resolution Methods (ranked by confidence)

| Method | Confidence | Mechanism |
|---|---|---|
| `ImportBased` | 0.95 | Follows import statement |
| `SameFile` | 0.90 | Symbol defined in same file |
| `GlobalUnique` | 0.80 | Single definition in entire project |
| `ImportKnown` | 0.75 | Import exists but target not parsed |
| `External` | 0.65 | Resolved to external package |
| `GlobalSameDir` | 0.60 | Same directory, not unique globally |
| `GlobalAmbiguous` | 0.40 | Multiple definitions, no disambiguator |
| `Unresolved` | 0.00 | Could not resolve |

### Language Behavior Trait

Strategy pattern for language-specific conventions:
```rust
pub trait LanguageBehavior: Send + Sync {
    fn module_separator(&self) -> &str;          // "::" for Rust, "." for Python
    fn source_roots(&self) -> &[&str];           // "src" for Rust, "" for Python
    fn is_test_symbol(&self, name: &str) -> bool;
    fn is_stdlib_module(&self, name: &str) -> bool;
    // ... 8 more methods
}
```

9 implementations: TypeScript, Python, Java, C#, Go, PHP, Ruby, Rust, Generic (fallback).

### Workspace Detection

Detects monorepo workspace layouts: `Pnpm`, `Npm`, `Cargo`, `Go`, `Uv`. Returns `WorkspaceLayout` with package list and root config path.

---

## theo-engine-graph

### Purpose
Builds and maintains an in-memory, persisted **Multi-relational Code Property Hypergraph** (MCPH) from parser output. Tracks structural relationships, temporal co-change from git history, and supports community detection. Optional SCIP integration for exact cross-file symbol resolution.

### Edge Types (8)

| Type | Default Weight | Meaning |
|---|---|---|
| `Contains` | 1.0 | File contains symbol |
| `Calls` | 1.0 | Function calls function |
| `Imports` | 1.0 | File imports from file |
| `Inherits` | 1.0 | Class/struct inherits from |
| `TypeDepends` | 0.8 | Type dependency |
| `Tests` | 0.7 | Test file tests production file |
| `CoChanges` | dynamic | Files historically changed together (temporal decay) |
| `References` | 1.0 | General symbol reference |

### Node Types

`File`, `Symbol`, `Import`, `Type`, `Test` — each carrying name, file_path, signature, kind, line range, doc, last_modified.

### Community Detection

Four algorithms, selectable per use case:

| Algorithm | Best for | Deterministic |
|---|---|---|
| `Louvain` | General purpose, fast | No (order-dependent) |
| `Leiden { resolution }` | Tunable granularity | No |
| `FileLeiden` | File-level clustering (ignores symbols) | No |
| `LPA` (seeded) | Stable assignments across runs | Semi (seed-dependent) |

Returns `ClusterResult { communities: Vec<Community>, modularity: f64 }`.

### Git Co-Change (cochange.rs)

Extracts file co-occurrence from `git log`, applies temporal decay weighting, and injects `CoChanges` edges into the graph. Used for impact analysis and retrieval boosting.

### Persistence

`CodeGraph` serialized via bincode to `.theo/graph.bin`. Atomic write (write to `.tmp`, rename).

### Bridge (bridge.rs)

4-pass algorithm that converts parser `FileData` DTOs into graph nodes and edges:
1. File nodes
2. Symbol nodes + Contains edges
3. Import/call edges
4. Reference/type dependency edges

### Optional: SCIP Integration

Feature-gated (`scip`). Reads `.scip` index files for exact cross-file symbol resolution. Merges SCIP edges into the graph, upgrading Tree-Sitter-approximate edges to SCIP-exact.

---

## theo-engine-retrieval

### Purpose
Multi-stage retrieval pipeline over the code graph. Layers BM25 full-text search, dense neural embeddings, graph attention propagation, cross-encoder reranking, and greedy context assembly within a token budget. Also includes the deterministic Code Wiki generation pipeline.

From the harness perspective, retrieval has one job: provide **high-signal, budget-aware, structurally grounded context** so the runtime can act with fewer guesses.

### Retrieval Pipeline

```
Query
  │
  ├─→ BM25 over community summaries ──────────────┐
  │     (k1=1.2, b=0.75)                          │
  │                                                │
  ├─→ File-level BM25 (FileBm25) ─────────────────┤
  │                                                │ RRF Fusion
  ├─→ Dense vector search (fastembed ONNX) ────────┤ (k=60)
  │     (optional: dense-retrieval feature)        │
  │                                                │
  ├─→ Graph attention propagation ─────────────────┤
  │     (double-buffered, configurable hops/damping)│
  │                                                │
  └─→ Cross-encoder reranking ─────────────────────┘
        (Jina Reranker v2 Base Multilingual, ~10ms/doc)
        (optional: reranker feature)
          │
          ▼
  MultiSignalScorer
    bm25: 0.55, file_boost: 0.30, centrality: 0.05, recency: 0.10
          │
          ▼
  Greedy knapsack context assembly
    (within token budget)
          │
          ▼
  ContextPayload { blocks, total_tokens, budget_tokens }
```

### Feature Flags

| Flag | Adds | Cost |
|---|---|---|
| (default) | BM25 + graph attention + assembly | Minimal |
| `tantivy-backend` | Tantivy full-text engine + hybrid RRF | +10MB binary |
| `dense-retrieval` | Neural embeddings (fastembed ONNX) + cache | +50MB model |
| `reranker` | Cross-encoder reranking | +100MB model |
| `scip` | SCIP exact resolution via graph crate | Negligible |

### Scoring Signals

| Signal | Source | Weight |
|---|---|---|
| `Bm25Content` | Text match in summaries | 0.55 |
| `Bm25Path` | File path match | Part of file_boost |
| `SymbolMatch` | Exact symbol name match | Part of file_boost |
| `CommunityScore` | Community-level relevance | Part of bm25 |
| `CoChange` | Git co-change frequency | Part of recency |
| `GraphProximity` | Graph attention score | 0.05 |
| `AlreadySeenPenalty` | Dedup across queries | Negative |
| `RedundancyPenalty` | Content similarity | Negative |

### IR Metrics (metrics.rs)

Built-in evaluation: `recall_at_k`, `precision_at_k`, `hit_at_k`, `mrr`, `ndcg_at_k`, `average_precision`, `dep_coverage`, `missing_dep_rate`.

### Code Wiki (wiki/)

Deterministic pipeline that generates an Obsidian-like knowledge base from the code graph:

```
CodeGraph + Communities
    │
    ▼
  wiki/generator.rs     → WikiDoc pages (per community, per crate)
    │
    ▼
  wiki/renderer.rs      → Markdown with frontmatter (authority tiers, source refs)
    │
    ▼
  wiki/persistence.rs   → Write to .theo/wiki/ (with orphan cleanup, WAL, GC)
    │
    ▼
  wiki/lookup.rs        → BM25 search over wiki pages
    │
    ▼
  wiki/runtime.rs       → Runtime insight ingestion (command results → wiki learnings)
    │
    ▼
  wiki/lint.rs          → Wiki health checks (stale pages, broken links)
```

Authority tiers: `Canonical` > `Derived` > `Inferred` > `Stale`.

### Embeddings

| Component | Model | Purpose |
|---|---|---|
| `NeuralEmbedder` | fastembed ONNX (CPU) | Dense vector embeddings |
| `TfidfModel` | Custom | Sparse TF-IDF for fallback |
| `TurboQuantizer` | int8 scalar | Compress dense vectors 4x |
| `EmbeddingCache` | bincode on disk | Avoid recomputation |

---

## Performance & Quality Targets

Derived from `docs/pesquisas/context-engine.md` and adapted to the Rust workspace. These are the numbers the context engine is designed against and should be validated by the benchmark harness (`apps/theo-benchmark`).

### Analysis Time

| Project size | Target |
|---|---|
| Small (<50 files) | 1–3 s |
| Medium (50–200 files) | 3–8 s |
| Large (200–500 files) | 8–15 s |
| Very large (>500 files) | 15–30 s |

### Memory Footprint

| Component | Target |
|---|---|
| Base overhead | ~50 MB |
| Per file analyzed | ~100 KB |
| Graph storage | ~200 KB per 100 files |
| Dense vector cache (int8) | ~50 MB for 10k symbols |

### Retrieval Quality (IR metrics)

| Metric | Target |
|---|---|
| `recall@10` on canonical queries | > 0.70 |
| `hit@5` on file-localized queries | > 0.80 |
| Graph-attention lift vs BM25-only | +10% on `ndcg@10` |

### Cache

The graph cache (`.theo/graph.bin`) and embedding cache must:

- Invalidate per-file on mtime change (not wholesale).
- Survive process crash via atomic write (tmp + rename).
- Be regeneratable from source alone — the cache is never authoritative.

## Context Budget Invariants

Context assembly obeys four invariants that the runtime depends on:

1. **Token budget is hard** — the assembler never emits a payload that exceeds `budget_tokens`.
2. **Deterministic under fixed input** — same graph + same query produces the same blocks (required for reproducibility and benchmark stability).
3. **Source-traceable** — every block carries a `source_ref` so the runtime can cite where context came from and the governance layer can check provenance.
4. **Redundancy-penalized** — repeated content across blocks is demoted, so the budget buys maximum information.

These invariants are why the context engine is safe to call every turn: the cost is bounded and the output is legible.
