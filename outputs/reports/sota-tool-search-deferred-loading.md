---
type: report
question: "What is the state of the art in deferred/lazy tool loading for LLM agents, and how should Theo implement tool_search?"
generated_at: 2026-04-26T15:00:00-03:00
confidence: 0.88
sources_used: 18
---

# Report: SOTA Tool Search and Deferred Tool Loading for LLM Agents

## Executive Summary

Deferred tool loading via a `tool_search` meta-tool is now industry consensus for LLM agents with 10+ tools. Anthropic's official API supports two server-side variants (regex and BM25) with measured 85% token reduction and accuracy improvements from 49% to 74% (Opus 4) on large catalogs. Theo's current implementation has the correct architectural skeleton (`should_defer`, `search_hint`, `search_deferred` with substring matching) but uses a naive substring-contains scoring algorithm that will not scale. The recommended upgrade path is: (1) promote the existing 6-8 planning/git tools to deferred, (2) replace substring matching with BM25 scoring using Theo's existing Tantivy infrastructure, and (3) add a dynamic unload mechanism for long sessions.

## Analysis

### Finding 1: Anthropic's Official Tool Search API Sets the Standard

Anthropic's tool search tool (released November 2025, documented at `platform.claude.com`) defines the protocol:

- **Two variants**: `tool_search_tool_regex_20251119` (Claude constructs Python regex patterns) and `tool_search_tool_bm25_20251119` (Claude uses natural language queries) [Source 1]
- **`defer_loading: true`** flag on tool definitions tells the API to exclude the tool from the system prompt prefix
- **Returns 3-5 tools per search** -- not all matches, a curated shortlist
- **Searches over**: tool names, descriptions, argument names, and argument descriptions
- **Scales to 10,000 tools** per catalog
- **Prompt caching preserved**: deferred tools are appended inline as `tool_reference` blocks; the prefix is untouched [Source 1]
- **Strict mode compatible**: grammar builds from the full toolset regardless of deferral

Key recommendation from Anthropic: **keep 3-5 most frequently used tools as non-deferred**. This is the "core set" that the agent always sees.

**Measured results (Anthropic internal):**
- 85% reduction in token usage with tool search vs. all-tools-loaded [Source 1]
- Opus 4: accuracy 49% -> 74% on MCP evaluations with tool search enabled [Source 3]
- Opus 4.5: accuracy 79.5% -> 88.1% [Source 3]
- Real-world: 51K tokens down to 8.5K (46.9% reduction in total agent tokens) [Source 4]
- Threshold trigger: tool definitions exceeding 10K tokens [Source 2]

### Finding 2: Claude Code's Five-Layer Context Architecture

Claude Code (v2.1.69+) defers ALL built-in tools behind ToolSearch, not just MCP tools [Source 5]. The arxiv analysis (2604.14228) documents a five-layer compaction pipeline:

1. **Budget Reduction** -- per-message size limits on tool results
2. **Snip** -- lightweight temporal trim of older history
3. **Microcompact** -- fine-grained compression with cache-aware processing
4. **Context Collapse** -- read-time projection persisting across turns
5. **Auto-compact** -- model-generated semantic compression as fallback

Claude Code's tool pool: ~54 tools (19 unconditional, 35 conditional on feature flags). The tool assembly function `assembleToolPool()` merges built-in + MCP tools and pre-filters denied tools before the model sees them [Source 6].

**Known issues with full deferral:**
- First-turn unavailability breaks automated/headless tasks (mitigated by `--eager-tools` flag) [Source 5]
- Context compression losing deferred references after `/compact` (mitigated by marking references non-compressible) [Source 5]
- Hook conflicts causing agent hangs after deferred tool loading [Source 5]

### Finding 3: OpenDev's Quantified Lazy Discovery Results

The OpenDev paper (ZenML reference) reports the most rigorous measurements of lazy tool loading in a terminal-native coding agent:

- **Startup context cost**: reduced from 40% to under 5% with lazy MCP discovery [Source 7]
- **Peak context consumption**: ~54% reduction [Source 7]
- **Session length**: extended from 15-20 turns to 30-40 turns without emergency compaction [Source 7]
- **Two-phase skill loading**: name+description metadata index at startup; full content loaded only at invocation [Source 7]

OpenDev's `should_defer` trait (traits.rs:547-575) is the pattern Theo already references in its codebase. The key insight: **expensive schemas are the primary deferral criterion, not just rarity**.

### Finding 4: Scoring Algorithms -- What Works Best

The academic literature and production systems converge on a clear ranking of approaches:

