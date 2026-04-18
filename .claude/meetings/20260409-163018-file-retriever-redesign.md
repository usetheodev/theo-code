---
id: 20260409-163018
date: 2026-04-09
topic: "CRITICO: File Retriever — community e unidade errada de retrieval"
verdict: REVISED
participants: 16
---

# Reuniao: File Retriever — Redesign do Pipeline de Retrieval

## Pauta

**Contexto**: Benchmark real em 7 repos revelou MRR=0.43, Precision@5=0.10. Diagnostico: community como unidade de retrieval e o bug estrutural. Pipeline otimizado para gerar conhecimento (wiki), nao recuperar com precisao.

**Questoes**: (1) Concordam com diagnostico? (2) Como implementar file-level retrieval? (3) Quantos tipos novos? (4) Como nao quebrar 1023 testes?

---

## Posicoes por Agente

### Estrategia

| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE | Diagnostico correto. FileBm25 ja existe — reusa-lo. 3 tipos novos suficientes (nao 5). Phase 1: BM25 file + community flatten + sort. NAO mudar assinatura de GraphContextProvider. |
| evolution-agent | APPROVE | Community vira contexto de expansao, nao resultado. Fallback mantido. |

### Conhecimento

| Agente | Posicao | Resumo |
|--------|---------|--------|
| knowledge-compiler | APPROVE | Wiki continua como layer de grounding e summary. Nao e afetada. |
| ontology-manager | APPROVE | 2 tipos novos (nao 5): RankedFile + Signal enum. Ficam em theo-engine-retrieval, NAO em theo-domain. |
| data-ingestor | APPROVE | CodeGraph JA e o file index. Nao precisa de indice separado. |
| wiki-expert | APPROVE | Wiki lookup continua como layer 1 (<5ms). FileRetriever e layer 2 ortogonal. |

### Qualidade

| Agente | Posicao | Resumo |
|--------|---------|--------|
| validator | CONDICIONAL APPROVE | 3 invariantes obrigatorias: (1) ghost path filter, (2) expansion limit hardcoded, (3) wiki lookup permanece layer 1. TDD gate bloqueante. |
| linter | APPROVE | Nenhum tipo novo em theo-domain = complexidade controlada. |
| retrieval-engineer | APPROVE | graph_proximity via cosine adjacency, expansion so Calls+Imports. Nao CoChanges (ruido para query expansion). |
| memory-synthesizer | APPROVE | Memory injection compativel com novo pipeline. |

### Engineering

| Agente | Posicao | Resumo |
|--------|---------|--------|
| code-reviewer | APPROVE | FileBm25 line 552 ja faz file-level BM25. Extend, nao duplicate. |
| graphctx-expert | APPROVE | graph_neighbors_for_file e wrapper thin sobre adjacency existente. Expansion: Calls+Imports only. |
| arch-validator | APPROVE | Sem violacao de fronteira. Novos modulos em theo-engine-retrieval. |
| test-runner | CONCERN | Precisa de scope confirmation e ground truth antes de TDD plans. |
| frontend-dev | APPROVE | Diagnostics do retriever podem alimentar context bar futura. |

### Pesquisa

| Agente | Posicao | Resumo |
|--------|---------|--------|
| research-agent | APPROVE | SOTA unanime: file-level e correto. Zoekt, Sourcegraph, RANGER, Aider todos usam file-level. Community BM25 ja marcado dead_code no proprio codebase. |

---

## Conflitos

### Conflito 1: Quantos tipos novos?

**Proposta original**: 5 tipos em theo-domain (RetrievalQuery, RetrievalResult, RankedFile, RankingReason, RerankWeights)
**Chief-architect**: 3 tipos suficientes
**Ontology-manager**: 2 tipos suficientes, ficam em theo-engine-retrieval

**Resolucao**: **Ontology-manager vence**. 2 tipos (RankedFile + Signal enum) em theo-engine-retrieval. NAO poluir theo-domain com internals de retrieval. RetrievalQuery nao necessario (ja e &str + budget). RerankWeights duplica ScoringWeights existente.

### Conflito 2: File Index separado vs reusar CodeGraph?

**Proposta original**: Novo FileIndex struct
**Infra/research**: CodeGraph JA e o file index. FileBm25 ja itera File nodes.

**Resolucao**: **Infra vence**. NAO criar FileIndex separado. Reusar CodeGraph + FileBm25 existente. Para repos 21K+, tantivy-backend feature (ja implementado) resolve.

### Conflito 3: Expansion edge types

**Proposta original**: Todos os tipos de edge
**GraphCTX-expert**: Somente Calls + Imports. CoChanges e ruido para expansion. Tests alimentam separadamente.

**Resolucao**: **GraphCTX-expert vence**. Expansion = Calls + Imports only. Tests via compute_test_proximity separado.

---

## Consensos

1. **UNANIME**: Diagnostico correto — community como unidade de retrieval e o bug
2. **UNANIME**: FileBm25 ja existe e e a base certa
3. **UNANIME**: Wiki lookup permanece layer 1 (nao deslocar)
4. **UNANIME**: NAO criar FileIndex separado — CodeGraph e o indice
5. **FORTE**: 2 tipos novos em retrieval (nao 5 em domain)
6. **FORTE**: Expansion = Calls + Imports only
7. **FORTE**: Ghost path filter obrigatorio antes do reranker
8. **FORTE**: max_neighbors hardcoded no graph expander

