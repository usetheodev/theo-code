# Code Wiki

A persistent, compounding knowledge base generated from your code graph. Inspired by [Karpathy's LLM Wiki](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f) pattern.

Instead of re-deriving knowledge on every query (like RAG), the wiki **compiles once and compounds over time** — every code change triggers incremental updates, and every query enriches the cache for future lookups.

## Architecture

Three layers, following the Karpathy pattern:

```
  Raw Sources          The Wiki              The Schema
  ───────────          ────────              ──────────
  CodeGraph     →    .theo/wiki/     ←    wiki.schema.toml
  (Tree-Sitter        modules/*.md          (user-editable
   + Leiden)           cache/*.md            conventions)
                       index.md
                       overview.md
                       architecture.md
```

**Raw Sources** — The `CodeGraph` built by Tree-Sitter parsing (9 languages) + Leiden community detection. Immutable input. The wiki reads from it but never modifies it.

**The Wiki** — LLM-generated and deterministic markdown pages in `.theo/wiki/`. The system owns this layer entirely. It creates pages, updates them incrementally when code changes, and maintains cross-references via `[[wiki-links]]`.

**The Schema** — `wiki.schema.toml` defines how the wiki is structured: bounded-context groups, page thresholds, naming conventions. User-editable. The system reads it on every generation.

## Modules

```
wiki/
  model.rs        — Canonical IR: WikiDoc, WikiManifest, WikiSchema, SourceRef
  generator.rs    — CodeGraph → Vec<WikiDoc> (zero LLM cost, deterministic)
  renderer.rs     — WikiDoc → Obsidian-compatible markdown
  persistence.rs  — Disk I/O, cache invalidation, schema load/save
  lookup.rs       — BM25 search over wiki pages (<5ms)
  lint.rs         — Health check: orphans, broken links, stale cache, large pages
```

## Knowledge Compounding Loop

The core innovation: the wiki grows with **usage**, not just with code changes.

```
    Code changes
         │
         ▼
    ┌────────────────┐
    │ Ingest Loop    │  generate_wiki_incremental()
    │ (deterministic)│  only changed communities + 2-hop dependents
    └───────┬────────┘
            ▼
    ┌────────────────────────────────────┐
    │         .theo/wiki/                │
    │  modules/  (from code graph)       │
    │  cache/    (from queries)          │
    │  overview/ (from LLM enrichment)   │
    └───────┬────────────────────────────┘
            │
    ┌───────▼────────────┐
    │  query_context()   │
    │                    │
    │  Layer 0: Wiki BM25│──── HIT → instant response (<5ms)
    │       │            │
    │       └── MISS     │
    │           │        │
    │  Layer 1-2: RRF    │  (BM25 + Tantivy + Dense)
    │           │        │
    │           └─ write-back ──► cache/{slug}.md
    └────────────────────┘           │
                                     │
         Next similar query ─────────┘  (Layer 0 HIT)
```

**Each query that passes through the full RRF pipeline writes back a cache page.** The next query on the same topic is answered from the wiki in <5ms without the pipeline.

## Operations

### Ingest (automatic)

Triggered on `GraphContextService::initialize()`. Checks graph hash — if stale:

1. Computes per-community hash (`compute_community_hash`)
2. Compares with `WikiManifest.page_hashes` from previous run
3. Regenerates only changed pages + 2-hop dependents
4. Cleans up orphaned pages (from Leiden re-clustering)
5. Writes updated manifest with new `page_hashes`
6. Falls back to full regeneration if >50% communities changed

**Cost:** Zero LLM. Pure graph traversal. ~1.4s for 119 pages (full), <100ms incremental.

### Query (automatic)

`query_context()` checks the wiki first:

1. `lookup()` runs BM25 over `modules/` + `cache/` directories
2. Title tokens get 3x boost. Content truncated at 3000 chars for speed.
3. If top result confidence >= 0.6 → return directly (skip RRF)
4. If miss → run full RRF pipeline → write result to `cache/{slug}.md`

Cache pages include YAML frontmatter with `graph_hash` for staleness detection:

```yaml
---
graph_hash: 1695828210123601777
generated_at: "1775631860"
query: "authentication oauth device flow"
---
```

When the graph changes, stale cache pages are overwritten on next write-back.

### Lint (on demand)

`lint(wiki_dir)` detects 5 categories of issues:

| Check | What it detects |
|-------|----------------|
| Orphan pages | No inbound `[[wiki-links]]` from any other page |
| Broken links | `[[target]]` pointing to non-existent slugs |
| Large pages | Exceeding token threshold (configurable via schema) |
| Empty sections | `## Header` with no content before next header |
| Stale cache | Cache pages with `graph_hash` different from manifest |

### LLM Enrichment (opt-in)

Two separate enrichment paths, both async and never on the hot path:

- `enrich_wiki()` — Adds narrative prose ("What This Module Does", "Key Concepts") to existing module pages while preserving all deterministic sections.
- `generate_highlevel_pages()` — Generates overview, architecture, getting-started, and concept pages via LLM.

## Wiki Schema

User-configurable at `.theo/wiki/wiki.schema.toml`:

```toml
[project]
name = "my-project"
description = "A code analysis tool"

[[groups]]
name = "Core"
prefixes = ["my-core", "my-lib"]

[[groups]]
name = "API"
prefixes = ["my-api", "my-server"]

[pages]
min_file_count = 1      # minimum files for a community to get a page
max_token_size = 5000    # lint threshold for large page warning
```

If the file doesn't exist, defaults are used (8 groups matching `theo-*` prefixes). The schema is created automatically on first wiki generation.

## Data Model

Every claim in the wiki has **provenance** — a `SourceRef` tracing back to exact file, symbol, and line range:

```rust
SourceRef {
    file_path: "src/auth.rs",
    symbol_name: Some("verify_token"),
    line_start: Some(10),
    line_end: Some(30),
}
// Renders as: src/auth.rs:10-30
```

The canonical IR (`WikiDoc`) contains:

| Section | Content | Provenance |
|---------|---------|------------|
| Entry Points | Functions with no inbound calls from within the community | Per-symbol SourceRef |
| Public API | Top 15 exported symbols with signatures | Per-symbol SourceRef |
| Files | File list with symbol counts | Per-file SourceRef |
| Dependencies | Cross-community `[[wiki-links]]` (Imports/Calls/TypeDepends) | Edge type |
| Call Flow | BFS 2-hop call chains within the community | Per-step SourceRef |
| Test Coverage | Tested/total from graph's NodeType::Test + EdgeType::Tests | Per-function |

## Concept Detection

Communities are grouped into high-level concepts using two strategies:

1. **Topology-based** (primary): Builds an adjacency matrix from cross-community dependencies. Communities with >= 3 mutual edges are merged via union-find into concept clusters.

2. **Prefix-based** (fallback): Groups unclustered modules by shared crate prefix (e.g., `theo-engine-*` → "Code Intelligence Engine").

Detected concepts feed into `generate_highlevel_pages()` for LLM-generated concept pages.

## Disk Layout

```
.theo/wiki/
  wiki.schema.toml          # user-editable conventions
  wiki.manifest.json         # cache invalidation (graph_hash, page_hashes)
  index.md                   # hierarchical TOC grouped by bounded context
  overview.md                # LLM-generated project overview
  architecture.md            # LLM-generated architecture page
  getting-started.md         # LLM-generated onboarding guide
  log.md                     # append-only event log (grep-friendly)
  modules/                   # one .md per community (deterministic)
    theo-engine-retrieval-19.md
    theo-infra-auth-7.md
    ...
  cache/                     # write-back from queries (compounding)
    authentication-oauth-device-flow.md
    ...
  concepts/                  # LLM-generated concept pages
    concept-code-intelligence-engine.md
    ...
```

## Rendering

The wiki can be rendered as a self-contained HTML page using `theo-marklive`:

```bash
cargo run -p theo-marklive -- .theo/wiki/ -o wiki.html --title "My Project Wiki"
```

Features: dark theme (theo-desktop design system), sidebar navigation, client-side search, mermaid diagram rendering, visual overview components (hero section, feature cards).

## Tests

51 unit tests across all modules:

```bash
cargo test -p theo-engine-retrieval --lib wiki
```

E2E and benchmarks (requires full project parse):

```bash
# Full wiki generation + validation
cargo test -p theo-engine-retrieval --test benchmark_suite -- wiki_e2e --ignored --nocapture

# Knowledge compounding loop demonstration
cargo test -p theo-engine-retrieval --test benchmark_suite -- wiki_knowledge_loop --ignored --nocapture
```
