# Wiki System — SOTA Research for AI Coding Agents

**Date:** 2026-04-29
**Domain:** Wiki
**Target:** Raise score from 0.5 to 4.0
**Status:** Research complete

---

## Executive Summary

Andrej Karpathy's LLM Wiki pattern (April 2026, 16M+ views, 5K+ stars) crystallized a paradigm shift: from stateless RAG retrieval to stateful, compounding knowledge. The core analogy is compilation: raw sources are "source code," the LLM compiles them into a structured wiki "artifact," and queries run against the compiled artifact. Multiple implementations have emerged (llm-wiki-compiler, llm-wiki, second-brain), all converging on a three-layer architecture (raw -> wiki -> query) with SHA-256 incremental compilation and lint-based self-healing. For Theo Code, the wiki system is the bridge between "agent that forgets everything" and "agent that compounds codebase knowledge." The wiki should be compiled from codebase analysis (not chat history -- that is memory), serve as an additional document root for the retrieval pipeline (RRF), and self-heal via lint rules.

---

## 1. Karpathy LLM Wiki Pattern

### 1.1 The Core Insight

> "Stop re-deriving, start compiling. RAG retrieves and forgets. A wiki accumulates and compounds."

Most people's experience with LLMs and documents looks like RAG: upload files, retrieve chunks at query time, generate an answer. The LLM rediscovers knowledge from scratch on every question. There is no accumulation.

The compilation analogy: when you write source code, a compiler transforms it into an optimized binary artifact. You compile once, distribute the artifact, run it efficiently on demand. The LLM Wiki does the same: process raw sources once into a structured wiki, then query the wiki.

### 1.2 Three-Layer Architecture

```
sources/          (immutable raw inputs -- never modified by LLM)
  |
  v  [ingest: LLM compilation]
  |
wiki/             (LLM-generated pages -- one per concept, cross-referenced)
  |-- index.md    (master index with all page links + tags)
  |-- concept-a.md
  |-- concept-b.md
  |
  v  [query: LLM navigates index -> reads pages -> synthesizes]
  |
answers           (ephemeral or filed as permanent wiki pages)
```

### 1.3 Three Operations

| Operation | Purpose | Cost |
|----------|---------|------|
| **Ingest** | Process new sources into wiki pages | LLM calls per source chunk |
| **Query** | Ask questions against compiled wiki | LLM reads index + relevant pages |
| **Lint** | Health-check: orphans, broken links, contradictions | Structural (free) + LLM-powered (deep) |

### 1.4 Key Design Principles

1. **Sources are immutable:** The LLM reads but never modifies raw sources. They are ground truth. The wiki can always be re-derived from scratch.
2. **One page per concept:** Each wiki page represents a single concept with cross-references via `[[wikilinks]]`.
3. **Provenance tracking:** Every claim in the wiki traces back to its source file(s) via frontmatter.
4. **Knowledge compounds:** Valuable query answers optionally get filed as permanent wiki pages.
5. **Git as version control:** The wiki is plain markdown in git. Diffs show exactly what the LLM changed.

---

## 2. llm-wiki-compiler: Two-Phase Pipeline

### 2.1 Architecture

**Source:** github.com/atomicmemory/llm-wiki-compiler

The pipeline flows:

```
sources/ -> SHA-256 hash check -> [Phase 1: Extract] -> [Phase 2: Generate] -> wiki/
                                        |                       |
                                 Extract concepts         Generate pages
                                 from each source         with [[wikilinks]]
                                                          + frontmatter
                                                               |
                                                          Resolve wikilinks
                                                               |
                                                          Update index.md
```

**Phase 1 (Extract):** For each source file, extract all concepts, entities, and claims. Output: structured JSON with concept names, relationships, and source provenance.

**Phase 2 (Generate):** For each extracted concept, generate a wiki page. Merge information from multiple sources about the same concept. Resolve `[[wikilinks]]` to actual page slugs. Write YAML frontmatter with metadata.

### 2.2 SHA-256 Incremental Compilation

Each source file is hashed with SHA-256. The hash is stored in the wiki page frontmatter under `source_hash` or `sources[]` with their hashes. On subsequent runs:

1. Hash all source files
2. Compare against stored hashes
3. Skip unchanged sources (cache hit)
4. Re-extract and re-generate only for changed/new sources

**Target cache hit rate:** >= 80% for typical development sessions (most files unchanged).

### 2.3 Frontmatter Format

```yaml
---
id: "a1b2c3d4"           # SHA-256 first 8 chars
title: "Concept Name"
domain: "architecture"
sources:
  - file: "src/main.rs"
    hash: "e5f6a7b8..."
    lines: "42-67"
  - file: "docs/design.md"
    hash: "c3d4e5f6..."
confidence: 0.85           # optional epistemic metadata
status: "current"          # current | outdated | conflict | review-needed
tags: ["rust", "architecture", "error-handling"]
created: "2026-04-29"
updated: "2026-04-29"
---
```

