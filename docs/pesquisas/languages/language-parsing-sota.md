# Language Parsing (Tree-Sitter + LSP) — SOTA Research for AI Coding Agents

**Date:** 2026-04-29
**Domain:** Languages
**Target:** Raise score from 0.5 to 4.0
**Status:** Research complete

---

## Executive Summary

Tree-sitter is the universal parser for AI coding agents. Every major agent (Aider, Cline, OpenDev, Claude Code) uses it for AST-aware code chunking, symbol extraction, and structural navigation. However, tree-sitter has fundamental limitations: no type resolution, no cross-file semantics, no workspace-level renaming. OpenDev adopted LSP over tree-sitter for semantic analysis, and Claude Code shipped native LSP support in December 2025 (v2.0.74). The emerging consensus is a two-layer architecture: tree-sitter for fast, offline parsing (chunking, symbol extraction, syntax highlighting) + LSP for semantic operations (type resolution, references, rename). Theo Code supports 14 languages via tree-sitter grammars but lacks per-language quality benchmarks and LSP integration. This research maps the landscape and defines thresholds for SOTA.

---

## 1. Tree-Sitter: Strengths and Limitations

### 1.1 Why Tree-Sitter Dominates

| Strength | Detail |
|---------|--------|
| **Speed** | Incremental parsing: re-parses only changed ranges. Sub-millisecond updates. |
| **Offline** | No external server needed. Works without a running language server. |
| **Language breadth** | 305+ grammars available via tree-sitter-language-pack. |
| **Deterministic** | Same input always produces same AST. No LLM variability. |
| **Battle-tested** | Powers syntax highlighting in Neovim, Helix, Zed, VS Code (experimentally). |
| **Concrete syntax tree** | Preserves whitespace, comments, formatting -- essential for code editing. |

### 1.2 What Tree-Sitter Cannot Do

| Limitation | Impact on Coding Agent | Alternative |
|-----------|----------------------|-------------|
| **No type resolution** | Cannot distinguish overloaded methods by signature | LSP `textDocument/hover` |
| **No cross-file semantics** | Cannot resolve imports, find all references across files | LSP `textDocument/references` |
| **No workspace rename** | Cannot safely rename a symbol across the project | LSP `textDocument/rename` |
| **No inheritance resolution** | Cannot navigate class hierarchies, find implementations | LSP `textDocument/implementation` |
| **No scope analysis** | Cannot distinguish identically named symbols in different scopes | LSP or custom scope resolution |
| **Grammar quality varies** | Some languages have incomplete or buggy grammars | Per-language testing |

### 1.3 The Two-Layer Architecture

```
                    ┌─────────────────────────┐
                    │     AI Coding Agent      │
                    └─────────┬───────────────┘
                              │
              ┌───────────────┼───────────────┐
              │                               │
    ┌─────────▼─────────┐          ┌──────────▼──────────┐
    │    Tree-Sitter     │          │        LSP          │
    │  (fast, offline)   │          │  (semantic, online)  │
    ├────────────────────┤          ├─────────────────────┤
    │ AST-aware chunking │          │ Type resolution     │
    │ Symbol extraction  │          │ Cross-file refs     │
    │ Syntax validation  │          │ Workspace rename    │
    │ Code folding       │          │ Diagnostics         │
    │ Incremental reparse│          │ Call hierarchy      │
    └────────────────────┘          └─────────────────────┘
```

**When to use which:**

| Operation | Tree-Sitter | LSP | Rationale |
|----------|-------------|-----|-----------|
| Index all symbols in a file | Yes | No | Fast, no server needed |
| Chunk code for RAG embedding | Yes | No | Preserves structural boundaries |
| Find all references of a symbol | No | Yes | Requires cross-file resolution |
| Rename a variable across project | No | Yes | Requires scope + type awareness |
| Validate syntax after edit | Yes | No | Instant, incremental |
| Get type of expression | No | Yes | Requires type checker |
| Build repo-map for context | Yes | Augment | Tree-sitter for structure, LSP for types |

---

## 2. AST-Aware Chunking for RAG

### 2.1 cAST: Chunking via Abstract Syntax Trees

**Source:** arXiv:2506.15655 (June 2025)

cAST applies a recursive split-then-merge algorithm to convert AST tree structures into chunks aligned with syntactic boundaries. Results show consistent improvement across code generation tasks vs naive line-based or token-based chunking.