| Algorithm | Tool Selection Accuracy | Latency | Implementation Cost |
|---|---|---|---|
| Substring contains (Theo current) | Low (exact match only) | <1ms | Trivial |
| BM25 sparse scoring | Good (lexical matching) | <5ms | Low (Tantivy) |
| Dense embedding retrieval | Best for semantic queries (+40-60pp vs BM25) | 10-50ms | Medium |
| Hybrid BM25 + embedding | Highest (0.816 Recall@5 per KDD 2025) | 15-60ms | High |
| LLM reranking on top | Marginal gains over hybrid | 200ms+ | Very high |

**Recommended for Theo**: BM25 scoring via Tantivy. Rationale:
- Theo already has Tantivy as a workspace dependency in `theo-engine-retrieval` [Source: codebase]
- Tool catalogs in coding agents are 20-200 tools, not 10,000 -- BM25 suffices at this scale
- Embeddings add latency and complexity for marginal gains at <200 tools
- BM25 matches on tool names, descriptions, and parameter names -- the same fields Anthropic searches [Source 1]

The 30/70 BM25/embedding weight split is optimal for document retrieval (KDD 2025) but overkill for tool catalogs of this size [Source 10].

### Finding 5: The Core vs. Deferred Split

Multiple sources converge on the same heuristic:

| Source | Core Tools | Deferred Threshold | Notes |
|---|---|---|---|
| Anthropic docs | 3-5 most used | `defer_loading: true` | [Source 1] |
| Claude Code | 19 unconditional | 35 conditional | Feature-flag gated [Source 6] |
| OpenDev | Metadata index only | Full schema on invoke | Two-phase approach [Source 7] |
| Tool RAG (Red Hat) | "Always loaded" set | Retrieved on demand | Triple accuracy vs. naive [Source 8] |

**For Theo's 27 default registry tools**, the recommended core set (always visible) is:

1. `read` -- most-called tool in any coding agent
2. `write` -- second most-called
3. `edit` -- file modification
4. `bash` -- shell execution
5. `grep` -- search
6. `glob` -- file discovery
7. `think` -- cognitive (zero-cost, prevents wasted tool calls)
8. `done` -- meta-tool (always needed for completion signal)

**Candidates for deferral** (14 tools):
- Planning tools: `plan_create`, `plan_update_task`, `plan_advance_phase`, `plan_log`, `plan_summary`, `plan_next_task` (6 tools -- expensive schemas, used only in plan mode)
- Git tools: `git_status`, `git_diff`, `git_log`, `git_commit` (4 tools -- used only during commit workflows)
- `webfetch`, `http_get`, `http_post` (3 tools -- rarely needed)
- `codebase_context` (1 tool -- expensive, rarely called)

**Estimated token savings**: Each deferred tool with schema + description averages 150-300 tokens. Deferring 14 tools saves ~2,100-4,200 tokens per turn from the system prompt. Over a 30-turn session, this compounds to 63K-126K saved input tokens.

### Finding 6: Competitor Approaches

**Cursor**: Uses Merkle tree-based indexing for codebase understanding. Tool management is opaque (proprietary), but context is limited to 10K-50K tokens practically. No public tool search mechanism [Source 11].

**Windsurf**: Graph-based dependency models. Cascade system reasons across codebase. No public deferred tool loading -- but context window (200K via RAG) mitigates the need [Source 11].

**Codex CLI (OpenAI)**: The OpenAI Agents SDK now has `deferLoading: true` with tool search (requires GPT-5.4+), confirming cross-vendor convergence [Source 2].

**SWE-agent**: Uses lazy module loading (Python imports deferred until CLI command invoked) but does not have tool-level deferral for the LLM [Source 7].

**CrewAI**: Added dynamic tool injection via Anthropic's tool search API in v1.10.2a1 (March 2026) [Source 2].

### Finding 7: Dynamic Unloading -- The Next Frontier

Claude Code issue #44536 proposes that tools loaded via ToolSearch that haven't been used for N turns should return to deferred state, so context self-cleans over long sessions [Source 5]. This is not yet implemented anywhere in production but addresses the "context accumulation" problem where discovered tools stay loaded indefinitely.

Theo could implement this as an extension to the compaction pipeline: after compaction, check which deferred-then-loaded tools have zero invocations in the surviving context window, and re-defer them.

## Gaps

1. **No benchmark for Theo's specific tool catalog** -- we have not measured how many tokens our 27 tools consume or what accuracy degradation occurs at this scale
2. **No telemetry on tool usage frequency** -- the core vs. deferred split is based on intuition, not data. We should instrument which tools are called per session before committing to a split
3. **BM25 vs. substring on Theo's tool descriptions** -- no A/B comparison exists. The recommendation is based on external evidence
4. **Dynamic unload** is unproven in production -- no system has shipped it yet
5. **Interaction with prompt caching** -- Theo does not use Anthropic's prompt caching API directly; the `tool_reference` inline-append pattern may not apply cleanly