All fields are optional. Existing pages without them continue to work.

---

## 3. Wiki Lint: Self-Healing Rules

### 3.1 Core Lint Rules (6 Structural)

These rules run without LLM calls -- pure structural analysis:

| # | Rule | Description | Auto-Fix |
|---|------|------------|----------|
| 1 | **Broken wikilinks** | `[[links]]` pointing to non-existent pages | Suggest closest match or flag for creation |
| 2 | **Orphan pages** | Pages with zero inbound links from other pages | Link from related pages or flag for review |
| 3 | **Duplicate pages** | Pages covering the same concept (fuzzy title match) | Merge into canonical page |
| 4 | **Empty pages** | Pages with no meaningful content (only frontmatter) | Delete or flag for generation |
| 5 | **Broken citations** | Frontmatter `sources[]` pointing to non-existent files | Flag as stale |
| 6 | **Missing frontmatter** | Pages without required YAML frontmatter | Generate frontmatter from content |

### 3.2 Extended Lint Rules (LLM-Powered)

These require LLM calls and are run with `--deep`:

| # | Rule | Description | Cost |
|---|------|------------|------|
| 7 | **Contradictions** | Two pages make conflicting claims about the same concept | Compare tag-overlapping pairs |
| 8 | **Stale content** | Source file changed but wiki page not updated | Hash comparison |
| 9 | **Missing cross-references** | Concept mentioned in text but not linked | NLP entity matching |
| 10 | **Claim verification** | Claims that cannot be traced to any source | Source provenance check |
| 11 | **Summary accuracy** | Page summary does not reflect page content | LLM evaluation |

### 3.3 Self-Healing Loop

```
lint --check   ->  identify issues
lint --fix     ->  auto-fix structural issues (1-6)
lint --deep    ->  LLM-powered analysis (7-11)
lint --deep --fix -> auto-fix + regenerate stale pages
```

From LLM Wiki v2: "Self-healing is emphasized -- the lint operation should automatically fix what it can. Orphan pages get linked or flagged."

### 3.4 Quality Scoring

Every page gets a quality score:
- Well-structured? (headers, lists, code blocks)
- Cites sources? (frontmatter provenance)
- Consistent with rest of wiki? (no contradictions)
- Content below threshold -> flagged for review or rewritten

---

## 4. Wiki vs Memory Boundary

### 4.1 Clear Separation

| Aspect | Wiki | Memory |
|--------|------|--------|
| **Content source** | Codebase analysis (files, architecture, patterns) | Agent session experience (decisions, errors, learnings) |
| **Compilation trigger** | File changes (git diff) | Session end or milestone |
| **Format** | One page per concept, cross-referenced | Episode records, procedural rules |
| **Query pattern** | "How does module X work?" | "What did we try last time?" |
| **Persistence** | Permanent (git-tracked) | Curated (forgetting curve) |
| **Ownership** | Codebase-scoped | Agent-scoped |

### 4.2 Integration Points

```
                    ┌──────────────┐
                    │  Agent Query │
                    └──────┬───────┘
                           │
              ┌────────────┼────────────┐
              │                         │
    ┌─────────▼─────────┐    ┌─────────▼─────────┐
    │   Wiki Backend     │    │  Memory Backend    │
    │  (codebase know.)  │    │  (agent experience)│
    ├────────────────────┤    ├────────────────────┤
    │ architecture.md    │    │ episodic: session_42│
    │ error-handling.md  │    │ procedural: "always │
    │ api-design.md      │    │  run tests before   │
    │ module-x.md        │    │  submitting PRs"    │
    └─────────┬──────────┘    └─────────┬──────────┘
              │                         │
              └────────────┬────────────┘
                           │
                    ┌──────▼───────┐
                    │  RRF Merger  │
                    │ (rank fusion)│
                    └──────────────┘
```

The wiki serves as an additional document root for the retrieval pipeline. When the agent queries for context, RRF merges results from: (1) direct file search, (2) wiki pages, (3) memory records.

---

## 5. Compilation Cost Analysis

### 5.1 LLM Calls Per Source

| Phase | Calls | Typical Size | Model |
|-------|-------|-------------|-------|
| Extract concepts | 1 per source chunk | ~4K tokens input | Haiku/Flash (cheap) |
| Generate page | 1 per concept | ~2K tokens input + ~1K output | Haiku/Flash |
| Resolve wikilinks | 1 batch call | All slugs + context | Haiku/Flash |
| Deep lint (contradictions) | 1 per page pair | ~4K tokens | Haiku/Flash |

### 5.2 Cost Estimation

For a medium codebase (500 source files, ~100 wiki pages):

