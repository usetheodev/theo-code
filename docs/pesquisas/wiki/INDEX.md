# Wiki — Pesquisa SOTA

## Escopo
3 wiki tools: wiki_generate, wiki_query, wiki_ingest. Code wiki from AST graph, BM25 search, runtime insight ingestion.

## Crates alvo
- `theo-tooling` — wiki tools
- `theo-infra-memory` — WikiMemoryProvider (shared with memory domain)

## Referências-chave
| Fonte | O que extrair |
|-------|---------------|
| Karpathy LLM Wiki | raw → compile → query, self-healing loop, compounding knowledge |
| llm-wiki-compiler | Two-phase pipeline, SHA-256 incremental, frontmatter format |
| llm-wiki-compiler lint | 6 rules (broken wikilinks, orphans, duplicates, empty pages) |
| qmd | BM25 + vector search over markdown, MCP tools |

## Arquivos nesta pasta
- (pesquisas sobre wiki system vão aqui)

## Gaps para pesquisar
- Wiki compilation cost: benchmark on real repo (Karpathy pattern)
- Wiki vs memory overlap: clear boundary between wiki and LTM-semantic
- Wiki lint: port llm-wiki-compiler 6 rules to Rust
- Wiki query: integration with theo-engine-retrieval RRF
