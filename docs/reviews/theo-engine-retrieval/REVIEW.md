# theo-engine-retrieval — Revisao

> **Contexto**: Semantic search, RRF 3-ranker, embeddings. Bounded Context: Code Intelligence.
>
> **Dependencias permitidas** (ADR-011): `theo-domain`, `theo-engine-graph`, `theo-engine-parser`.
>
> **Features**: `dense-retrieval`, `reranker`, `tantivy-backend`.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `assembly` | Context assembly: monta o pacote final de contexto para o LLM. | Pendente |
| 2 | `budget` | Orcamento de tokens do contexto assembly. | Pendente |
| 3 | `code_tokenizer` | Tokenizador especializado em codigo. | Pendente |
| 4 | `dense_search` | Busca densa por embeddings (feature `dense-retrieval`). | Pendente |
| 5 | `embedding::cache` | Cache de embeddings em disco. | Pendente |
| 6 | `embedding::neural` | Embeddings neurais (fastembed / ONNX). | Pendente |
| 7 | `embedding::tfidf` | Embeddings TF-IDF (baseline classico). | Pendente |
| 8 | `embedding::turboquant` | Quantizacao rapida de embeddings. | Pendente |
| 9 | `escape` | Escape de strings para evitar prompt injection em snippets. | Pendente |
| 10 | `experimental::compress` | Compressao experimental de contexto. | Pendente |
| 11 | `file_retriever` | Retrieval no nivel de arquivo completo. | Pendente |
| 12 | `fs_source_provider` | Provider de source code a partir do filesystem. | Pendente |
| 13 | `graph_attention` | Atencao baseada no grafo de codigo (vizinhos relevantes). | Pendente |
| 14 | `harm_filter` | Filtro de conteudo potencialmente prejudicial. | Pendente |
| 15 | `inline_builder` | Construtor de contexto inline (citacoes curtas). | Pendente |
| 16 | `memory_tantivy` | Backend tantivy para memoria (feature `tantivy-backend`). | Pendente |
| 17 | `metrics` | Metricas do pipeline de retrieval. | Pendente |
| 18 | `pipeline` | Pipeline completo de retrieval (feature `reranker`). | Pendente |
| 19 | `reranker` | Re-ranker cross-encoder (feature `reranker`). | Pendente |
| 20 | `search` | API publica de search (BM25 + dense + RRF). | Pendente |
| 21 | `summary` | Sumarizacao de resultados de retrieval. | Pendente |
| 22 | `tantivy_search` | Backend tantivy (BM25, feature `tantivy-backend`). | Pendente |
| 23 | `wiki::generator` | Gerador de paginas do Code Wiki a partir do grafo. | Pendente |
| 24 | `wiki::lint` | Lint de paginas do Code Wiki. | Pendente |
| 25 | `wiki::lookup` | Lookup de paginas/concepts no Code Wiki. | Pendente |
| 26 | `wiki::model` | Modelo de dados do Code Wiki (pages, links). | Pendente |
| 27 | `wiki::persistence` | Persistencia do Code Wiki no disco. | Pendente |
| 28 | `wiki::renderer` | Renderizacao de paginas wiki em markdown. | Pendente |
| 29 | `wiki::runtime` | Runtime de consulta ao Code Wiki. | Pendente |
