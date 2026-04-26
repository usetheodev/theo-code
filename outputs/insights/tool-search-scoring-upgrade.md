---
type: insight
topic: "tool_search scoring algorithm upgrade"
confidence: 0.92
impact: high
---

# Insight: Theo's substring-contains tool search will fail at scale; BM25 via Tantivy is the minimum viable upgrade

**Key finding:** Theo's `search_deferred` uses case-insensitive substring matching (`id.contains(&q) || hint.contains(&q)`), which produces zero results for semantic queries like "create a project breakdown" when the tool hint is "plan project tasks phases". Every production system (Claude Code, OpenDev, CrewAI) uses BM25 or regex as the minimum, with Anthropic's API offering both variants natively.

**Evidence:**
- Anthropic's tool search API offers two variants: regex (pattern-based) and BM25 (natural language). Both search over tool names, descriptions, argument names, and argument descriptions. Returns top 3-5 results. [Source: platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool]
- Dense retrieval shows +40-60 absolute points over BM25 for tool selection (EmergentMind survey), but only matters at 200+ tools. At Theo's scale (27-40 tools), BM25 suffices.
- Theo already has Tantivy as a workspace dependency in `theo-engine-retrieval` with proven BM25 scoring infrastructure. The RRF 3-ranker (MRR=0.914) demonstrates the team's expertise.

**Implication for Theo:**
Replace `ToolRegistry::search_deferred` with a Tantivy-backed BM25 index built at registry creation time. Index four fields: `id`, `description`, `search_hint`, `param_names` (joined). Return top-5 by BM25 score. This is a 2-3 day task that reuses existing Tantivy expertise, aligns with Anthropic's production pattern, and unblocks the deferral of 10-14 tools currently consuming ~2-4K tokens per turn.
