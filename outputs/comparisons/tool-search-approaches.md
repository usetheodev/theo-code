---
type: comparison
topic: "tool search scoring approaches for Theo"
generated_at: 2026-04-26T15:00:00-03:00
---

# Comparison: Tool Search Scoring Approaches

## Side-by-Side

| Criterion | Substring (current) | Regex (Anthropic v1) | BM25 (Anthropic v2) | Dense Embedding | Hybrid BM25+Embed |
|---|---|---|---|---|---|
| **Query type** | Exact substring | Pattern matching | Natural language | Semantic similarity | Both |
| **Accuracy at 20-40 tools** | Adequate | Good | Good | Best | Best |
| **Accuracy at 200+ tools** | Poor | Fair | Good | Very good | Best |
| **Latency** | <1ms | <2ms | <5ms | 10-50ms | 15-60ms |
| **Implementation cost** | 0 (done) | Low (regex crate) | Low (Tantivy exists) | Medium (model needed) | High |
| **Handles synonyms** | No | No | Partial (via tokenization) | Yes | Yes |
| **Handles typos** | No | With patterns | Partial | Yes | Yes |
| **External dependency** | None | `regex` crate | `tantivy` (already in workspace) | Embedding model | Both |
| **Anthropic alignment** | No equivalent | `tool_search_tool_regex` | `tool_search_tool_bm25` | Custom tool_search | Custom tool_search |
| **Production precedent** | None known | Claude Code (regex variant) | Claude Code (BM25 variant), OpenDev | Tool-to-Agent Retrieval (ACL 2025) | KDD 2025 Multi-Agent RAG |

## Recommendation

**BM25 via Tantivy** is the sweet spot for Theo:

1. Matches Anthropic's BM25 variant semantics
2. Zero new dependencies (Tantivy already in workspace)
3. Natural language queries work (unlike regex, which requires the LLM to construct patterns)
4. Sufficient accuracy for Theo's tool catalog size (20-50 tools)
5. Leaves the door open for hybrid scoring later (Theo's RRF ranker already fuses BM25 + embedding scores)

Dense embeddings should be deferred (YAGNI) until Theo's tool catalog exceeds 100 tools, which is unlikely in the near term given the current 42-tool manifest.

## Anti-Pattern to Avoid

Do NOT implement the regex variant. It requires the LLM to construct regex patterns, which:
- Adds cognitive load to the model
- Wastes output tokens on pattern construction
- Is brittle for weaker models (Codex, Qwen)
- Anthropic's BM25 variant exists precisely because regex was insufficient for natural language tool discovery
