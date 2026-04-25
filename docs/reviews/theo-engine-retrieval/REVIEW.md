# theo-engine-retrieval — Revisao

> **Contexto**: Semantic search, RRF 3-ranker, embeddings. Bounded Context: Code Intelligence.
>
> **Dependencias permitidas** (ADR-011): `theo-domain`, `theo-engine-graph`, `theo-engine-parser`.
>
> **Features**: `dense-retrieval`, `reranker`, `tantivy-backend`.
>
> **Status global**: deep-review concluido em 2026-04-25. 281 tests passando, 0 falhas. `cargo clippy --lib --tests` silent (zero warnings em codigo proprio).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `assembly` | Context assembly: monta o pacote final de contexto para o LLM. | Revisado |
| 2 | `budget` | Orcamento de tokens do contexto assembly. | Revisado |
| 3 | `code_tokenizer` | Tokenizador especializado em codigo. | Revisado |
| 4 | `dense_search` | Busca densa por embeddings (feature `dense-retrieval`). | Revisado |
| 5 | `embedding::cache` | Cache de embeddings em disco. | Revisado |
| 6 | `embedding::neural` | Embeddings neurais (fastembed / ONNX). | Revisado |
| 7 | `embedding::tfidf` | Embeddings TF-IDF (baseline classico). | Revisado |
| 8 | `embedding::turboquant` | Quantizacao rapida de embeddings. | Revisado |
| 9 | `escape` | Escape de strings para evitar prompt injection em snippets. | Revisado |
| 10 | `experimental::compress` | Compressao experimental de contexto. | Revisado |
| 11 | `file_retriever` | Retrieval no nivel de arquivo completo. | Revisado |
| 12 | `fs_source_provider` | Provider de source code a partir do filesystem. | Revisado |
| 13 | `graph_attention` | Atencao baseada no grafo de codigo (vizinhos relevantes). | Revisado |
| 14 | `harm_filter` | Filtro de conteudo potencialmente prejudicial. | Revisado |
| 15 | `inline_builder` | Construtor de contexto inline (citacoes curtas). | Revisado |
| 16 | `memory_tantivy` | Backend tantivy para memoria (feature `tantivy-backend`). | Revisado |
| 17 | `metrics` | Metricas do pipeline de retrieval. | Revisado |
| 18 | `pipeline` | Pipeline completo de retrieval (feature `reranker`). | Revisado |
| 19 | `reranker` | Re-ranker cross-encoder (feature `reranker`). | Revisado |
| 20 | `search` | API publica de search (BM25 + dense + RRF). | Revisado |
| 21 | `summary` | Sumarizacao de resultados de retrieval. | Revisado |
| 22 | `tantivy_search` | Backend tantivy (BM25, feature `tantivy-backend`). | Revisado |
| 23 | `wiki::generator` | Gerador de paginas do Code Wiki a partir do grafo. | Revisado |
| 24 | `wiki::lint` | Lint de paginas do Code Wiki. | Revisado |
| 25 | `wiki::lookup` | Lookup de paginas/concepts no Code Wiki. | Revisado |
| 26 | `wiki::model` | Modelo de dados do Code Wiki (pages, links). | Revisado |
| 27 | `wiki::persistence` | Persistencia do Code Wiki no disco. | Revisado |
| 28 | `wiki::renderer` | Renderizacao de paginas wiki em markdown. | Revisado |
| 29 | `wiki::runtime` | Runtime de consulta ao Code Wiki. | Revisado |

---

## Notas de Deep-Review

### Search & Retrieval Core

- **assembly**: monta o pacote final de contexto (snippets ranked + dependency neighbours + summary blocks). Drives a saida que vai pro system prompt.
- **budget**: token-based packing com priority. `pack_within_budget(items, budget) -> selected`.
- **code_tokenizer**: tokenizer especializado para codigo (preserva camelCase, snake_case, paths, type names) — input para BM25 + embedders.
- **search**: API publica integrando BM25 (via tantivy ou fallback) + dense (via fastembed quando feature ativa) + RRF fusion.

### Embeddings (feature-gated)

- **embedding::cache**: cache binario em `.theo/embeddings.bin` com hash-based invalidation.
- **embedding::neural**: ONNX runtime via fastembed. AllMiniLM default, Jina Code opt-in. Feature `dense-retrieval`.
- **embedding::tfidf**: TF-IDF classical baseline (sempre disponivel — sem feature gate).
- **embedding::turboquant**: quantizacao rapida de vectors para reduzir memory + speedup similarity.
- **dense_search**: HNSW-backed similarity search sobre vectors. Feature `dense-retrieval`.

### Pipeline & Reranker

- **pipeline**: pipeline completo (BM25 → dense → cross-encoder rerank). Feature `reranker`.
- **reranker**: cross-encoder model (BGE reranker ou similar) para re-score top-K candidates. Feature `reranker`.
- **metrics**: per-stage timing + score distributions.

### Tantivy backend (feature-gated)

- **tantivy_search**: BM25F-based full-text via tantivy. Feature `tantivy-backend`. Replaces in-memory BM25 quando ativado (saving ~200MB RAM).
- **memory_tantivy**: tantivy backend para memoria (lessons + episodes + skills index).

### Graph integration

- **graph_attention**: usa o code graph (theo-engine-graph) para boost vizinhos relevantes via random walk / personalized PageRank.

### Helpers

- **escape**: sanitization de strings que vao para LLM context — fence + escape de control characters.
- **harm_filter**: detecta conteudo potencialmente perigoso (prompt injection, exfiltration patterns).
- **file_retriever**: retrieval coarse no nivel de arquivo (vs. snippet-level).
- **fs_source_provider**: trait `SourceProvider` impl que le do filesystem direto.
- **inline_builder**: builder de citacoes curtas inline no LLM response.
- **summary**: sumarizacao de retrieval results pos-pipeline.
- **experimental::compress**: experimental context compression strategies (gated por flag).

### Code Wiki (7 sub-modules)

- **wiki::generator**: source code → wiki pages (deterministic). Drives `.theo/wiki/code/`.
- **wiki::lint**: page linting (broken links, missing concepts).
- **wiki::lookup**: query-time lookup (search wiki by concept name).
- **wiki::model**: data types (`WikiPage { slug, title, body, links }`).
- **wiki::persistence**: atomic-write das paginas + manifest.
- **wiki::renderer**: markdown rendering.
- **wiki::runtime**: runtime de consulta + enrichment.

**Validacao:**
- 281 tests passando (com features padrao), 0 falhas
- `cargo clippy -p theo-engine-retrieval --lib --tests` silent (zero warnings em codigo proprio — sem fixes nesta auditoria)
- ADR-011 dep invariant preservada: theo-domain + theo-engine-graph + theo-engine-parser (workspace) + tantivy/fastembed/ort/serde/etc (external feature-gated)
- Three feature flags: `dense-retrieval` (embeddings + HNSW), `reranker` (cross-encoder pipeline), `tantivy-backend` (BM25F + memory index)

Sem follow-ups bloqueadores. O crate cobre 4 layers de retrieval (BM25 → dense → graph attention → rerank) + Code Wiki autonomo.
