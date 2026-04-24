# theo-engine-graph — Revisao

> **Contexto**: Code graph via Tree-Sitter. Bounded Context: Code Intelligence.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Regra**: parser/graph sao read-only sobre o source code. Retrieval consome o graph, nunca o inverso.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `bridge` | Bridge entre representacoes (parser ↔ graph ↔ retrieval). | Pendente |
| 2 | `cluster` | Clustering de nodes do grafo (comunidades/afinidade). | Pendente |
| 3 | `cochange` | Analise de co-change (arquivos que mudam juntos via git). | Pendente |
| 4 | `git` | Integracao com git (blame, history, log). | Pendente |
| 5 | `model` | Modelo do grafo (nodes, edges, properties). | Pendente |
| 6 | `parse` | Parsing via Tree-Sitter (multi-linguagem). | Pendente |
| 7 | `persist` | Persistencia incremental do grafo (bincode). | Pendente |
| 8 | `scip::adapter` | Adaptador SCIP (Source Code Intelligence Protocol). | Pendente |
| 9 | `scip::indexer` | Indexador SCIP. | Pendente |
| 10 | `scip::merge` | Merge de multiplos indices SCIP. | Pendente |
| 11 | `scip::reader` | Leitor de arquivos SCIP gerados por outras ferramentas. | Pendente |