| Operation | Calls | Input Tokens | Output Tokens | Haiku Cost |
|----------|-------|-------------|---------------|------------|
| Full compilation | ~500 extract + ~100 generate = 600 | ~2.4M | ~100K | ~$0.75 |
| Incremental (20% changed) | ~100 extract + ~20 generate = 120 | ~480K | ~20K | ~$0.15 |
| Lint (structural) | 0 LLM calls | 0 | 0 | $0.00 |
| Lint (deep) | ~50 pair comparisons | ~200K | ~25K | ~$0.10 |

### 5.3 Chunk-Size Limits

Source files must be chunked to fit model context windows:

| Model | Context Window | Recommended Chunk | Chunks per File |
|-------|---------------|-------------------|-----------------|
| Haiku 3.5 | 200K | 4K tokens | 1-3 per file |
| Flash 2.0 | 1M | 8K tokens | 1-2 per file |
| Sonnet 4 | 200K | 4K tokens | 1-3 per file |

**Optimization:** Use tree-sitter AST-aware chunking (section from language-parsing-sota.md) to split source files at function/class boundaries rather than arbitrary token counts.

---

## 6. Compilation Determinism

### 6.1 The Problem

LLM outputs are non-deterministic by default. Two compilations of the same source may produce different wiki pages, causing unnecessary git diffs and confusing developers.

### 6.2 Mitigations

| Strategy | Effect | Trade-off |
|---------|--------|-----------|
| `temperature=0` | Minimizes randomness | Slightly less creative extractions |
| `seed` parameter | Reproducible outputs (when supported) | Not all providers support seeds |
| Structured output (JSON mode) | Reduces formatting variation | Constrains output format |
| Hash-based skip | Don't regenerate if source unchanged | Stale pages if prompt changes |
| Diff-based commit | Only commit meaningful changes | Requires diff filtering |

### 6.3 Practical Approach

1. Use `temperature=0` + `seed=42` for all compilation calls
2. After generation, normalize whitespace and formatting
3. Compare against existing page content with fuzzy match
4. Only write if semantic content changed (not just formatting)
5. Track compilation prompt version in frontmatter -- re-compile all when prompt changes

---

## 7. Implementation Landscape

### 7.1 Reference Implementations

| Project | Architecture | Incremental | Lint | Self-Healing |
|---------|-------------|------------|------|-------------|
| **atomicmemory/llm-wiki-compiler** | Two-phase extract->generate | SHA-256 | Basic (orphans, stale) | Partial |
| **Pratiyush/llm-wiki** | Multi-phase with verify | SHA-256 | 16 rules (8 structural + 3 LLM) | Yes (auto-fix) |
| **nashsu/llm_wiki** | Desktop app, two-step ingest | SHA-256 cache | Basic | No |
| **NiharShrotri/llm-wiki** | Local Ollama + Qwen3 + QMD | SHA-256 | Broken links + orphans + frontmatter + contradictions | Yes (`--fix`) |
| **kytmanov/obsidian-llm-wiki-local** | Obsidian integration | Content change detection | `olw lint` (no LLM needed) | Yes (`olw maintain --fix`) |
| **kenhuangus/llm-wiki** | Entity/claim/relationship extraction | SHA-256 in frontmatter | Basic | No |
| **Ar9av/obsidian-wiki** | Obsidian framework | Provenance tracking | Audit + lint (orphans, stale, contradictions) | Partial |

### 7.2 Karpathy's Evolution of Thinking

| Phase | Date | Concept |
|-------|------|---------|
| **Vibe Coding** | Feb 2025 | Let LLMs write code with minimal human direction |
| **Agentic Engineering** | Jan 2026 | Structured agent workflows with tools and feedback |
| **LLM Knowledge Bases** | Apr 2026 | Compile knowledge once, query forever |

Each phase shifts more cognitive labor to the LLM while keeping humans in the loop for judgment.

---

## 8. Thresholds for SOTA Level

### 8.1 Basic Wiki (Score 2.0 -> 3.0)

| Threshold | Target | Metric |
|----------|--------|--------|
| Compilation pipeline works E2E | PASS (ingest + generate + index) | Binary |
| SHA-256 incremental compilation | Cache hit rate >= 80% | % of unchanged sources skipped |
| Frontmatter on all generated pages | 100% | Count |
| Structural lint (rules 1-6) | All 6 rules implemented | Count |
| Wiki queryable via agent tools | PASS | E2E test |

### 8.2 Production Wiki (Score 3.0 -> 4.0)

| Threshold | Target | Metric |
|----------|--------|--------|
| Compilation determinism | temp=0 + seed, same input -> same output | Reproducibility test |
| Self-healing lint with auto-fix | >= 4 rules auto-fixable | Count |
| Wiki integrated into retrieval pipeline (RRF) | PASS | E2E test |
| Deep lint (contradictions) | PASS | E2E test |
| Compilation cost per 100-file project | <= $1.00 with Haiku | Cost measurement |
| Wiki vs memory boundary enforced | No session data in wiki | Audit |