---

## Decisoes

### 1. Arquitetura aprovada (simplificada)

```
Query
  |
  v
[Wiki Lookup] (layer 1, <5ms, existente)
  |
  v  Se confidence alta → retorno direto
  |
[FileBm25::search] (layer 2, existente, file-level)
  |
  v  Top 50 files
[Community Flatten] (candidatos adicionais de top communities)
  |
  v  Candidate Pool (~80 files)
[Simple Reranker] (4-6 features iniciais)
  |
  v  Top 5-8 files
[Graph Expansion] (1-hop Calls+Imports, max_neighbors=15)
  |
  v  Primary + Secondary files
[Context Assembler] (files first, wiki summary as backup)
```

### 2. Tipos novos (2 apenas, em theo-engine-retrieval)

```rust
pub struct RankedFile {
    pub path: String,
    pub score: f64,
    pub signals: Vec<(Signal, f64)>,
}

pub enum Signal {
    Bm25Content,
    Bm25Path,
    SymbolMatch,
    CommunityScore,
    CoChange,
    Recency,
    GraphProximity,
    TestProximity,
}
```

### 3. Novos modulos (em theo-engine-retrieval)

- `file_retriever.rs` — orquestrador do pipeline file-first
- Reutiliza: `FileBm25`, `ScoringWeights`, `assembly`

### 4. Novo metodo (em theo-engine-graph)

```rust
impl CodeGraph {
    pub fn file_neighbors(&self, file_id: &str, edge_types: &[EdgeType], max: usize) -> Vec<String>
}
```

### 5. Invariantes (validator)

- Ghost path filter: `candidates.retain(|f| graph.get_node(&format!("file:{}", f)).is_some())`
- Expansion limit: `max_neighbors` parametro obrigatorio, NUNCA expansao livre
- Wiki layer 1: FileRetriever NUNCA substitui wiki lookup como primeiro gate
- Score floor: nenhum score abaixo de 0.0

### 6. Rollout

```
Phase 1 MVP (P@5 > 0.35):
  - FileBm25::search como primary ranker
  - Community flatten como secondary
  - Simple reranker (bm25_content, bm25_path, community_score, cochange)
  - Ghost path filter
  - Top 5 files → assembler

Phase 2 Hybrid (P@5 > 0.50):
  - Graph expansion (Calls+Imports, max=15)
  - Test proximity
  - Usefulness EMA
  - Penalties (redundancy, already_seen)

Phase 3 Regimes (reduce variance):
  - Python: lexical forte + filename boost
  - C: header/source pairing
  - Monorepo: package boundary
```

---

## Plano TDD

### Phase 1: FileBm25 como primary + reranker simples

```
RED:
  #[test]
  fn file_retriever_returns_files_not_communities() {
      // Build graph from test fixtures
      // Query "dict implementation"
      // Assert: results contain file paths, not community IDs
  }

  #[test]
  fn file_retriever_top1_matches_expected_for_known_query() {
      // Build graph with known structure (auth module)
      // Query "verify_token"
      // Assert: top-1 result contains "auth" in path
  }

  #[test]
  fn file_retriever_ghost_path_filter() {
      // Candidates include path not in graph
      // Assert: ghost path filtered out
  }

GREEN: FileRetriever struct in file_retriever.rs
       - calls FileBm25::search for candidates
       - flattens top communities
       - filters ghost paths
       - sorts by score
       - returns Vec<RankedFile>

REFACTOR: Extract reranker into separate function.
VERIFY: cargo test -p theo-engine-retrieval -- file_retriever
```

### Phase 1: graph_neighbors_for_file

```
RED:
  #[test]
  fn file_neighbors_returns_files_via_calls() {
      // file_a contains symbol_x, symbol_x calls symbol_y in file_b
      // Assert: file_neighbors("file:a", [Calls], 10) contains "file:b"
  }

  #[test]
  fn file_neighbors_respects_max_limit() {
      // Dense graph with 50 neighbors
      // Assert: result.len() <= max_neighbors
  }

GREEN: file_neighbors() in model.rs using contains_children + adjacency
VERIFY: cargo test -p theo-engine-graph -- file_neighbors
```

---

## Action Items

- [ ] **graphctx-expert** — Implementar file_neighbors() em model.rs — Phase 1
- [ ] **retrieval-engineer** — Implementar FileRetriever em file_retriever.rs — Phase 1
- [ ] **validator** — Ghost path filter + expansion limit invariantes — Phase 1
- [ ] **test-runner** — TDD plans para Phase 1 (RED first) — Phase 1
- [ ] **chief-architect** — Integrar FileRetriever com ContextAssembler — Phase 1
- [ ] **ontology-manager** — RankedFile + Signal types em retrieval — Phase 1
- [ ] **research-agent** — ADR: "File-level retrieval as primary, community as expansion context" — Phase 1

---

## Veredito Final

**REVISED**: Diagnostico unanimemente aprovado — community como unidade de retrieval e o bug estrutural (#1 prioridade). Abordagem simplificada: reusar FileBm25 existente (nao criar FileIndex novo), 2 tipos novos em retrieval (nao 5 em domain), expansion via Calls+Imports only, ghost path filter obrigatorio. Wiki lookup permanece layer 1. Principio: **"File e unidade de decisao; community e contexto de expansao."**
