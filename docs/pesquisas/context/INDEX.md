# Context Engineering — Pesquisa SOTA

## Escopo
GRAPHCTX assembly, RRF 3-ranker fusion, BM25 + embeddings + graph, prompt caching, token budget management, dependency coverage, graph clustering, incremental indexing, representation format.

## Crates alvo
- `theo-engine-retrieval` — RRF fusion, BM25/tantivy, context assembly
- `theo-engine-graph` — code graph construction, clustering
- `theo-engine-parser` — Tree-Sitter extraction (14 languages)

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| Tsinghua representation | +16.8 SWE-Bench com structured NL vs code-native |
| qmd | BM25 + vector + LLM reranking, RRF fusion, AST-aware chunking |
| fff.nvim | Frecency scoring, bigram prefilter, Smith-Waterman fuzzy |
| opendev | Staged compaction 5 thresholds, ContextPicker dinâmico |
| hermes-agent | Context compression via auxiliary LLM, prompt caching |
| Anthropic cache_control | 5-min/1-hour TTLs, 84% reduction |

## Arquivos nesta pasta
- (mover `context-engine.md` para cá quando apropriado)

## Gaps para pesquisar
- NDCG@5 benchmark (currently unmeasured)
- Per-language Recall@5 (need per-lang benchmark suite)
- Cache hit rate measurement (60-80% target)
- Graph clustering quality metrics