### 8.3 Advanced Wiki (Score 4.0 -> 5.0)

| Threshold | Target | Metric |
|----------|--------|--------|
| Knowledge compounding | Query answers filed as wiki pages | E2E flow |
| Cross-project wiki federation | Shared concepts across repos | Design doc |
| Wiki diff quality (meaningful changes only) | >= 90% of commits are semantic | Audit |
| LLM-powered lint rules (7-11) | All 5 implemented | Count |
| Wiki freshness tracking | Pages updated within 24h of source change | Staleness metric |

---

## 9. Relevance for Theo Code

### 9.1 Immediate Actions

1. **Implement WikiBackend trait:** Theo Code already has a `WikiBackend` trait. Implement the two-phase pipeline (extract -> generate) using tree-sitter for source chunking and Haiku for compilation.
2. **SHA-256 incremental cache:** Hash source files, store in frontmatter, skip unchanged.
3. **6 structural lint rules:** Broken wikilinks, orphans, duplicates, empty pages, broken citations, missing frontmatter. All run without LLM calls.
4. **Wire wiki into RRF:** Add wiki pages as a document root alongside direct file search and memory.

### 9.2 Architecture Decision

```
theo-wiki crate
  |-- WikiCompiler
  |     |-- SourceHasher: SHA-256 incremental detection
  |     |-- ConceptExtractor: Phase 1, LLM extracts concepts from source chunks
  |     |-- PageGenerator: Phase 2, LLM generates wiki pages with wikilinks
  |     |-- IndexBuilder: generates/updates index.md
  |
  |-- WikiLinter
  |     |-- StructuralRules: 6 rules, no LLM needed
  |     |-- DeepRules: LLM-powered contradiction/staleness detection
  |     |-- AutoFixer: resolves fixable issues automatically
  |
  |-- WikiBackend (trait implementation)
  |     |-- query(): navigate index -> read pages -> synthesize
  |     |-- search(): fuzzy search over wiki pages for RRF
  |
  |-- FrontmatterParser: YAML frontmatter read/write
```

### 9.3 Wiki vs Memory Boundary (Concrete)

- **Wiki contains:** Module architecture, API contracts, design patterns used, error handling conventions, dependency relationships, build system configuration
- **Memory contains:** "User prefers explicit error handling over Result propagation," "Last session we refactored module X, tests broke in Y"
- **Rule:** If the knowledge comes from reading code, it goes in wiki. If it comes from interacting with the user, it goes in memory.

### 9.4 Why This Matters

An agent that compiles codebase knowledge into a queryable wiki can:
- Answer "how does module X interact with module Y?" without re-reading all source files
- Detect architectural contradictions (module A assumes X, module B assumes not-X)
- Maintain a living index of the codebase that compounds over sessions
- Reduce context window usage by querying compiled knowledge instead of raw source

This is the difference between an agent that starts from zero every session and one that has deep, persistent codebase understanding.

---

## Sources

- [Karpathy LLM Wiki Gist](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f)
- [LLM Wiki v2 Extension](https://gist.github.com/rohitg00/2067ab416f7bbe447c1977edaaa681e2)
- [Beyond RAG: Karpathy's LLM Wiki Pattern](https://levelup.gitconnected.com/beyond-rag-how-andrej-karpathys-llm-wiki-pattern-builds-knowledge-that-actually-compounds-31a08528665e)
- [LLM Wiki Revolution (Analytics Vidhya)](https://www.analyticsvidhya.com/blog/2026/04/llm-wiki-by-andrej-karpathy/)
- [atomicmemory/llm-wiki-compiler](https://github.com/atomicmemory/llm-wiki-compiler)
- [Pratiyush/llm-wiki](https://github.com/Pratiyush/llm-wiki)
- [NiharShrotri/llm-wiki (Local Ollama)](https://github.com/NiharShrotri/llm-wiki)
- [nashsu/llm_wiki (Desktop App)](https://github.com/nashsu/llm_wiki)
- [kytmanov/obsidian-llm-wiki-local](https://github.com/kytmanov/obsidian-llm-wiki-local)
- [Ar9av/obsidian-wiki](https://github.com/Ar9av/obsidian-wiki)
- [NicholasSpisak/second-brain](https://github.com/NicholasSpisak/second-brain)
- [What Karpathy's LLM Wiki Is Missing](https://dev.to/penfieldlabs/what-karpathys-llm-wiki-is-missing-and-how-to-fix-it-1988)
- [MindStudio: Karpathy LLM Wiki Guide](https://www.mindstudio.ai/blog/karpathy-llm-wiki-knowledge-base-pattern)
