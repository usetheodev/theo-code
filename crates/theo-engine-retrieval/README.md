# theo-engine-retrieval

**Inteligência estrutural sobre código, derivada do código.**

Este crate NÃO é um sistema de documentação. Não gera markdown. Não cria
resumos que ficam stale. Ele extrai inteligência estrutural diretamente do
código-fonte via Tree-Sitter parse + grafo de dependências, e a expõe como
**ferramentas que o agente invoca sob demanda**.

## Princípio Fundamental

```
Código = fonte de verdade (muda a cada segundo)
Documentação = lixo (stale no instante em que é escrita)
Grafo estrutural = inteligência DERIVADA do código (sempre fresca)
```

O problema que este crate resolve:

- LLM não encontra o arquivo certo (falta overlap lexical com a query)
- LLM faz correlações incorretas (não sabe que auth.rs depende de session.rs)
- LLM monta contextos frágeis (inclui 3 arquivos quando precisava de 7)
- LLM não vê o codebase inteiro (não sabe quais módulos existem)

A solução NÃO é documentar o código. É dar ao LLM **ferramentas estruturais**
que entendem o grafo de dependências, a centralidade dos arquivos, e as
comunidades de código — tudo extraído do código real, recalculado a cada parse.

## Arquitetura: Graph-Augmented Agentic Retrieval

```
                     CÓDIGO (source of truth)
                         │
                    Tree-Sitter Parse
                         │
                         ▼
              ┌─────────────────────┐
              │     Code Graph      │  Symbols, edges, communities
              │  (theo-engine-graph) │  Recalculado do código real
              └──────────┬──────────┘
                         │
          ┌──────────────┼──────────────┐──────────────┐
          ▼              ▼              ▼              ▼
   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
   │ search() │   │ impact() │   │context() │   │repo_map()│
   │          │   │          │   │          │   │          │
   │ BM25 +   │   │ Graph    │   │ Graph +  │   │Community │
   │ Dense    │   │Attention │   │ Assembly │   │+ PageRank│
   └────┬─────┘   └────┬─────┘   └────┬─────┘   └────┬─────┘
        │              │              │              │
        └──────────────┴──────────────┴──────────────┘
                              │
                              ▼
                    LLM raciocina e decide
                    qual tool invocar next
```

**Quem toma a decisão é o LLM, não o pipeline.** Os algoritmos deste crate
são backends de tools — o agente escolhe quando e qual usar.

## O que cada componente faz

### Busca — "encontrar código"

| Módulo | Algoritmo | O que resolve |
|--------|-----------|---------------|
| `search/file_bm25.rs` | BM25F com field boosts (filename 5×, path 3×, symbols 3×) + PRF | Busca lexical code-aware: `getUserById` → [get, user, by, id] |
| `dense_search.rs` | Embeddings neurais (AllMiniLM-L6-v2) + cosine similarity + PRF | Busca semântica quando não há overlap lexical |
| `search/query_type.rs` | Classifier (Identifier / NaturalLanguage / Mixed) | Roteia para o ranker certo baseado no tipo de query |
| `search/tokenizing.rs` | Tokenizer code-aware (camelCase, snake_case, stem) | Entende que `HTMLParser` são [html, parser], não uma palavra |

### Grafo — "entender relações"

| Módulo | Algoritmo | O que resolve |
|--------|-----------|---------------|
| `graph_attention.rs` | Propagação de atenção no grafo (damping 0.5, multi-hop) | "Se auth.rs é relevante, session.rs e crypto.rs também são" |
| `search/signals.rs` | PageRank (sparse, 20 iter, damping 0.85) | "Quais são os arquivos mais centrais do codebase" |
| `search/multi.rs` | MultiSignalScorer (BM25 25% + Semantic 20% + FileBoost 20% + Graph 15% + PageRank 10% + Recency 10%) | Combina sinais para ranking holístico |

### Assembly — "montar contexto completo"

