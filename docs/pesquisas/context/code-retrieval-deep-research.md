---
type: deep-research
question: "What is the state of the art in code retrieval and context assembly for AI coding agents, and how does theo-engine-retrieval compare?"
generated_at: 2026-04-29T18:00:00-03:00
confidence: 0.91
sources_used: 68
target_crate: theo-engine-retrieval
---

# Code Retrieval and Context Assembly for AI Coding Agents: State of the Art

## Executive Summary

The state of the art in code retrieval for AI coding agents (2025-2026) has undergone a fundamental paradigm shift. The industry has bifurcated into two camps: **agentic search** (Claude Code, Codex, Cline) which uses grep/ripgrep + file reading + LLM reasoning with no pre-computed index, and **hybrid RAG** (Cursor, Windsurf, Augment Code) which combines BM25 + dense embeddings + reranking with agentic exploration. The academic consensus increasingly supports a third path: **graph-augmented retrieval** that leverages code structure (dependencies, call graphs, type hierarchies) for multi-hop reasoning -- exactly the approach taken by theo-engine-retrieval.

Key findings:

1. **Agentic search beats static RAG for code.** Claude Code abandoned vector DB-based RAG in favor of grep + reasoning. Experiments at leading labs show agentic search consistently outperforms RAG on both internal benchmarks and subjective quality (Preprints.org, Oct 2025).

2. **But graph-based retrieval beats both.** LocAgent (ACL 2025) achieves 92.7% file-level localization accuracy using graph-guided search -- substantially above pure agentic methods. HippoRAG (NeurIPS 2024) shows Personalized PageRank on knowledge graphs outperforms standard RAG by up to 20% on multi-hop reasoning.

3. **Hybrid search (BM25 + Dense + Reranker) remains the gold standard for retrieval quality** when an index is available. RRF fusion is the standard baseline; learned fusion outperforms when tuning data exists.

4. **Code-specific embedding models have matured.** Voyage-code-3 and Jina Code Embeddings (2025) achieve ~79% on CoIR benchmarks, far surpassing general-purpose models like AllMiniLM-L6-v2. Qodo-Embed-1 leads the open-source CoIR leaderboard at 1.5B parameters.

5. **Context assembly is as important as retrieval.** OpenDev's 5-stage Adaptive Context Compaction reduces peak context by ~54%. Greedy knapsack packing achieves 95%+ of optimal with O(n log n) complexity.

6. **theo-engine-retrieval's graph-augmented approach is architecturally aligned with SOTA** (graph attention + PageRank + community detection + BM25 + dense + RRF), but its current metrics (MRR 0.695, Recall@5 0.507) indicate significant room for improvement in implementation quality, particularly in embedding model choice and retrieval pipeline tuning.

---

## Part 1: How Production Agents Do Retrieval (2025-2026)

### 1.1 Claude Code -- Agentic Search, No Vector DB

Claude Code uses a three-layer memory architecture for code retrieval, deliberately eschewing vector databases and embeddings.

**Layer 1: CLAUDE.md** -- Plain markdown files serving as persistent memory across sessions. Human-readable, version-controllable instruction store.

**Layer 2: Grep-Based Active Retrieval** -- The primary retrieval mechanism. Claude Code uses `Read`, `Grep` (ripgrep), `Glob`, and `Bash` tools to search codebases. It navigates project structure like a senior developer: starting from entry points, following the dependency graph, and building a working model of component interactions.

**Layer 3: Background Indexing (KAIROS)** -- A planned background indexer for pre-computed retrieval, described as the "next step" beyond real-time grep. Not yet the primary mechanism.

**Context Compression:** Three compaction strategies -- MicroCompact (zero API calls, local edits), AutoCompact (fires near context ceiling, generates 20K-token structured summary), and Full Compact (compresses entire conversation, re-injects recently accessed files capped at 5K tokens per file).

**Why they abandoned RAG:** Boris Cherny (Claude Code developer) explained that early versions used RAG + local vector DB. They found agentic search "generally works better -- simpler, no issues with security, privacy, staleness, and reliability." Index generation, storage, sync, and permissions add complexity without proportional benefit.

**Key insight:** The retrieval architecture is *agentic tools* -- the LLM decides what to search, reads results, reasons, and searches again. No pre-computed index needed for codebases that fit within tool-call budgets.