**Algorithm:**
1. Parse source file into AST via tree-sitter
2. Walk tree top-down; if a node's token count exceeds `max_chunk_size`, recurse into children
3. If a node fits within budget, emit it as a chunk
4. Merge adjacent small sibling nodes until they reach `min_chunk_size`
5. Preserve parent context (class name, function signature) as chunk metadata

### 2.2 Aider's Graph-Based Approach

Aider combines tree-sitter with NetworkX graph analysis:

1. Tree-sitter extracts definitions and references across 40+ languages
2. Build dependency graph with explicit edges (function A calls function B)
3. Apply PageRank with personalization factors for context weighting
4. Token-optimized repo-map using binary search to fit within budget
5. Disk-based caching with modification-time tracking

**Result:** Highest retrieval efficiency (4.3-6.5% context utilization) while preserving architectural awareness.

### 2.3 Supermemory code-chunk

Extracts semantic entities: functions, methods, classes, interfaces, types, enums, imports. Each chunk includes: signature, docstring, byte ranges, line ranges, parent context.

### 2.4 AFT: Tree-sitter Tools for AI Agents

Every operation addresses code by what it IS (function, class, call site, symbol) rather than where it sits in a file. Includes AST pattern search/replace with meta-variables, semantic search with cosine similarity, disk persistence.

---

## 3. LSP Integration in Coding Agents

### 3.1 Claude Code LSP (December 2025)

Claude Code shipped native LSP support in v2.0.74:
- Automatic diagnostics after every file edit
- Semantic code navigation (go-to-definition, find references)
- Type-aware understanding
- Result: fewer false positives, safer refactoring

### 3.2 LSAP: Language Server Agent Protocol

**Source:** github.com/lsp-client/LSAP

LSAP is an orchestration layer that composes atomic LSP operations into semantic interfaces aligned with agent cognitive logic. Key insight: LSP is designed for editors (atomic operations), LSAP is designed for agents (cognitive capabilities). An agent using LSP directly needs a dozen sequential interactions to get useful context.

Example LSAP operations:
- `understand_symbol`: combines hover + definition + references + type hierarchy into one call
- `safe_rename`: combines prepare_rename + references + preview + apply
- `diagnose_region`: combines diagnostics + code_actions + related_info

### 3.3 LSPRAG: LSP + Tree-sitter for Code Understanding

**Source:** arXiv:2510.22210 (October 2025)

Architecture: language-specific module (tree-sitter for AST parsing + feature extraction) + language-agnostic modules (LSP for context retrieval + code fixing). Results across Java, Go, Python: max line coverage increases of 174.55%, 213.31%, 31.57% respectively.

### 3.4 OpenDev's Approach

OpenDev uses tree-sitter for structural parsing but does NOT ship built-in LSP integration. The paper notes this as a deliberate trade-off: LSP requires a running language server per language, which conflicts with their "4.3ms startup, 9.4MB memory" goal.

---

## 4. Theo Code's 14 Supported Languages

### 4.1 Current Grammar Status

| Language | Grammar | Known Quality Issues | Priority |
|---------|---------|---------------------|----------|
| **Rust** | tree-sitter-rust | Good. Macros partially parsed. | P0 (self) |
| **Python** | tree-sitter-python | Good. Decorators, f-strings OK. | P0 |
| **TypeScript** | tree-sitter-typescript | Good. JSX/TSX supported. | P0 |
| **JavaScript** | tree-sitter-javascript | Good. ES2024+ features OK. | P0 |
| **Go** | tree-sitter-go | Good. Generics (Go 1.18+) supported. | P1 |
| **Java** | tree-sitter-java | **Moderate. Known duplication issues (Kilo Code).** | P1 |
| **C** | tree-sitter-c | Good. Preprocessor partially parsed. | P1 |
| **C++** | tree-sitter-cpp | Moderate. Templates complex. | P1 |
| **C#** | tree-sitter-c-sharp | Good. LINQ, async/await OK. | P2 |
| **Kotlin** | tree-sitter-kotlin-ng | **Known structural errors (Kilo Code).** | P2 |
| **Ruby** | tree-sitter-ruby | Good. Blocks, procs OK. | P2 |
| **PHP** | tree-sitter-php | Moderate. Mixed HTML/PHP tricky. | P2 |
| **Swift** | tree-sitter-swift | Moderate. Result builders complex. | P2 |
| **Scala** | tree-sitter-scala | Moderate. Scala 3 syntax evolving. | P2 |