| Módulo | Algoritmo | O que resolve |
|--------|-----------|---------------|
| `assembly/greedy.rs` | Greedy knapsack por score dentro do token budget | Empacota o máximo de código relevante sem estourar o contexto |
| `budget.rs` | Alocação: repo_map 15% + modules 25% + code 40% + history 15% + reserve 5% | Divide o budget entre tipos de conteúdo |
| `escape.rs` | Context miss detection + 1-hop neighbor suggestion | Detecta quando um arquivo necessário ficou de fora e sugere expansão |
| `summary.rs` | Community summaries com symbols, edges, cross-deps | Mapa estrutural por comunidade (derivado do grafo, não de docs) |

### Embeddings — "representação vetorial"

| Módulo | O que faz |
|--------|-----------|
| `embedding/neural.rs` | NeuralEmbedder: AllMiniLM-L6-v2 (384-dim) ou Jina Code v2 (768-dim, opt-in) |
| `embedding/cache.rs` | Cache em disco com invalidação por graph hash — evita re-embed de 28s |
| `embedding/tfidf.rs` | Fallback TF-IDF → random projection quando neural falha |
| `embedding/turboquant.rs` | TurboQuant: compressão 2-bit (32×), ~5% perda de qualidade |

### Reranking — "refinar resultados"

| Módulo | O que faz |
|--------|-----------|
| `reranker.rs` | Cross-Encoder (Jina Reranker v2, ~568MB). Opcional, gated por config |
| `pipeline.rs` | RRF fusion (BM25 + Tantivy + Dense) → top-50 → reranker → top-20 |

### Métricas — "medir qualidade"

| Módulo | O que mede |
|--------|-----------|
| `metrics.rs` | MRR, Recall@K, Precision@K, nDCG@K, MAP, Dependency Coverage |

## O que NÃO é este crate

- **NÃO é documentação.** Nenhum markdown é gerado. Toda informação vem do parse do código.
- **NÃO é um pipeline automático.** O agente decide quando buscar, o que buscar, e quanto contexto montar.
- **NÃO é um substituto do LLM.** O grafo dá inteligência estrutural; o LLM dá raciocínio.
- **NÃO fica stale.** O grafo é recalculado do código via Tree-Sitter a cada análise.

## Métricas atuais

| Métrica | Floor SOTA | Atual | Status |
|---------|-----------|-------|--------|
| MRR | 0.90 | 0.695 | BELOW |
| Recall@5 | 0.92 | 0.507 | BELOW |
| Recall@10 | 0.95 | 0.577 | BELOW |
| DepCov | 0.96 | 0.767 | BELOW |
| nDCG@5 | 0.85 | 0.495 | BELOW |

Medidos com `cargo test -p theo-engine-retrieval --features dense-retrieval --test benchmark_suite`.

## Features (Cargo)

| Feature | O que habilita |
|---------|---------------|
| `dense-retrieval` | Embeddings neurais + Tantivy backend |
| `tantivy-backend` | Full-text index via Tantivy |
| `scip` | SCIP-based graph (via theo-engine-graph) |

## Dependências no workspace

```
theo-engine-retrieval
  → theo-domain        (tipos puros, zero deps)
  → theo-engine-graph  (grafo de código, clustering)
  → theo-engine-parser (Tree-Sitter, 14 linguagens)
```

Nenhuma dependência ascendente — este crate é uma leaf do grafo de deps.

## Direção futura

A transição de "pipeline automático" para "toolkit de tools agentic" mantém
todos os algoritmos e muda a interface:

1. **search()** → backend de tool de busca (BM25 ou Dense conforme query type)
2. **impact()** → backend de tool de análise de impacto (graph attention)
3. **context()** → backend de tool de montagem de contexto (assembly + DepCov)
4. **repo_map()** → backend de tool de mapa do repositório (communities + PageRank)
5. **symbols()** → backend de tool de inspeção de arquivo (via theo-engine-parser)

O LLM invoca o tool que precisa. O grafo fornece a inteligência. O código
é a fonte de verdade.