> Sources: [Dive into Claude Code (arXiv:2604.14228)](https://arxiv.org/html/2604.14228v1), [Claude Code Source Leak Analysis (MindStudio)](https://www.mindstudio.ai/blog/claude-code-source-leak-three-layer-memory-architecture), [Why Coding Agents Use Grep, Not Vectors (MindStudio)](https://www.mindstudio.ai/blog/is-rag-dead-what-ai-agents-use-instead), [Settling the RAG Debate (SmartScope)](https://smartscope.blog/en/ai-development/practices/rag-debate-agentic-search-code-exploration/)

### 1.2 OpenAI Codex -- Sandbox + Tools

Codex runs each task in an isolated cloud sandbox preloaded with the repository. It can read/edit files, run commands (test harnesses, linters, type checkers), and iteratively refine output until tests pass.

**Retrieval approach:** Tool-driven exploration within the sandbox. No documented embedding index. Guided by `AGENTS.md` files placed in repositories (analogous to CLAUDE.md). Support for MCP enables extending context with third-party tools.

**Built-in retrieval tools:** Web search (from OpenAI-maintained index), File Search (vector stores as hosted RAG primitive), Code Interpreter (sandboxed Python), and shell execution.

**Model:** GPT-5.4-Codex (March 2026), specialized for coding tasks. Sandbox architecture ensures safety and reproducibility but introduces latency.

> Sources: [OpenAI Codex Developers](https://developers.openai.com/codex), [Codex CLI Features](https://developers.openai.com/codex/cli/features), [Codex Wikipedia](https://en.wikipedia.org/wiki/OpenAI_Codex_(AI_agent))

### 1.3 Cursor -- Embeddings + RAG + Hybrid Search

Cursor is the most prominent agent that **does** use vector-based retrieval for code.

**Indexing pipeline:**
1. Code is chunked at logical boundaries using tree-sitter (functions, classes) -- typically a few hundred tokens per chunk.
2. Chunks are embedded using OpenAI or a custom embedding model.
3. Embeddings + metadata stored in **Turbopuffer** (serverless vector + full-text search engine backed by object storage).
4. File paths are obfuscated before transmission for privacy; only embeddings and metadata go to the cloud, raw source stays local.

**Retrieval pipeline:**
1. Query is converted to embedding vector.
2. Vector database queried for nearest matches.
3. Results (masked file paths + line ranges) returned to local client.
4. Client resolves metadata to retrieve actual code chunks from local filesystem.
5. Chunks provided as context alongside query to LLM.

**Hybrid approach:** Semantic search (embeddings) combined with regex/grep pattern matching. Users can also explicitly include files via `@file`, `@folder`, `@docs` for direct control.

**Key difference from Claude Code:** Cursor proactively indexes the entire codebase and retrieves relevant chunks without the user knowing which files matter. Claude Code requires the LLM to actively search.

> Sources: [How Cursor Actually Indexes Your Codebase (Towards Data Science)](https://towardsdatascience.com/how-cursor-actually-indexes-your-codebase/), [Building RAG on Codebases (LanceDB)](https://blog.lancedb.com/building-rag-on-codebases-part-2/), [Cursor Context Window (Morph)](https://www.morphllm.com/cursor-context-window)

### 1.4 Windsurf (Codeium / Cognition) -- RAG Context Engine + M-Query

Windsurf uses a proprietary RAG-based context engine with a custom retrieval method called **M-Query**.

**Context assembly pipeline (per interaction):**
1. Load rules (global `.windsurfrules` + project-level)
2. Load relevant memories (persistent facts from previous sessions)
3. Read open files (active file gets highest weight)
4. Run codebase retrieval (M-Query pulls semantically relevant snippets from index)
5. Read recent actions (edits, terminal commands, navigation history)
6. Assemble final prompt (merged, weighted, trimmed to context window)

**Indexing:** Entire local codebase indexed using embeddings generated locally. Embeddings (not raw source) power retrieval. M-Query improves precision over basic cosine similarity, reducing hallucination rate.

**Fast Context (2026):** Powered by SWE-grep and SWE-grep-mini -- retrieves relevant code 10x faster than standard agentic search, using 8 parallel tool calls per turn across 4 turns.

**Codemaps:** AI-annotated visual maps of code structure powered by SWE-1.5 and Sonnet 4.5.

> Sources: [Windsurf Flow Context Engine (Markaicode)](https://markaicode.com/windsurf-flow-context-engine/), [Windsurf Context Awareness Docs](https://docs.windsurf.com/context-awareness/overview), [Windsurf Review 2026 (VibeCoding)](https://vibecoding.app/blog/windsurf-review)

### 1.5 GitHub Copilot -- Multi-Source Context Assembly

Copilot collects context from multiple sources with a priority hierarchy:

1. **Active file** (always highest priority, area around cursor)
2. **Editor state** (open files, recent copies, file switches)
3. **Cross-file workspace context** (related files, modules, patterns)
4. **Semantic search** (for repos on GitHub/Azure DevOps: creates remote codebase index with embeddings)

**Token limits:** 64K tokens standard, 128K with VS Code Insiders + GPT-4o extended.

**Compaction:** At ~80% context capacity, Copilot CLI automatically compacts in background, leaving ~20% buffer.

**Copilot Memories (2026):** Learns project-specific coding standards across sessions via intelligent detection of corrections and preferences.

> Sources: [Copilot Context Handling (M365FM)](https://www.m365.fm/blog/copilot-context-handling-explained/), [GitHub Docs: Provide Context](https://docs.github.com/en/copilot/how-tos/provide-context), [Context Management GitHub Docs](https://docs.github.com/en/copilot/concepts/agents/copilot-cli/context-management)

### 1.6 OpenDev -- 4-Layer Retrieval Pipeline

OpenDev (arXiv:2603.05344) implements the most thoroughly documented retrieval pipeline among open-source coding agents.

**Retrieval layers:**
1. **Anchor-based tool selection** -- Query analyzed for semantic anchors (symbol names, string literals, patterns, paths, dependencies) that map to specific retrieval tools.
2. **Code Explorer subagent** -- Dedicated read-only subagent for multi-step codebase exploration.
3. **Tool result optimization** -- Per-tool-type summarization (30K tokens to <100 tokens for test suite output).
4. **5-stage Adaptive Context Compaction (ACC)** -- Progressive compression at 70/80/85/90/99% context pressure thresholds.

**Impact:** ACC reduces peak context by ~54%. Sessions extend from 15-20 turns to 30-40 turns without emergency compaction.

**LSP choice:** OpenDev chose LSP over tree-sitter for semantic analysis, citing the need for type resolution and cross-file references -- though the paper notes friction in adapting LSP for autonomous agent workflows.

> Source: [OpenDev (arXiv:2603.05344)](https://arxiv.org/html/2603.05344v2)

### 1.7 Aider -- Tree-Sitter + PageRank Repo Map

Aider represents the most structurally sophisticated retrieval among CLI-based tools, using a four-layer architecture:

1. **Tree-sitter AST parsing** -- Extracts definitions and references across 130+ languages using tags.scm query files.
2. **NetworkX graph building** -- Files as nodes, edges as reference relationships (imports, function calls, inheritance).
3. **PageRank ranking** -- Personalised PageRank with context weighting to identify most relevant files.
4. **Token budget optimization** -- Binary search to fit symbols within configurable budgets (default 1K tokens via `--map-tokens`).

**Output:** Hierarchical tree structure showing file paths and their definitions. Only the most relevant portions included based on graph ranking.

**Key characteristics:**
- No semantic embeddings -- relies entirely on structural relationships.
- Achieves lowest token usage among compared agents (8.5-13K tokens).
- Successfully identifies interface implementations, missing methods, and dependency relationships through static analysis alone.

> Sources: [Aider Repo Map Docs](https://aider.chat/docs/repomap.html), [Building a Better Repo Map (Aider Blog)](https://aider.chat/2023/10/22/repomap.html), [Aider DeepWiki](https://deepwiki.com/Aider-AI/aider/4.1-repository-mapping)

### 1.8 Continue.dev -- Extensible Context Providers

Continue.dev implements context retrieval through a plugin-based architecture of **Context Providers**:

- **@Codebase** -- Semantic search using embeddings stored in vector databases + tree-sitter AST parsing + ripgrep text search.
- **@Search** -- Powered by ripgrep for exact-match codebase search.
- **@Tree** -- Repository structure map, inspired by Aider's repo map.
- **@Code** -- Reference specific functions/classes from throughout the project.
- **@File / @Folder** -- Direct file/folder inclusion.

**Repository map:** Uses file paths only (not full symbol extraction like Aider). Models in Claude 3, Llama 3.1/3.2, Gemini 1.5, and GPT-4o families automatically use the repository map during retrieval.

**Indexing:** Code files indexed using embeddings stored in vector databases, AST parsing via tree-sitter, and fast text search through ripgrep. LanceDB used for vector storage.

> Sources: [Continue.dev Context Providers Docs](https://docs.continue.dev/customize/custom-providers), [Continue Codebase Indexing (DeepWiki)](https://deepwiki.com/continuedev/continue/3.4-context-providers)

### 1.9 Augment Code -- Custom Paired Embeddings

Augment Code differentiates through custom embedding and retrieval models **trained in pairs** for maximum quality. Their Context Engine processes 400K-500K files with millisecond-level sync to code changes.

**Key technical claims:**
- Research-driven embeddings (custom, not off-the-shelf).
- Entire repositories ingested with semantic embeddings maintained.
- Secure, personalized code indexing processing thousands of files per second on Google Cloud.

**Architecture:** Proprietary models for code-specific tasks + Claude models (via Vertex AI) for chat/generation. Technical details of embedding architecture remain proprietary.

> Sources: [How Augment Code Solved the Large Codebase Problem (Codacy)](https://blog.codacy.com/ai-giants-how-augment-code-solved-the-large-codebase-problem), [Augment Code on Google Cloud](https://cloud.google.com/customers/augment)

### 1.10 Production Agent Retrieval Summary

| Agent | Retrieval Method | Embeddings | Index | Graph | Key Innovation |
|-------|-----------------|------------|-------|-------|----------------|
| **Claude Code** | Agentic (grep/read/glob) | No | No | No | LLM reasons about what to search |
| **Codex** | Sandbox tools | Optional (File Search) | Optional | No | Isolated sandbox per task |
| **Cursor** | Hybrid RAG | Yes (Turbopuffer) | Yes | No | Tree-sitter chunking + cloud vector DB |
| **Windsurf** | RAG + M-Query | Yes (local) | Yes | No | Proprietary M-Query retrieval |
| **Copilot** | Multi-source assembly | Yes (remote index) | Yes | No | Priority-based context hierarchy |
| **OpenDev** | Anchor-based tools | No | No | No | 5-stage ACC, tool result optimization |
| **Aider** | Tree-sitter + PageRank | No | Graph only | **Yes** | Graph-ranked repo map |
| **Continue** | Plugin-based (embed + grep) | Yes (LanceDB) | Yes | No | Extensible @provider architecture |
| **Augment** | Custom paired embeddings | Yes (proprietary) | Yes | Unknown | Custom trained embedding pairs |
| **theo** | BM25 + Dense + Graph + RRF | Yes (AllMiniLM/Jina) | Yes | **Yes** | Graph attention + PageRank + community |

---

## Part 2: Academic Research on Code Retrieval

### 2.1 Retrieval-Augmented Code Generation Survey (arXiv:2510.04905)

The most comprehensive survey on RACG (579 candidate papers, Jan 2023 - Sep 2025). Key findings:

- Despite progress, most deployed systems still offer "only basic context retrieval, typically limited to file- or text-based search within agent frameworks."
- Structural context (ASTs, dependency graphs, call graphs) remains underutilized in production systems.
- The survey categorizes retrieval strategies: sparse (BM25), dense (embedding), hybrid (fusion), structural (AST/graph), and agentic (tool-driven).

> Source: [arXiv:2510.04905](https://arxiv.org/abs/2510.04905)

### 2.2 CodeRAG-Bench (NAACL 2025)

Holistic benchmark for retrieval-augmented code generation covering basic programming, open-domain, and repository-level tasks.

**Key findings from evaluating 10 retrievers + 10 LMs:**
- Retrieving high-quality contexts improves code generation.
- Retrievers often struggle to fetch useful contexts, especially with limited lexical overlap.
- Generators face limitations in effectively using retrieved contexts.
- Documents from 5 sources: competition solutions, tutorials, library docs, StackOverflow, GitHub repos.

> Source: [CodeRAG-Bench (arXiv:2406.14497)](https://arxiv.org/abs/2406.14497)

### 2.3 LocAgent -- Graph-Guided Code Localization (ACL 2025)

**The most directly relevant academic work to theo-engine-retrieval.**

LocAgent parses codebases into **directed heterogeneous graphs** capturing code structures (files, classes, functions) and their dependencies (imports, invocations, inheritance), enabling LLM agents to search and locate relevant entities through multi-hop reasoning.

**Results:**
- Up to **92.7% accuracy** on file-level localization (SWE-Bench Lite).
- Fine-tuned Qwen-2.5-Coder-Instruct-32B achieves comparable results to proprietary models at **~86% cost reduction**.
- Over 70% top-1 accuracy on file-, class-, and function-level localization.
- Improves downstream GitHub issue resolution by 12% (Pass@10).
- On harder MULocBench: <40% Acc@5, showing room for improvement.

**Relevance for theo:** LocAgent validates the core thesis that graph-based code representation improves retrieval. Its directed heterogeneous graph is structurally similar to theo's code graph.

> Source: [LocAgent (arXiv:2503.09089)](https://arxiv.org/abs/2503.09089), [ACL 2025](https://aclanthology.org/2025.acl-long.426/)

### 2.4 SWE-Search -- MCTS for Code Navigation (ICLR 2025)

SWE-Search integrates Monte Carlo Tree Search with software agents for repository-level tasks.

**Architecture:**
- SWE-Agent for adaptive exploration
- Value Agent for iterative feedback
- Discriminator Agent for multi-agent debate
- Hybrid value function combining numerical estimation + qualitative LLM evaluation

**Results:** 23% relative improvement across 5 models on SWE-bench compared to standard agents without MCTS. Performance scales with increased inference-time compute through deeper search.

**Key insight:** Linear, sequential exploration is suboptimal. Tree search with backtracking enables better code navigation.

> Source: [SWE-Search (arXiv:2410.20285)](https://arxiv.org/abs/2410.20285)

### 2.5 AutoCodeRover and Agentless -- Localization Approaches

**AutoCodeRover** uses AST-based code search with stratified keyword extraction. LLM extracts keywords from issue descriptions, then invokes search APIs based on class/method/file structure. With spectrum-based fault localization: 30.67% SWE-bench Lite resolution.

**Agentless** uses a hierarchical two-stage pipeline: (1) pre-process repository with Python AST into simplified overview, (2) LM views files and selects targets. Localization: file -> class/function -> edit locations.

**RGFL (January 2026) comparison on SWE-bench Verified:**

| Method | Hit@3 File Localization |
|--------|------------------------|
| RGFL (Gemini 2.5 Pro) | 92.8% |
| OpenHands | 92.2% |
| Agentless | 90.0% |
| AutoCodeRover | 67.8% |

> Sources: [AutoCodeRover (arXiv:2404.05427)](https://arxiv.org/html/2404.05427v2), [Agentless (GitHub)](https://github.com/OpenAutoCoder/Agentless), [RGFL (arXiv:2601.18044)](https://arxiv.org/html/2601.18044)

### 2.6 GrepRAG (arXiv:2601.23254, February 2026)

Introduces Naive GrepRAG: a framework where the LLM autonomously generates ripgrep commands for context localization. Exploratory experiments show this naive approach **outperforms complex graph-based methods** for code completion tasks specifically.

**Implication for theo:** Graph-based methods excel at multi-hop reasoning and localization, but for simpler retrieval tasks (single-file completion), grep-driven approaches may suffice. The value of graph augmentation increases with task complexity.

> Source: [GrepRAG (arXiv:2601.23254)](https://arxiv.org/html/2601.23254)

### 2.7 Agentic RAG Survey (arXiv:2501.09136, updated April 2026)

Agentic RAG embeds autonomous AI agents into the RAG pipeline, leveraging reflection, planning, tool use, and multi-agent collaboration to dynamically manage retrieval.

**Key classification:**
- **Single-agent Agentic RAG:** One agent manages retrieval loop.
- **Multi-agent Agentic RAG:** Specialized agents for different retrieval subtasks.
- **Hierarchical Agentic RAG (A-RAG):** Substantially surpasses prior methods; performance improves with computational resources.

> Source: [Agentic RAG Survey (arXiv:2501.09136)](https://arxiv.org/abs/2501.09136)

### 2.8 HippoRAG -- Knowledge Graph + Personalized PageRank

**Directly relevant to theo's PageRank-based approach.**

HippoRAG (NeurIPS 2024) orchestrates LLMs, knowledge graphs, and Personalized PageRank to mimic hippocampal memory indexing.

**Results:**
- Outperforms SOTA on multi-hop QA by up to **20%**.
- Single-step retrieval achieves comparable/better performance than iterative retrieval (IRCoT) while being **10-30x cheaper** and **6-13x faster**.

**HippoRAG 2 (ICML 2025):** Dual-node knowledge graph with passage and phrase nodes, enhanced PPR + LLM-based triple filtering. Lifts associative QA F1 by 7 points over SOTA embedding retrievers.

**Relevance for theo:** Validates PageRank on knowledge graphs as a retrieval strategy. theo's PageRank on code dependency graphs is analogous.

> Sources: [HippoRAG (arXiv:2405.14831)](https://arxiv.org/abs/2405.14831), [HippoRAG GitHub](https://github.com/osu-nlp-group/hipporag)

### 2.9 RAPTOR -- Recursive Tree-Structured Summaries (ICLR 2024)

Recursively embeds, clusters, and summarizes text chunks into a tree with different levels of abstraction. Retrieves from multiple tree levels at inference time.

**Results:** With GPT-4, improved best performance on QuALITY benchmark by 20% absolute accuracy. Consistently outperforms any single-level retriever across all datasets.

**Relevance for theo:** theo's community summaries (derived from graph clustering) serve a similar purpose to RAPTOR's hierarchical summaries -- providing multi-level abstraction of code structure.

> Source: [RAPTOR (arXiv:2401.18059)](https://arxiv.org/abs/2401.18059)

### 2.10 Self-RAG -- Adaptive Retrieval (Asai et al., 2023)

Trains a single LM that adaptively retrieves passages on-demand using **reflection tokens** to decide retrieval necessity.

**Mechanism:** Three-stage process (Retrieve -> Generate -> Critique) using special tokens:
- Retrieval decision tokens ("Yes", "No", "Continue")
- Relevance assessment tokens
- Support evaluation tokens
- Utility scoring tokens

**Extension: Probing-RAG** -- Skips retrieval in 57.5% of cases via hidden-state probing while exceeding prior adaptive RAG baselines by 6-8 points.

**Relevance for theo:** theo's query type classifier (Identifier/NaturalLanguage/Mixed) serves a simpler version of adaptive retrieval, routing queries to appropriate rankers.

> Sources: [Self-RAG (selfrag.github.io)](https://selfrag.github.io/), [Probing-RAG](https://blog.reachsumit.com/posts/2025/10/learning-to-retrieve/)

### 2.11 GraphRAG for Code

**"Towards Practical GraphRAG" (arXiv, December 2025):** First application of GraphRAG to enterprise legacy code migration. Uses dependency parsing for efficient KG construction + hybrid retrieval (vector similarity + graph traversal) fused with RRF. Significant improvements over dense retrieval baselines.

**Key variants in the ecosystem:**
- **LightRAG / FastGraphRAG** -- Optimized indexing for speed and cost.
- **LazyGraphRAG (Microsoft)** -- Defers expensive graph construction until query time.
- **LinearRAG** -- Accepted at ICLR 2026 for efficient GraphRAG.

> Sources: [GraphRAG Survey (arXiv:2501.00309)](https://arxiv.org/abs/2501.00309), [Practical GraphRAG (arXiv:2507.03226)](https://arxiv.org/html/2507.03226)

---

## Part 3: BM25 for Code -- Is It the Right Approach?

### 3.1 BM25 Effectiveness for Code Search

**Sourcegraph's BM25F implementation (April 2025):** After implementing BM25F in their 6.2 release, internal evaluations showed roughly **20% improvement across all key metrics** compared to baseline ranking. Answer: yes, BM25 works well for code search.

**BM25F field boosting for code:** theo-engine-retrieval uses BM25F with field boosts (filename 5x, path 3x, symbols 3x), which aligns with Sourcegraph's approach of weighting different code fields differently.

### 3.2 Code-Specific Tokenization Challenge

The central challenge: code identifiers don't tokenize like natural language.

**Problem example:** A query `create pthread` should match `pthread_create()`, but `thread_create` must also work. Standard tokenization can't predict how `pthread_create()` should be split.

**Sourcegraph's approach:** Each new query "induces" a tokenization on the corpus -- splitting text everywhere it matches a query term. This is dynamic, not pre-computed.

**theo's approach:** Custom tokenizer that splits camelCase (`HTMLParser` -> [html, parser]) and snake_case, with stemming. This is a reasonable approach but less flexible than Sourcegraph's query-induced tokenization.

**Benchmark impact:** Tokenizer choice has significant impact on BM25 effectiveness. The variation from tokenization often exceeds the variation between BM25 scoring variants (BM25, BM25+, BM25L).

### 3.3 BM25 Limitations for Code

- **No semantic understanding:** BM25 cannot capture that `authenticate()` and `login()` serve similar purposes.
- **No structural awareness:** Doesn't understand type relationships, inheritance hierarchies, or call chains.
- **Identifier fragmentation:** Rare identifiers in large codebases get disproportionately high IDF scores, potentially outranking more relevant results.

### 3.4 Tantivy as Backend

**Performance:** Tantivy starts in under 10ms and runs approximately **2x faster than Lucene** in benchmarks. Query speed: 0.8ms average vs. Elasticsearch's 5.2ms (6.5x speedup).

**Architecture:** Posting lists compressed into blocks of 128 documents with SIMD bit packing. Finite State Transducers (FSTs) for efficient term dictionary storage.

**Code search adoption:** Used by Bloop's low-latency code search engine and integrated into Milvus as inverted index.

**Assessment for theo:** Tantivy is the correct choice for a Rust-based code search engine. Performance characteristics are excellent.

> Sources: [Sourcegraph BM25F Blog](https://sourcegraph.com/blog/keeping-it-boring-and-relevant-with-bm25f), [Tantivy GitHub](https://github.com/quickwit-oss/tantivy), [Tantivy Architecture](https://github.com/quickwit-oss/tantivy/blob/main/ARCHITECTURE.md)

---

## Part 4: Dense/Semantic Search for Code

### 4.1 Code Embedding Models -- Current Landscape

| Model | Avg Score (CoIR/25 benchmarks) | Parameters | Context Length | Open Source |
|-------|-------------------------------|------------|---------------|-------------|
| **Qodo-Embed-1-7B** | 71.5% | 7B | -- | Conditional |
| **Qodo-Embed-1-1.5B** | 70.06% | 1.5B | -- | OpenRAIL++ |
| **voyage-code-3** | ~79.23% | Undisclosed | 32K | No |
| **jina-code-embeddings-1.5B** | ~79.04% | 1.54B | -- | Yes |
| **jina-code-embeddings-0.5B** | ~78.41% | 494M | -- | Yes |
| **gemini-embedding-001** | ~77.38% | Undisclosed | -- | No |
| **Qwen3-Embedding-0.6B** | ~73% | 600M | -- | Yes |
| **OpenAI text-embedding-3-large** | 65.17% | ~7B | 8K | No |
| **AllMiniLM-L6-v2** | ~55-60% (estimated) | 22M | 256 | Yes |
| **CodeSage-large** | ~62% | 1.3B | 1K | Yes |

### 4.2 AllMiniLM-L6-v2 vs Code-Specific Models

**theo currently uses AllMiniLM-L6-v2 (384-dim, 22M parameters)** as its default neural embedder, with optional Jina Code v2 (768-dim).

**The gap is significant.** AllMiniLM-L6-v2 is a general-purpose model trained on NLI data, not code. Code-specific models like voyage-code-3, Jina Code Embeddings (2025), and Qodo-Embed-1 are trained on millions of code-NL pairs and outperform general-purpose models by 15-25+ percentage points on code retrieval benchmarks.

**Recommendation:** Upgrading from AllMiniLM-L6-v2 to Jina Code Embeddings 0.5B or Qodo-Embed-1-1.5B would likely provide the single largest improvement to theo's retrieval quality.

### 4.3 Fine-Tuning Impact

Fine-tuning shows **+10-30% gains** for specialized domains like code. Qodo's training approach combines high-quality synthetic data with real-world code samples, training the model to recognize nuanced differences in functionally similar code.

### 4.4 Quantization Impact

**theo's TurboQuant (2-bit compression, 32x storage reduction, ~5% quality loss)** is aggressive. Industry benchmarks suggest:
- **int8 quantization:** Minimal impact (~1-2% loss).
- **binary quantization:** Significant loss but Matryoshka learning (used by voyage-code-3) mitigates it.
- **2-bit:** theo's 5% loss claim should be validated against SOTA benchmarks.

### 4.5 Cross-Encoder Reranking for Code

**Top rerankers (2025-2026):**

| Reranker | nDCG@10 (general) | Code Support | Latency | Size |
|----------|-------------------|--------------|---------|------|
| **Jina Reranker v3** | 61.94 (BEIR) | 63.28 CoIR | 188ms | 600M |
| **Nemotron-rerank-1b** | High | Unknown | 243ms | 1.2B |
| **Cohere Rerank v4** | 0.735 (QA) | Unknown | 210ms | Proprietary |
| **BGE-reranker-large v2** | 0.715 | Unknown | 145ms | Medium |
| **Jina Reranker v2** | 0.694 | Yes (multilingual + code) | 110ms | 278M |

**Key finding:** Model size does not determine reranker quality. Jina v3 at 600M matches or beats 1.2B models.

**theo uses Jina Reranker v2 (~568MB).** This is a reasonable choice with explicit code search support. Upgrading to Jina Reranker v3 would provide better performance.

**Impact of reranking:** Top rerankers deliver **15-40% higher precision** than embeddings alone.

> Sources: [Jina Reranker v3](https://jina.ai/models/jina-reranker-v3/), [Reranker Benchmark (AIMultiple)](https://aimultiple.com/rerankers), [Agentset Reranker Leaderboard](https://agentset.ai/rerankers), [6 Best Code Embedding Models (Modal)](https://modal.com/blog/6-best-code-embedding-models-compared), [Qodo-Embed-1](https://www.qodo.ai/blog/qodo-embed-1-code-embedding-code-retrieval/), [Voyage-code-3 (MongoDB)](https://www.mongodb.com/company/blog/voyage-code-3-more-accurate-code-retrieval-lower-dimensional-quantized-embeddings)

---

## Part 5: Hybrid Retrieval (BM25 + Dense + Graph)

### 5.1 RRF -- Still SOTA for Zero-Shot Fusion

**Formula:** `RRF_score(d) = SUM(1 / (k + rank(d)))`, k = 60 typically.

**Strengths:**
- Normalization-free (works with any score scales).
- No training data needed.
- Outlier-resistant.
- Seven lines of code.
- Beat Condorcet Fuse, CombMNZ, and learning-to-rank methods in original SIGIR 2009 paper.

**Limitations:**
- Ignores actual score magnitudes (a document with a much higher BM25 score is treated identically to one marginally higher).
- Less adaptable than learned convex combinations.
- Prone to performance non-smoothness under domain shift.

**Recent developments (2025-2026):**
- **Weighted RRF (Elasticsearch):** Fine-tune influence per retriever.
- **Linear Retriever (Elasticsearch):** Weighted sum with normalization, preserving score magnitudes.
- **Dynamic weighting extensions (Exp4Fuse):** Task-dependent optimal values.

### 5.2 RRF vs Learned Fusion

| Feature | RRF | Learned/Score-Based Fusion |
|---------|-----|---------------------------|
| Training data needed | No | Yes (modest) |
| Handles score scale differences | Inherently | Requires normalization |
| Adaptability | Lower | Higher |
| Preserves score magnitude | No | Yes |
| Best for | Zero-shot, quick deployment | Tuned production systems |

**Consensus:** RRF is the go-to baseline for hybrid search. Learned fusion outperforms when labeled data is available.

**theo's position:** Uses standard RRF (BM25 + Tantivy + Dense -> top-50 -> reranker -> top-20). This is the correct starting point. Weighted RRF or learned fusion would be the next step.

### 5.3 ColBERT and SPLADE for Code

**ColBERT (Late Interaction):** Token-level embeddings with sum-of-max scoring. ColBERTv2 adds residual compression for storage efficiency. RAGatouille library for easy adoption. Query times under 1ms with PLAID centroid pruning.

**SPLADE (Learned Sparse):** Learns to expand queries/terms and plays well with inverted indexes. SPLADE -> ColBERT pipeline is becoming standard for speed + quality.

**SPLATE:** Bridges ColBERT and SPLADE -- converts frozen ColBERTv2 to effective SPLADE with lightweight residual adaptation.

**For code specifically:** No dedicated code-trained ColBERT or SPLADE model exists yet. This is an opportunity.

### 5.4 Evidence: When Does Hybrid Beat Single-Signal?

**For code search specifically:**
- Sourcegraph (BM25 only) achieves ~20% improvement over baseline.
- Cursor (hybrid semantic + grep) handles cross-file dependencies that grep alone misses.
- CodeRAG-Bench shows retrievers with limited lexical overlap struggle -- semantic search fills this gap.

**General evidence:**
- Two-stage architecture (hybrid first stage + cross-encoder rerank) is industry standard.
- Hybrid search (BM25 + dense) consistently outperforms either signal alone, with reranking adding 15-40% precision improvement.

> Sources: [RRF Advanced RAG (glaforge.dev)](https://glaforge.dev/posts/2026/02/10/advanced-rag-understanding-reciprocal-rank-fusion-in-hybrid-search/), [Weighted RRF (Elastic)](https://www.elastic.co/search-labs/blog/weighted-reciprocal-rank-fusion-rrf), [ColBERT GitHub](https://github.com/stanford-futuredata/ColBERT), [Late Interaction Overview (Weaviate)](https://weaviate.io/blog/late-interaction-overview)

---

## Part 6: Graph-Based Code Intelligence

### 6.1 Code Property Graphs (CPG)

CPGs combine Abstract Syntax Trees, Control Flow Graphs, and Program Dependency Graphs into a unified representation. They enable:
- Precise taint analysis across data flow, control flow, and syntax.
- Pattern matching for vulnerability detection.
- Large-scale queries about codebase structure.

**Tools:** Joern (open-source), CodeQL (GitHub), ShiftLeft/Qwiet AI (commercial). Stored in graph databases (OverflowDB, Neo4j, TinkerGraph) and queried via Gremlin, Cypher, or tool-specific DSLs.

**theo's approach:** Uses Tree-sitter-derived code graphs (symbols, edges, dependencies) rather than full CPGs. This is lighter-weight and faster but sacrifices control flow and data flow analysis.

### 6.2 PageRank on Code Graphs

**Does it help?** Yes, with strong evidence:
- **Aider** uses PageRank on dependency graphs to rank file relevance -- achieving the lowest token usage (8.5-13K) among compared agents while maintaining quality.
- **HippoRAG** uses Personalized PageRank on knowledge graphs -- outperforms SOTA by up to 20% on multi-hop QA.
- **LocAgent** uses graph-guided traversal -- achieves 92.7% file-level localization accuracy.

**theo's implementation:** Sparse PageRank (20 iterations, damping 0.85) on the code dependency graph. This is well-aligned with SOTA approaches.

**Personalized PageRank (PPR) vs standard PageRank:** PPR allows biasing the random walk toward query-relevant seed nodes. HippoRAG shows PPR significantly outperforms standard PageRank for retrieval. theo should consider implementing PPR with query-relevant files as seeds.

### 6.3 Community Detection for Code Clustering

**Algorithms:**
- **Louvain:** Modularity optimization, widely used, fast. Produces hierarchical community structure.
- **Leiden:** Improved version of Louvain, guarantees connected communities.
- **Label Propagation (CDLP):** Simpler, faster, but less stable.

**Application to code:** Community detection on code dependency graphs identifies logical modules/subsystems. This aligns with how developers mentally organize codebases.

**theo's implementation:** Community detection with summaries (symbols, edges, cross-deps). Used for repo_map tool. This is a unique and valuable capability -- no other production agent uses community detection for code organization.

### 6.4 Graph Attention Propagation

**theo's graph_attention.rs:** Propagates attention scores through the dependency graph (damping 0.5, multi-hop). "If auth.rs is relevant, session.rs and crypto.rs also are."

**Academic parallel:** Graph Attention Networks (GATs) use attention mechanisms to weight neighbor contributions. Graph Neural Networks for code (Code2Graph, CodeGNN) show promising results for various code understanding tasks.

**Uniqueness:** Graph attention propagation for retrieval is relatively novel. Most graph-based code retrieval uses simple traversal or PageRank, not attention-weighted propagation.

> Sources: [Code Property Graph (Apiiro)](https://apiiro.com/glossary/code-property-graph/), [PageRank Neo4j GDS](https://neo4j.com/docs/graph-data-science/current/algorithms/page-rank/), [Community Detection Neo4j GDS](https://neo4j.com/docs/graph-data-science/current/algorithms/community/)

---

## Part 7: Context Assembly and Token Budget Management

### 7.1 Budget Allocation Strategies

**theo's current allocation:**
- repo_map: 15%
- modules: 25%
- code: 40%
- history: 15%
- reserve: 5%

**OpenDev's approach:** Not percentage-based but priority-based -- tool result optimization (compress first), then observation masking, then conversation trimming, then LLM summarization.

**Industry approach (greedy knapsack):** Sort items by (relevance_score / token_cost) descending, greedily pack until budget is full. Achieves 95%+ of optimal with O(n log n) complexity vs O(2^n) for exhaustive search.

**theo implements greedy knapsack** (assembly/greedy.rs), which is correct for the problem.

### 7.2 Granularity: Token vs File vs Symbol

| Granularity | Pros | Cons | Used By |
|-------------|------|------|---------|
| **Token-level** | Maximum packing efficiency | Complex to implement, may break semantic units | LLMLingua |
| **File-level** | Simple, preserves file context | Wastes budget on irrelevant parts of relevant files | Claude Code, Codex |
| **Symbol-level** | Precise, includes only relevant functions/classes | Requires AST parsing, may lose surrounding context | Aider, theo |
| **Chunk-level** | Balance of precision and simplicity | Arbitrary boundaries may split semantic units | Cursor, Windsurf |

**theo operates at file level for assembly** (greedy knapsack by score within token budget) but has symbol-level awareness through the graph. The optimal approach is likely chunk-level with symbol-aware boundaries (as Cursor does with tree-sitter chunking).

### 7.3 Tool Result Optimization

**OpenDev's compression examples:**
- File read: `"Read file (142 lines, 4,831 chars)"` (was: full file content)
- Search: `"Search completed (23 matches found)"` (was: all match details)
- Single test suite: 30,000 tokens -> <100 tokens

**Impact:** Extended sessions from 15-20 turns to 30-40 turns without compaction.

### 7.4 Adaptive Context Compaction

**5-stage pipeline (OpenDev/Claude Code pattern):**

| Stage | Trigger | Action | Cost |
|-------|---------|--------|------|
| 1. Warning | 70% | Monitor only | Free |
| 2. Observation Masking | 80% | Replace old tool results with summaries | Free |
| 3. Fast Pruning | 85% | Remove outputs beyond recency window | Free |
| 4. Aggressive Compression | 90% | Shrink preservation window | Free |
| 5. Full Compaction | 99% | LLM summarization + archive | LLM call |

**Key design principle:** Cheaper strategies first. Fast pruning alone often avoids expensive LLM compaction.

> Sources: [OpenDev ACC (arXiv:2603.05344)](https://arxiv.org/html/2603.05344v2), [TALE Token Budget (ACL 2025)](https://aclanthology.org/2025.findings-acl.1274/), [Context Engineering Infrastructure (DEV)](https://dev.to/siddhantkcode/context-engineering-the-critical-infrastructure-challenge-in-production-llm-systems-4id0)

---

## Part 8: LSP vs Tree-sitter vs Custom AST

### 8.1 Core Differences

| Feature | Tree-sitter | LSP |
|---------|-------------|-----|
| **Speed** | Sub-millisecond incremental parsing | Slower (separate process, type inference) |
| **Scope** | Syntax-level (single file) | Semantic-level (cross-file) |
| **Agent suitability** | High -- lightweight, no server needed | Friction -- designed for interactive use |
| **Token efficiency** | Very efficient (structural extraction) | Higher overhead |
| **Type resolution** | No | Yes |
| **Cross-file references** | No (requires external graph) | Yes (built-in) |

### 8.2 Evidence from the Exploratory Study (October 2025)

Key finding: **"LSP, designed for interactive human workflows with IDE integration, does not translate directly to autonomous agent performance."** Symbol resolution failures, empty reference searches, and coordinate-precision requirements created friction for agent-driven exploration.

However, the underlying principles of LSP (structural code understanding, dependency tracking) proved valuable when adapted for autonomous workflows.

### 8.3 Agent-Specific Results

- **Aider** (Tree-sitter + PageRank, no LSP): 8.5-13K tokens, successfully identifies interface implementations and dependencies through static analysis.
- **Cline** (ripgrep + fzf + Tree-sitter): 17.5% context utilization, structural code awareness.
- **OpenDev** chose LSP over tree-sitter for semantic analysis but acknowledged friction in agent workflows.

### 8.4 The Hybrid Consensus

The emerging consensus: **Tree-sitter as primary parsing backbone** (speed, token efficiency, structural awareness) with **selective LSP-inspired techniques** for semantic intelligence when needed.

**theo's approach:** Tree-sitter parsing via theo-engine-parser (14 languages) for graph construction. No LSP integration. This is the right approach for a retrieval engine -- LSP is better suited for the agent layer, not the retrieval layer.

> Sources: [Tree-sitter vs LSP (Lambda Land)](https://lambdaland.org/posts/2026-01-21_tree-sitter_vs_lsp/), [Exploratory Study (Preprints.org)](https://www.preprints.org/manuscript/202510.0924), [AFT Tree-sitter Tools (GitHub)](https://github.com/cortexkit/aft)

---

## Part 9: Agentic Retrieval vs Pre-Computed Retrieval

### 9.1 The Shift

The industry has shifted from "build index -> query -> rank" to "agent reasons -> chooses tool -> searches" for code specifically. Key evidence:

- **Claude Code abandoned vector DB-based RAG** in favor of grep + reasoning.
- **Leading AI lab experiments:** "Agentic search consistently outperformed RAG approaches across both internal benchmarks and subjective quality evaluations" (Exploratory Study, Oct 2025).
- **GrepRAG (February 2026):** Naive grep-based LLM retrieval outperforms complex graph-based methods for code completion.

### 9.2 Why Agentic Beats Static RAG for Code

1. **Code has explicit structure** -- imports, function calls, type definitions -- that embeddings don't capture reliably.
2. **Agentic search can follow chains** -- `handler.ts` -> `utils/auth.ts` -> `lib/jwt.ts` -- while RAG retrieves isolated chunks.
3. **No index maintenance** -- no staleness, sync, or privacy concerns.
4. **Failure mode:** Grep fails loudly (no match). RAG fails silently (wrong match).

### 9.3 When Pre-Computed Index Wins

1. **Very large codebases (400K+ files):** Scanning via grep becomes slow; pre-computed index provides instant results.
2. **Semantic queries with low lexical overlap:** "find the authentication handler" won't match `session_manager.rs` via grep.
3. **Cross-language search:** Embeddings can bridge language boundaries.
4. **Batch/offline analysis:** Pre-computed graph enables structural analysis without LLM involvement.

### 9.4 Can They Be Combined?

**Yes -- graph-augmented agentic retrieval.** This is exactly theo's approach: pre-compute the graph and index, but expose them as tools the agent invokes on demand.

**Meta Context Engineering (MCE, arXiv:2601.21557):** Automates context engineering itself via bi-level optimization. Achieves 89.1% on benchmarks, 13.6x faster than hand-engineering. Demonstrates that the combination of pre-computed knowledge + agentic optimization beats either alone.

**The optimal architecture:**
```
Agent decides what to search (agentic)
    -> Queries pre-computed graph for structural context (graph)
    -> Runs BM25/dense search for lexical/semantic matches (index)
    -> Fuses results with RRF (hybrid)
    -> Reranks with cross-encoder (reranker)
    -> Packs into context budget (assembly)
```

This is essentially what theo-engine-retrieval implements.

> Sources: [Agentic Search (Morph)](https://www.morphllm.com/agentic-search), [Claude Code RAG Decision (SmartScope)](https://smartscope.blog/en/ai-development/practices/rag-debate-agentic-search-code-exploration/), [MCE (arXiv:2601.21557)](https://arxiv.org/abs/2601.21557)

---

## Part 10: Metrics and Benchmarks for Code Retrieval

### 10.1 Retrieval Quality Metrics

| Metric | What It Measures | Relevance for Code Agents |
|--------|-----------------|--------------------------|
| **MRR** (Mean Reciprocal Rank) | Average 1/rank of first relevant result | High -- agents often use only the top result |
| **Recall@K** | Fraction of relevant items in top K | High -- determines if correct files are surfaced |
| **nDCG@K** | Ranking quality with graded relevance | Medium -- order matters but less than coverage |
| **MAP** (Mean Average Precision) | Average precision across all relevant items | Medium -- comprehensive but complex |
| **Precision@K** | Fraction of top K results that are relevant | Low for agents -- agents can filter irrelevant results |
| **DepCov** (Dependency Coverage) | Fraction of true dependencies captured | **High -- unique to theo, measures graph completeness** |

### 10.2 theo's Current Metrics vs Targets

| Metric | Current | SOTA Target | Gap |
|--------|---------|-------------|-----|
| MRR | 0.695 | >= 0.90 | -0.205 |
| Recall@5 | 0.507 | >= 0.92 | -0.413 |
| Recall@10 | 0.577 | >= 0.95 | -0.373 |
| DepCov | 0.767 | >= 0.96 | -0.193 |
| nDCG@5 | 0.495 | >= 0.85 | -0.355 |

**Analysis:** The gaps are significant across all metrics. Recall@5 at 0.507 is particularly concerning -- it means only half the relevant files appear in the top 5 results. The most impactful improvement would be upgrading the embedding model (Section 4.2).

### 10.3 Cross-File Code Completion Benchmarks

**CrossCodeEval (NeurIPS 2023):** Static-analysis-based benchmark requiring cross-file context for accurate completion. Python, Java, TypeScript, C#. Baseline EM without cross-file context: <11%.

**RepoBench:** Three tasks -- Retrieval (RepoBench-R), Completion (RepoBench-C), and Pipeline (RepoBench-P). Measures end-to-end retrieval + completion quality.

**RepoFuse:** Achieves 40.90-59.75% improvement in EM accuracy over baselines using fused dual context on CrossCodeEval.

**CodeRAG-Bench (NAACL 2025):** Holistic RAG benchmark with 5 document sources. Shows retrievers struggle with limited lexical overlap and generators struggle with limited context.

### 10.4 SWE-bench as Indirect Retrieval Benchmark

SWE-bench tasks require code localization as a prerequisite. Localization accuracy directly impacts resolution rates:

| System | File Localization (Hit@3) | SWE-bench Resolution |
|--------|--------------------------|---------------------|
| RGFL (Gemini 2.5 Pro) | 92.8% | -- |
| LocAgent (fine-tuned) | 92.7% (top-1) | 12% improvement (Pass@10) |
| OpenHands | 92.2% | -- |
| Agentless | 90.0% | 30.67% |
| AutoCodeRover | 67.8% | 22-23% |

### 10.5 Dependency Coverage (DepCov)

theo's DepCov metric measures the fraction of true code dependencies that are captured in the retrieved context. This is unique to theo -- no other system in the literature uses this specific metric.

**The closest equivalent:** LocAgent's graph-based localization implicitly measures dependency coverage through its heterogeneous graph traversal. HippoRAG's multi-hop recall is conceptually similar (measuring whether graph-connected information is retrieved).

**Assessment:** DepCov is a valuable metric that should be promoted as a contribution. It measures something critical that standard IR metrics miss -- whether the retrieved context includes the files that the target code actually depends on.

> Sources: [CrossCodeEval (arXiv:2310.11248)](https://arxiv.org/abs/2310.11248), [RepoBench (ResearchGate)](https://www.researchgate.net/publication/371311819), [RepoFuse (arXiv:2402.14323)](https://arxiv.org/html/2402.14323), [CodeRAG-Bench (arXiv:2406.14497)](https://arxiv.org/abs/2406.14497)

---

## Part 11: What theo-engine-retrieval Does Differently

### 11.1 Architecture Comparison

theo-engine-retrieval implements a **graph-augmented hybrid retrieval** pipeline:

```
Query -> Query Type Classification (Identifier/NL/Mixed)
    -> BM25F (filename 5x, path 3x, symbols 3x) + PRF
    -> Dense Search (AllMiniLM-L6-v2 / Jina Code v2) + PRF
    -> RRF Fusion (BM25 + Tantivy + Dense) -> top-50
    -> Cross-Encoder Reranking (Jina Reranker v2) -> top-20
    -> Graph Attention Propagation (damping 0.5, multi-hop)
    -> MultiSignalScorer (BM25 25% + Semantic 20% + FileBoost 20% + Graph 15% + PageRank 10% + Recency 10%)
    -> Greedy Knapsack Assembly (within token budget)
    -> Context Miss Detection + 1-hop Neighbor Suggestion
```

### 11.2 Unique Capabilities

| Capability | theo | Aider | Cursor | Claude Code | LocAgent |
|-----------|------|-------|--------|-------------|----------|
| BM25/BM25F | Yes (code-aware) | No | No | No (grep) | No |
| Dense embeddings | Yes | No | Yes | No | No |
| Cross-encoder reranking | Yes (Jina v2) | No | Unknown | No | No |
| RRF fusion | Yes | No | Unknown | No | No |
| Code graph (dependencies) | Yes | Yes | No | No | Yes |
| PageRank on code graph | Yes | Yes | No | No | No |
| Graph attention propagation | **Yes (unique)** | No | No | No | No |
| Community detection | **Yes (unique)** | No | No | No | No |
| Greedy knapsack assembly | Yes | Similar | Unknown | No | No |
| Dependency Coverage metric | **Yes (unique)** | No | No | No | No |
| Context miss detection | **Yes (unique)** | No | No | No | No |

### 11.3 Is the Approach Unique?

**Graph attention propagation for code retrieval** -- No other production system or academic paper combines attention-weighted propagation on code dependency graphs with BM25 + dense + RRF fusion. LocAgent uses graph-guided search but without attention propagation. HippoRAG uses Personalized PageRank but on knowledge graphs, not code. This is genuinely novel.

**Community detection for code organization** -- Aider uses graph analysis but not community detection. RAPTOR uses hierarchical clustering of text but not structural code clustering. theo's community-based repo_map is unique.

**Context miss detection** -- No other system detects when a required dependency is missing from context and suggests expansion. This is a valuable safety net.

### 11.4 Is There Evidence It's Better?

**Current evidence is mixed:**
- **Architecture is well-motivated:** LocAgent (graph-guided, ACL 2025) validates graphs for localization. HippoRAG (PageRank + KG, NeurIPS 2024) validates PageRank for retrieval. Hybrid search is consistently better than single-signal.
- **Current metrics are below SOTA:** MRR 0.695 vs target 0.90. The architecture is sound but the implementation needs tuning.
- **Likely bottleneck:** AllMiniLM-L6-v2 as embedding model. Upgrading to a code-specific model would likely close a significant portion of the gap.

---

## Part 12: Concrete Recommendations

### 12.1 KEEP (Validated by Evidence)

| Component | Evidence |
|-----------|----------|
| **BM25F with field boosting** | Sourcegraph shows ~20% improvement with BM25F for code. Field boosting (filename, path, symbols) is correct. |
| **Code-aware tokenization** | camelCase/snake_case splitting is essential; BM25 effectiveness depends heavily on tokenizer choice. |
| **Tantivy backend** | 2x faster than Lucene, 6.5x faster than Elasticsearch. Correct choice for Rust codebase. |
| **RRF fusion** | Industry standard baseline for hybrid search. No training data needed. |
| **Cross-encoder reranking** | 15-40% precision improvement over embeddings alone. Jina Reranker v2 has explicit code support. |
| **PageRank on code graph** | Validated by Aider (practical), HippoRAG (academic), LocAgent (academic). |
| **Graph attention propagation** | Novel and well-motivated. Extends PageRank with query-aware attention. |
| **Community detection** | Unique capability for structural code organization. Aligns with RAPTOR's hierarchical approach. |
| **Greedy knapsack assembly** | 95%+ of optimal with O(n log n). Industry standard for budget-constrained packing. |
| **DepCov metric** | Unique and valuable. Measures what standard IR metrics miss. |
| **Query type classification** | Aligns with Self-RAG's adaptive retrieval principle and anchor-based tool selection. |
| **Pseudo-Relevance Feedback (PRF)** | Well-established technique for query expansion. |
| **Context miss detection** | Novel safety net for incomplete context. |

### 12.2 CHANGE (Evidence Suggests Better Approach)

| Component | Current | Recommended | Evidence | Priority |
|-----------|---------|-------------|----------|----------|
| **Default embedding model** | AllMiniLM-L6-v2 (384d, 22M) | Jina Code Embeddings 0.5B (494M) or Qodo-Embed-1-1.5B | 15-25+ point gap on code retrieval benchmarks | **P0** |
| **Reranker version** | Jina Reranker v2 | Jina Reranker v3 | 5.43% improvement over same-scale bge-reranker-v2-m3; best nDCG@10 on BEIR (61.94) and CoIR (63.28) | **P1** |
| **PageRank variant** | Standard PageRank | Personalized PageRank (PPR) | HippoRAG shows PPR outperforms standard PageRank by up to 20% on multi-hop retrieval | **P1** |
| **RRF fusion** | Standard RRF (k=60) | Weighted RRF with learned weights | Weighted RRF allows per-retriever tuning; learned fusion outperforms when tuning data exists | **P2** |
| **MultiSignal weights** | Fixed (BM25 25%, Semantic 20%, etc.) | Auto-tuned via grid search on benchmark | Fixed weights are unlikely optimal; MCE shows 16.9% improvement via automated optimization | **P2** |
| **2-bit quantization** | TurboQuant (32x, ~5% loss) | int8 first (minimal loss), 2-bit optional | int8 gives ~8x compression with ~1-2% loss. 2-bit is very aggressive. | **P2** |

### 12.3 ADD (Missing Capability that SOTA Has)

| Capability | Description | Evidence | Priority |
|-----------|-------------|----------|----------|
| **Symbol-level chunking** | Chunk code at function/class boundaries using tree-sitter for embedding, not full files | Cursor uses tree-sitter chunking; all embedding benchmarks operate on chunk-level | **P0** |
| **Matryoshka embedding dimensions** | Support variable-dimension embeddings (256/512/768) for cost-quality tradeoff | voyage-code-3 supports this; enables faster search at lower dimensions | **P2** |
| **Adaptive retrieval (Self-RAG style)** | Decide WHEN to retrieve vs use cached/existing context | Self-RAG skips retrieval in 57.5% of cases; Probing-RAG exceeds baselines by 6-8 points | **P2** |
| **LSP integration (optional)** | Type resolution and cross-file references for the agent layer | OpenDev uses LSP for semantic analysis; useful for type-aware retrieval | **P3** |
| **Code-to-code search** | Find similar code fragments (not just NL -> code) | Qodo-Embed-1 and Jina Code optimized for both NL-to-code and code-to-code | **P2** |
| **Benchmark suite on CrossCodeEval/RepoBench** | Evaluate against standard academic benchmarks | Current benchmarks are internal only; need external validation | **P1** |
| **Personalized context from history** | Weight retrieval based on which files the agent has recently touched | Windsurf tracks recent actions; Copilot uses editor state | **P3** |

### 12.4 REMOVE (Unnecessary Complexity)

| Component | Reason | Confidence |
|-----------|--------|------------|
| **TF-IDF fallback embedder** | With a proper code-specific model, TF-IDF fallback adds complexity without proportional benefit. Consider removing or simplifying. | Medium -- keep as emergency fallback only if neural model fails to load |
| *(No other removals recommended)* | theo's architecture is lean. Each component serves a validated purpose. The issue is not excess complexity but insufficient implementation quality (embedding model, tuning). | -- |

### 12.5 Prioritized Action Plan

**Phase 1 -- Embedding Model Upgrade (Highest Impact)**
1. Replace AllMiniLM-L6-v2 with Jina Code Embeddings 0.5B as default.
2. Implement tree-sitter-based chunking for embedding (function/class level).
3. Re-run benchmark suite and measure impact on MRR, Recall@K, DepCov.
4. Expected impact: 15-25 percentage point improvement on retrieval quality.

**Phase 2 -- Graph Intelligence Enhancement**
1. Implement Personalized PageRank (PPR) with query-relevant seed nodes.
2. Upgrade to Jina Reranker v3.
3. Add external benchmark evaluation (CrossCodeEval, RepoBench-R).
4. Expected impact: 5-15 percentage point improvement.

**Phase 3 -- Fusion and Assembly Optimization**
1. Implement weighted RRF with per-retriever tuning.
2. Auto-tune MultiSignal weights via grid search on benchmark.
3. Add adaptive retrieval (skip dense search for high-confidence BM25 matches).
4. Expected impact: 3-8 percentage point improvement.

---

## Full Citation List

### Production Systems
1. [Dive into Claude Code: The Design Space of AI Agent Systems (arXiv:2604.14228)](https://arxiv.org/html/2604.14228v1)
2. [Claude Code Source Code Leak Analysis (MindStudio)](https://www.mindstudio.ai/blog/claude-code-source-leak-three-layer-memory-architecture)
3. [Claude Code Architecture Deep Dive (WaveSpeedAI)](https://wavespeed.ai/blog/posts/claude-code-architecture-leaked-source-deep-dive/)
4. [OpenAI Codex Developers Portal](https://developers.openai.com/codex)
5. [How Cursor Actually Indexes Your Codebase (Towards Data Science)](https://towardsdatascience.com/how-cursor-actually-indexes-your-codebase/)
6. [Building RAG on Codebases: Part 2 (LanceDB)](https://blog.lancedb.com/building-rag-on-codebases-part-2/)
7. [Windsurf Flow Context Engine (Markaicode)](https://markaicode.com/windsurf-flow-context-engine/)
8. [Windsurf Context Awareness Docs](https://docs.windsurf.com/context-awareness/overview)
9. [Copilot Context Handling Explained (M365FM)](https://www.m365.fm/blog/copilot-context-handling-explained/)
10. [GitHub Docs: Provide Context to Copilot](https://docs.github.com/en/copilot/how-tos/provide-context)
11. [Copilot CLI Context Management (GitHub Docs)](https://docs.github.com/en/copilot/concepts/agents/copilot-cli/context-management)
12. [OpenDev: Building AI Coding Agents (arXiv:2603.05344)](https://arxiv.org/html/2603.05344v2)
13. [Aider Repo Map (Aider Blog)](https://aider.chat/2023/10/22/repomap.html)
14. [Aider Repository Mapping System (DeepWiki)](https://deepwiki.com/Aider-AI/aider/4.1-repository-mapping)
15. [Continue.dev Context Providers Docs](https://docs.continue.dev/customize/custom-providers)
16. [How Augment Code Solved the Large Codebase Problem (Codacy)](https://blog.codacy.com/ai-giants-how-augment-code-solved-the-large-codebase-problem)
17. [Augment Code on Google Cloud](https://cloud.google.com/customers/augment)

### Academic Papers -- Code Retrieval
18. [Retrieval-Augmented Code Generation Survey (arXiv:2510.04905)](https://arxiv.org/abs/2510.04905)
19. [CodeRAG-Bench: Can Retrieval Augment Code Generation? (NAACL 2025, arXiv:2406.14497)](https://arxiv.org/abs/2406.14497)
20. [LocAgent: Graph-Guided LLM Agents for Code Localization (ACL 2025, arXiv:2503.09089)](https://arxiv.org/abs/2503.09089)
21. [SWE-Search: MCTS for Software Agents (ICLR 2025, arXiv:2410.20285)](https://arxiv.org/abs/2410.20285)
22. [AutoCodeRover: Autonomous Program Improvement (arXiv:2404.05427)](https://arxiv.org/html/2404.05427v2)
23. [Agentless (GitHub)](https://github.com/OpenAutoCoder/Agentless)
24. [RGFL: Reasoning Guided Fault Localization (arXiv:2601.18044)](https://arxiv.org/html/2601.18044)
25. [GrepRAG: Grep-Like Retrieval for Code Completion (arXiv:2601.23254)](https://arxiv.org/html/2601.23254)
26. [An Exploratory Study of Code Retrieval Techniques in Coding Agents (Preprints.org)](https://www.preprints.org/manuscript/202510.0924)
27. [CrossCodeEval: Multilingual Cross-File Completion (NeurIPS 2023, arXiv:2310.11248)](https://arxiv.org/abs/2310.11248)
28. [RepoFuse: Repository-Level Code Completion with Fused Dual Context (arXiv:2402.14323)](https://arxiv.org/html/2402.14323)
29. [Codified Context: Infrastructure for AI Agents (arXiv:2602.20478)](https://arxiv.org/html/2602.20478v1)

### Academic Papers -- RAG and Retrieval
30. [Agentic RAG Survey (arXiv:2501.09136)](https://arxiv.org/abs/2501.09136)
31. [GraphRAG: Retrieval-Augmented Generation with Graphs (arXiv:2501.00309)](https://arxiv.org/abs/2501.00309)
32. [Graph RAG Survey (ACM TOIS)](https://dl.acm.org/doi/10.1145/3777378)
33. [Towards Practical GraphRAG (arXiv:2507.03226)](https://arxiv.org/html/2507.03226)
34. [GRAG: Graph Retrieval-Augmented Generation (NAACL 2025)](https://aclanthology.org/2025.findings-naacl.232/)
35. [HippoRAG: Neurobiologically Inspired Long-Term Memory (NeurIPS 2024, arXiv:2405.14831)](https://arxiv.org/abs/2405.14831)
36. [HippoRAG 2: From RAG to Memory (ICML 2025, arXiv:2502.14802)](https://arxiv.org/abs/2502.14802)
37. [RAPTOR: Recursive Abstractive Processing for Tree-Organized Retrieval (ICLR 2024, arXiv:2401.18059)](https://arxiv.org/abs/2401.18059)
38. [Self-RAG: Learning to Retrieve, Generate and Critique (selfrag.github.io)](https://selfrag.github.io/)
39. [Meta Context Engineering via Agentic Skill Evolution (arXiv:2601.21557)](https://arxiv.org/abs/2601.21557)

### Embedding Models
40. [Qodo-Embed-1: SOTA Code Retrieval with Efficient Embedding (Qodo Blog)](https://www.qodo.ai/blog/qodo-embed-1-code-embedding-code-retrieval/)
41. [Voyage-code-3: More Accurate Code Retrieval (MongoDB)](https://www.mongodb.com/company/blog/voyage-code-3-more-accurate-code-retrieval-lower-dimensional-quantized-embeddings)
42. [Jina Code Embeddings: SOTA at 0.5B and 1.5B (Jina AI)](https://jina.ai/news/jina-code-embeddings-sota-code-retrieval-at-0-5b-and-1-5b/)
43. [6 Best Code Embedding Models Compared (Modal)](https://modal.com/blog/6-best-code-embedding-models-compared)
44. [Which Embedding Model in 2026 (Benchmark)](https://zc277584121.github.io/rag/2026/03/20/embedding-models-benchmark-2026.html)

### Rerankers
45. [Jina Reranker v3 (Jina AI)](https://jina.ai/models/jina-reranker-v3/)
46. [Reranker Benchmark: Top 8 Models (AIMultiple)](https://aimultiple.com/rerankers)
47. [Agentset Reranker Leaderboard](https://agentset.ai/rerankers)
48. [AnswerDotAI Rerankers Library (GitHub)](https://github.com/AnswerDotAI/rerankers)

### Search and Fusion
49. [Sourcegraph BM25F Blog](https://sourcegraph.com/blog/keeping-it-boring-and-relevant-with-bm25f)
50. [Tantivy: Full-Text Search Engine in Rust (GitHub)](https://github.com/quickwit-oss/tantivy)
51. [RRF in Hybrid Search (glaforge.dev)](https://glaforge.dev/posts/2026/02/10/advanced-rag-understanding-reciprocal-rank-fusion-in-hybrid-search/)
52. [Weighted RRF in Elasticsearch (Elastic Blog)](https://www.elastic.co/search-labs/blog/weighted-reciprocal-rank-fusion-rrf)
53. [ColBERT: State-of-the-Art Neural Search (GitHub)](https://github.com/stanford-futuredata/ColBERT)
54. [Late Interaction Overview: ColBERT, ColPali, ColQwen (Weaviate)](https://weaviate.io/blog/late-interaction-overview)

### Graph Analysis
55. [Code Property Graph (Apiiro Glossary)](https://apiiro.com/glossary/code-property-graph/)
56. [PageRank (Neo4j GDS Docs)](https://neo4j.com/docs/graph-data-science/current/algorithms/page-rank/)
57. [Community Detection (Neo4j GDS Docs)](https://neo4j.com/docs/graph-data-science/current/algorithms/community/)

### Context Engineering
58. [Mei et al.: Context Engineering Survey (arXiv:2507.13334)](https://arxiv.org/abs/2507.13334)
59. [Hua et al.: Context Engineering 2.0 (arXiv:2510.26493)](https://arxiv.org/abs/2510.26493)
60. [Token-Budget-Aware LLM Reasoning (ACL 2025)](https://aclanthology.org/2025.findings-acl.1274/)
61. [Budget-Aware Tool-Use for Agents (arXiv:2511.17006)](https://arxiv.org/html/2511.17006v1)

### Industry Analysis
62. [Why Cursor, Claude Code, Devin Use Grep Not Vectors (MindStudio)](https://www.mindstudio.ai/blog/is-rag-dead-what-ai-agents-use-instead)
63. [RAG Still Wins on Large Docs (MindStudio)](https://www.mindstudio.ai/blog/is-rag-dead-what-ai-coding-agents-use-instead)
64. [Agentic Search: How Coding Agents Find Code (Morph)](https://www.morphllm.com/agentic-search)
65. [Beyond Grep and Vectors: Reimagining Code Retrieval (Medium)](https://medium.com/@akshat_ilen/beyond-grep-and-vectors-reimagining-code-retrieval-for-ai-agents-85049e8cf9e9)
66. [Tree-sitter vs LSP (Lambda Land)](https://lambdaland.org/posts/2026-01-21_tree-sitter_vs_lsp/)
67. [Exploratory Study: Tree-sitter vs LSP for Agents (Preprints.org)](https://www.preprints.org/manuscript/202510.0924)
68. [Settling the RAG Debate (SmartScope)](https://smartscope.blog/en/ai-development/practices/rag-debate-agentic-search-code-exploration/)