### 4.2 New Grammars to Evaluate

| Language | Grammar | Maintainer | Quality Signal | Recommendation |
|---------|---------|------------|---------------|----------------|
| **Zig** | tree-sitter-zig | tree-sitter-grammars | Active maintenance, used by Zed | **ADD** -- growing systems language |
| **Elixir** | tree-sitter-elixir | elixir-lang (official) | Official maintainer, ABI v14, highlights+tags | **ADD** -- official quality |
| **Dart** | tree-sitter-dart | community | Moderate activity | **EVALUATE** -- Flutter ecosystem |
| **HCL/Terraform** | tree-sitter-hcl | mitchellh + tree-sitter-grammars | Real-world corpus, active | **ADD** -- infrastructure-as-code |

---

## 5. Per-Language Quality: What We Know

### 5.1 Symbol Extraction Completeness

No dedicated benchmark exists for tree-sitter symbol extraction recall. Evidence is indirect:

| Source | Finding |
|--------|---------|
| **Kilo Code (2025)** | Java and Kotlin parsing produces duplication and structural errors in `list_code_definition_names` |
| **RepoLens (2025)** | Tree-sitter symbol extraction + GPT-4o achieves 22% gain in Hit@k, 46% in Recall@k for file-level localization |
| **Aider** | 40+ languages parsed successfully; no per-language recall reported |
| **cAST** | Language-invariant chunking via AST; no per-language quality breakdown |

### 5.2 Known Per-Language Gaps

| Language | Issue | Severity | Workaround |
|---------|-------|----------|------------|
| **Java** | Duplicate symbols in extraction | Medium | Dedup by (name, byte_range) |
| **Kotlin** | Structural errors in parsing | Medium | Use kotlin-ng grammar |
| **C++** | Template instantiation not fully parsed | Low | Accept partial parsing |
| **Rust** | Macro expansion not parsed (opaque `macro_invocation` node) | Medium | Expand macros first, or accept |
| **PHP** | Mixed HTML/PHP files misparse at boundaries | Low | Extract PHP blocks first |
| **Scala** | Scala 3 `given`/`using` syntax may be incomplete | Low | Grammar updates needed |

### 5.3 Proposed Benchmark: Symbol Extraction Recall

To measure per-language quality, define a benchmark:

1. **Ground truth:** For each test file, manually annotate all top-level symbols (functions, classes, types, constants, imports)
2. **Extraction:** Run tree-sitter symbol extraction
3. **Metrics:**
   - **Recall@all:** fraction of ground-truth symbols found
   - **Precision:** fraction of extracted symbols that are genuine
   - **F1:** harmonic mean
4. **Test corpus:** 10 files per language, covering edge cases (generics, macros, nested classes, decorators)

---

## 6. Tree-Sitter vs LSP: Decision Matrix

### 6.1 When to Use Tree-Sitter Alone (Theo Code Current State)

- Fast symbol indexing during file discovery
- AST-aware code chunking for RAG embeddings
- Repo-map generation (function/class signatures)
- Syntax validation after agent edits
- Offline operation (no language server required)

### 6.2 When LSP Is Required (Future Enhancement)

- Cross-file "find all references" for safe refactoring
- Type-aware rename across workspace
- Diagnostics integration (compile errors, lint warnings)
- Call hierarchy for understanding code flow
- Implementation navigation (interface -> concrete)

### 6.3 Cost of LSP Integration

| Concern | Impact | Mitigation |
|---------|--------|------------|
| Startup latency | Language servers take 1-10s to start | Lazy initialization (only when needed) |
| Memory overhead | 50-500MB per language server | Share servers across sessions |
| Availability | Not all languages have mature LSP servers | Fall back to tree-sitter-only |
| Configuration | Each server needs project-specific config | Auto-detect from project files |

---

## 7. Thresholds for SOTA Level

### 7.1 Parsing Quality (Score 2.0 -> 3.0)