## Recommendations

### P0: Instrument Tool Usage (prerequisite, 1 day)

Before optimizing, measure. Add a counter to `execute_tool_call` that logs tool invocation frequency per session. After 1 week of usage data, validate the core vs. deferred split empirically.

### P1: Promote Planning + Git Tools to Deferred (2 days)

The 6 planning tools and 4 git tools are the clearest deferral candidates:
- They are mode-specific (plan mode, commit workflow)
- Their schemas are among the largest
- They are never needed in the first turn

Implementation:
- Override `should_defer()` -> `true` on each tool
- Add `search_hint()` with keyword phrases: e.g., `"create plan project tasks phases"`, `"git commit status diff log"`
- Estimated saving: ~1,500-3,000 tokens per turn

### P2: Upgrade search_deferred to BM25 (3 days)

Replace `search_deferred`'s substring-contains with Tantivy BM25:
- Create a lightweight in-memory Tantivy index at registry creation time
- Index fields: tool id, description, search_hint, parameter names
- Return top-5 results by BM25 score (matching Anthropic's cardinality)
- This aligns with Theo's existing Tantivy expertise in `theo-engine-retrieval`

### P3: Context-Aware Tool Injection (5 days)

Instead of static `should_defer`, implement dynamic deferral based on agent mode:
- `--mode plan`: planning tools are core, git tools deferred
- `--mode agent`: file/search/edit are core, planning tools deferred
- `--mode ask`: minimal core set (read + grep + think + done only)

This is a natural extension of the existing `ToolExposure` enum and aligns with Anthropic's principle of "just-in-time context."

### P4: Dynamic Unload (future, after P0 data)

If P0 telemetry shows that long sessions (30+ turns) accumulate too many discovered tools, implement turn-based unloading: tools not invoked in the last 10 turns return to deferred state. Gate this behind a feature flag.

## Sources

1. [Anthropic Tool Search Tool Documentation](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool)
2. [Claude Code Lazy Loading for MCP Tools (Medium)](https://jpcaparas.medium.com/claude-code-finally-gets-lazy-loading-for-mcp-tools-explained-39b613d1d5cc)
3. [Claude Code MCP Tool Search: Save 95% Context](https://claudefa.st/blog/tools/mcp-extensions/mcp-tool-search)
4. [Claude Code Cut MCP Context Bloat by 46.9% (Medium)](https://medium.com/@joe.njenga/claude-code-just-cut-mcp-context-bloat-by-46-9-51k-tokens-down-to-8-5k-with-new-tool-search-ddf9e905f734)
5. [Claude Code Issue #44536: Lazy context loading](https://github.com/anthropics/claude-code/issues/44536) and [Issue #31002: Built-in tools deferred](https://github.com/anthropics/claude-code/issues/31002)
6. [Dive into Claude Code: arxiv 2604.14228](https://arxiv.org/html/2604.14228v1)
7. [OpenDev: Terminal-Native AI Coding Agent (ZenML)](https://www.zenml.io/llmops-database/terminal-native-ai-coding-agent-with-multi-model-architecture-and-adaptive-context-management)
8. [Tool RAG: The Next Breakthrough (Red Hat)](https://next.redhat.com/2025/11/26/tool-rag-the-next-breakthrough-in-scalable-ai-agents/)
9. [LLM-Based Agents for Tool Learning: A Survey (Springer)](https://link.springer.com/article/10.1007/s41019-025-00296-9)
10. [Tool-to-Agent Retrieval (ACL 2025)](https://arxiv.org/pdf/2511.01854)
11. [Cursor vs Claude Code vs Windsurf Comparison](https://www.shareuhack.com/en/posts/cursor-vs-claude-code-vs-windsurf-2026)
12. [Anthropic Advanced Tool Use](https://www.anthropic.com/engineering/advanced-tool-use)
13. [Anthropic Define Tools Documentation](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools)
14. [Benchmarking Tool Retrieval for LLMs (ACL 2025)](https://aclanthology.org/2025.findings-acl.1258.pdf)
15. [Tool Selection Accuracy in AI Agents (EmergentMind)](https://www.emergentmind.com/topics/tool-selection-accuracy-ts)
16. [SWE-agent (NeurIPS 2024)](https://github.com/SWE-agent/SWE-agent)
17. [Claude Code Issue #7336: Feature Request Lazy Loading](https://github.com/anthropics/claude-code/issues/7336)
18. [Anthropic Effective Context Engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
