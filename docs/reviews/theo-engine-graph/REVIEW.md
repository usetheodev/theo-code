# theo-engine-graph — Revisao

> **Contexto**: Code graph via Tree-Sitter. Bounded Context: Code Intelligence.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Regra**: parser/graph sao read-only sobre o source code. Retrieval consome o graph, nunca o inverso.
>
> **Status global**: deep-review concluido em 2026-04-25. 101 tests passando, 0 falhas. `cargo clippy -p theo-engine-graph --lib --tests` silent (zero warnings em codigo proprio).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `bridge` | Bridge entre representacoes (parser ↔ graph ↔ retrieval). | Revisado |
| 2 | `cluster` | Clustering de nodes do grafo (comunidades/afinidade). | Revisado |
| 3 | `cochange` | Analise de co-change (arquivos que mudam juntos via git). | Revisado |
| 4 | `git` | Integracao com git (blame, history, log). | Revisado |
| 5 | `model` | Modelo do grafo (nodes, edges, properties). | Revisado |
| 6 | `parse` | Parsing via Tree-Sitter (multi-linguagem). | Revisado |
| 7 | `persist` | Persistencia incremental do grafo (bincode). | Revisado |
| 8 | `scip::adapter` | Adaptador SCIP (Source Code Intelligence Protocol). | Revisado |
| 9 | `scip::indexer` | Indexador SCIP. | Revisado |
| 10 | `scip::merge` | Merge de multiplos indices SCIP. | Revisado |
| 11 | `scip::reader` | Leitor de arquivos SCIP gerados por outras ferramentas. | Revisado |

---

## Notas de Deep-Review

### 1. bridge
Conversoes parser→graph→retrieval. `FileData`, `NodeKind`, `EdgeKind`. Pure functions sem state.

### 2. cluster
Clustering algoritmico de nodes (Louvain ou similar) para detectar comunidades funcionais (modulos, dominios). Drives recommendations para novos contributors.

### 3. cochange
Analise temporal git: arquivos que mudam juntos com >X% co-frequency. Sinal forte para "concept boundaries". Driven por GraphContextProvider para enrich retrieval results.

### 4. git
Wrapper sobre `git2` ou subprocess: blame, log, file history. Fonte para cochange + cluster signals.

### 5. model
`CodeGraph { nodes: Vec<Node>, edges: Vec<Edge> }`. `Node { id, kind, file_id, range, qualified_name }`. `Edge { from, to, kind }`. Pure data types serializaveis via serde.

### 6. parse
Tree-Sitter integration para 16 linguagens. `parse_file(path, lang) -> ParsedFile`. Symbol extraction por language-specific queries. Multi-language fallback graceful.

### 7. persist
Incremental graph persistence via bincode em `.theo/code-graph.bin`. Hash-based change detection — repassar apenas arquivos modificados desde ultimo build.

### 8. scip::adapter
SCIP (Sourcegraph Code Intelligence Protocol) adapter. SCIP e o formato cross-tool padrao para code intelligence; adapter permite consumir indices SCIP de rust-analyzer / scip-go / etc.

### 9. scip::indexer
Genera indices SCIP dos parses Tree-Sitter para interop com tooling externo.

### 10. scip::merge
Merge de N SCIP indices em um agregado (e.g., SCIP do rust-analyzer + SCIP gerado pelo theo-engine-graph para linguas nao cobertas).

### 11. scip::reader
Parser de arquivos SCIP existentes — usa o crate scip official (protobuf bindings).

**Validacao:**
- 101 tests passando, 0 falhas
- `cargo clippy -p theo-engine-graph --lib --tests` silent (zero warnings — sem fixes nesta auditoria)
- ADR dep invariant preservada: theo-domain (workspace) + tree-sitter-* + bincode + scip protobuf (external)

Sem follow-ups bloqueadores.