| Threshold | Target | Metric |
|----------|--------|--------|
| Symbol extraction recall per language | >= 0.85 for all 14 languages | Recall@all on test corpus |
| Symbol extraction precision | >= 0.90 for all 14 languages | Precision on test corpus |
| AST-aware chunking respects boundaries | 0 mid-function splits | Count on test corpus |
| Incremental reparse latency | <= 5ms for single-line edit | p99 latency |

### 7.2 Language Breadth (Score 3.0 -> 4.0)

| Threshold | Target | Metric |
|----------|--------|--------|
| Languages with passing recall >= 0.85 | >= 16 (14 current + Zig + Elixir) | Count |
| HCL/Terraform support | Grammar integrated and tested | Binary pass/fail |
| Per-language integration test suite | >= 5 test files per language | Count |
| Grammar update CI | Auto-update grammars monthly | Binary pass/fail |

### 7.3 Semantic Layer (Score 4.0 -> 5.0)

| Threshold | Target | Metric |
|----------|--------|--------|
| LSP integration for top 4 languages | Rust (rust-analyzer), Python (pyright), TS (tsserver), Go (gopls) | Count |
| Cross-file reference resolution | PASS for rename-safety check | E2E test |
| Diagnostics after edit | Automatic for LSP-supported languages | E2E test |
| LSAP-style compound operations | >= 3 (understand_symbol, safe_rename, diagnose_region) | Count |

---

## 8. Relevance for Theo Code

### 8.1 Immediate Actions

1. **Build per-language test corpus:** 10 files per language with annotated ground-truth symbols. Measure recall and precision for all 14 languages.
2. **Fix known issues:** Java dedup, Kotlin-ng migration if not done, Rust macro handling.
3. **Add Zig + Elixir grammars:** Both have official/high-quality tree-sitter support. Low effort, high value.
4. **Add HCL/Terraform grammar:** Infrastructure-as-code is increasingly common in agent workflows.

### 8.2 Architecture Decision

**Two-layer parser in Theo Code:**

```
theo-languages crate
  |-- TreeSitterParser: fast, offline, all 14+ languages
  |     |-- SymbolExtractor: functions, classes, types, imports
  |     |-- ChunkBuilder: AST-aware chunking with parent context
  |     |-- IncrementalParser: reparse only changed ranges
  |
  |-- LspBridge (future): lazy-initialized LSP clients
  |     |-- DiagnosticsWatcher: auto-run after edits
  |     |-- ReferenceResolver: cross-file find-references
  |     |-- SafeRenamer: preview + apply rename
  |
  |-- LanguageRegistry: maps file extensions -> parser + optional LSP
```

### 8.3 Key Trade-off

OpenDev chose NOT to integrate LSP to preserve 4.3ms startup and 9.4MB memory. Theo Code can follow the same path for now (tree-sitter only) and add LSP lazily for languages where semantic operations are requested. The threshold is: if the agent needs to rename a symbol or find all references, spin up the LSP server for that language on demand.

---

## Sources

- [cAST: AST-Aware Chunking for Code RAG](https://arxiv.org/html/2506.15655v1)
- [Exploratory Study of Code Retrieval in Coding Agents](https://www.preprints.org/manuscript/202510.0924)
- [LSAP: Language Server Agent Protocol](https://github.com/lsp-client/LSAP)
- [LSPRAG: LSP + RAG for Test Generation](https://arxiv.org/html/2510.22210v1)
- [Supermemory code-chunk](https://supermemory.ai/blog/building-code-chunk-ast-aware-code-chunking/)
- [AFT: Tree-sitter Tools for AI Agents](https://github.com/cortexkit/aft)
- [Tree-sitter Language Pack (305+ grammars)](https://github.com/Goldziher/tree-sitter-language-pack/)
- [Tree-sitter Elixir (official)](https://github.com/elixir-lang/tree-sitter-elixir)
- [Tree-sitter HCL](https://github.com/tree-sitter-grammars/tree-sitter-hcl)
- [Kilo Code tree-sitter issues](https://github.com/Kilo-Org/kilocode/issues/3891)
- [Claude Code LSP Integration](https://amirteymoori.com/lsp-language-server-protocol-ai-coding-tools/)
- [OpenDev Paper](https://arxiv.org/html/2603.05344v1)
- [RepoLens: Conceptual Knowledge for Issue Localization](https://arxiv.org/html/2509.21427v2)
