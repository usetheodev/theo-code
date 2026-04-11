# GraphCTX + Code Wiki: A Solucao Completa

> Como o Theo Code transforma codigo-fonte, execucao e historico em uma base versionada, auditavel e compactavel que entrega ao agente o working set certo no momento certo.

---

## O Que E

GraphCTX e um sistema de code intelligence que constroi um grafo semantico de qualquer repositorio e o transforma em contexto otimizado para agentes de IA. Code Wiki e a camada de conhecimento persistente — paginas markdown geradas deterministicamente a partir do grafo, navegaveis por humanos e consultaveis por agentes.

Juntos, formam um **context runtime adaptativo**: nao apenas indexam codigo — gerenciam contexto ao longo do tempo com aprendizado, memoria operacional, checkpoints e recuperacao sob orcamento.

**Numeros**: 7 crates Rust, 142 arquivos-fonte, 1023+ testes, 16 linguagens suportadas, processamento de repos de 100 a 21.000+ arquivos.

---

## Arquitetura

```
Source Code (16 linguagens)
    |
    v
[Tree-Sitter Parsing] ──────────── theo-engine-parser
    |
    v  FileData (simbolos, imports, referencias)
[Graph Construction] ────────────── theo-engine-graph
    |
    v  CodeGraph (nodes + edges + adjacency)
[Clustering] ────────────────────── Leiden / FileLeiden
    |
    v  Vec<Community>
[Multi-Signal Scoring] ──────────── theo-engine-retrieval
    |
    v  ScoredCommunity[]
[Context Assembly] ──────────────── theo-application
    |                                   |
    v                                   v
Agent (working set                Code Wiki
 + memoria + budget)             (119+ paginas markdown)
```

### Bounded Contexts

```
theo-domain         → (nada — tipos puros)
theo-engine-*       → theo-domain
theo-governance     → theo-domain
theo-infra-*        → theo-domain
theo-agent-runtime  → theo-domain, theo-governance
theo-application    → todos acima
apps/*              → theo-application
```

---

## Fase 1: Parsing (theo-engine-parser)

Tree-Sitter parseia cada arquivo-fonte e extrai simbolos, imports e referencias.

### 16 Linguagens Suportadas

| Tier | Linguagens | Extracao |
|------|-----------|----------|
| **Tier 1 (Full)** | Rust, Python, TypeScript, JavaScript, Go, Java, Kotlin, Ruby, PHP, C#, Scala | Simbolos + referencias + call graph |
| **Tier 2 (Basic)** | Swift, C, C++ | Nodes a nivel de arquivo |

### O Que E Extraido

```rust
pub struct FileData {
    path: String,               // caminho relativo
    language: String,
    symbols: Vec<SymbolData>,   // funcoes, structs, traits, enums
    imports: Vec<ImportData>,   // use/import statements
    references: Vec<ReferenceData>, // chamadas, extends, implements
    data_models: Vec<DataModelData>,
}
```

Cada simbolo carrega: nome qualificado, kind (9 tipos), signature, docstring, linhas (start/end), e flag `is_test`.

**Performance**: ~200ms para 500 arquivos (paralelo via `rayon`, parser cache thread-local).

---

## Fase 2: Grafo (theo-engine-graph)

O bridge converte `Vec<FileData>` em `CodeGraph` — a estrutura central.

### 5 Tipos de Node

| Tipo | Descricao |
|------|-----------|
| `File` | Um arquivo fonte |
| `Symbol` | Funcao, struct, trait, enum, etc. |
| `Import` | Statement use/import |
| `Type` | Data model (struct com campos) |
| `Test` | Funcao de teste |

### 8 Tipos de Edge

| Edge | Peso | Significado |
|------|------|-------------|
| `Contains` | 1.0 | File → Symbol (hierarquia) |
| `Calls` | 1.0 | Chamada direta |
| `Imports` | 1.0 | File → modulo importado |
| `Inherits` | 1.0 | Extends/implements |
| `TypeDepends` | 0.8 | Type annotation |
| `Tests` | 0.7 | Teste cobre sujeito |
| `CoChanges` | decay(age) | Co-mudanca no git (temporal decay exponencial, half-life ~70 dias) |
| `References` | 1.0 | Mencao/leitura generica |

### Symbol-Level Hashing

Cada simbolo tem um hash blake3 baseado em `name + signature + doc`. Permite invalidacao granular — se o hash nao mudou, a wiki nao precisa regenerar.

```rust
pub fn symbol_content_hash(node: &Node) -> String // blake3, 16 chars hex
pub fn community_content_hash(&self, node_ids: &[String]) -> Option<String>
```

### Impact Set via Co-Change

```rust
pub fn compute_impact_set(graph, changed_files, top_k, min_weight) -> ImpactSet
```

Dado um conjunto de arquivos editados, retorna os top-K arquivos mais correlatos por co-change. Leitura pura, sem side-effects.

---

## Fase 3: Clustering

O grafo plano e transformado em **communities** — clusters de arquivos/simbolos semanticamente relacionados. Cada community vira uma pagina da wiki e uma unidade de contexto.

### Algoritmos

| Algoritmo | Uso | Descricao |
|-----------|-----|-----------|
| **Louvain** | Opcional | Phase 1 local moves, O(E) otimizado |
| **Leiden** | Opcional | Refinement phase, garante conectividade |
| **FileLeiden** | **Producao** | File-level clustering, 10-30 communities |

**Producao**: FileLeiden(resolution=0.5) → merge singletons (min_size=3) → LPA-seeded subdivide mega-communities (>30 membros).

**Performance**: ~200ms para 5000 nodes (otimizacao O(N²)→O(E) via adjacency lists pre-computadas).

---

## Fase 4: Multi-Signal Scoring (theo-engine-retrieval)

### BM25 File-Level

```
score(q, D) = Sum_t IDF(t) * (f(t,D) * (k1+1)) / (f(t,D) + k1*(1-b + b*|D|/avgdl))
```

- k1=1.2, b=0.75
- Field boosts: filename 5x, signature 2x, docstring 1.5x
- Code-aware tokenization: split camelCase, snake_case, SCREAMING_CASE

### Scoring Weights (configuraveis)

```rust
pub struct ScoringWeights {
    pub bm25: f64,        // default 0.55
    pub file_boost: f64,  // default 0.30
    pub centrality: f64,  // default 0.05
    pub recency: f64,     // default 0.10
}
```

Normalizados para somar 1.0. Ajustaveis sem recompilacao.

---

## Fase 5: Context Assembly (theo-application)

### ContextAssembler

O compositor que monta o pacote certo de contexto para cada step do agente.

**4 Hard Rules (NUNCA violadas)**:
1. Respeitar token budget
2. Sempre incluir task objective
3. Sempre incluir current plan step
4. Sempre incluir evidencias recentes (ate 8)

**Adaptive Budget**: `max(4000, min(32000, 500 * sqrt(file_count)))`

| Repo Size | Budget |
|-----------|--------|
| 50 files | 4,000 tokens (min) |
| 500 files | 11,180 tokens |
| 2,000 files | 22,360 tokens |
| 5,000+ files | 32,000 tokens (max) |

**Budget Allocation**: 15% task overhead + 25% execution context + 60% structural.

### Scoring Penalties (Precision)

Blocos repetidos sao penalizados para melhorar precision:

```
penalty_multiplier = max(0.5, 1.0 - 0.1 * assembly_count)
```

**Invariante**: Floor = 0.5. Score nunca e zerado — protege recall.

### Stability Bonus (Resume)

Blocos com positive signal (tool use/citation) do turno anterior ganham boost:

```
stability_bonus = 0.15 * 0.7^turns_without_signal
```

**Invariante**: Requer positive signal. Presenca no contexto anterior NAO e sinal.

### Memory Injection (Reuse)

`assemble_with_memory()` injeta EpisodeSummary antes do contexto estrutural:

- Learned constraints
- Failed attempts
- Cap: 10% do budget total

### Feedback Loop

Assembler aprende quais communities sao uteis via EMA:

```rust
record_feedback(community_id, score) // alpha=0.3
// Score persiste entre runs em .theo/assembly_feedback.json
```

---

## Fase 6: Code Wiki

### 5 Authority Tiers

| Tier | Peso | Fonte |
|------|------|-------|
| **Deterministic** | 1.0 | Fatos do CodeGraph |
| **Enriched** | 0.95 | LLM-enhanced |
| **PromotedCache** | 0.75 | Queries validadas |
| **RawCache** | 0.50 | Write-back |
| **EpisodicCache** | 0.40 | Agent execution summaries (TTL-gated, lookup-only) |

### BM25 Lookup com 3 Gates

```
Gate 1: BM25 Floor (< 12.0 → descarta)
Gate 2: Confidence composite (bm25 * tier_weight + title_bonus - stale_penalty)
Gate 3: Per-category threshold (ApiLookup=5.0, Architecture=9.0, Unknown=12.0)
```

### Promotion WAL + Archival

- Write-ahead ledger em `.theo/wiki/runtime/promotions.jsonl`
- Archival: rotate insights para `.theo/wiki/runtime/archive/` apos 24h ou 50k linhas
- Health check: alerta se perto dos limites (10MB raw, 500 summaries, 30-day TTL)

### Operational Limits

```rust
pub struct OperationalLimits {
    pub max_raw_event_bytes: usize,   // 10MB
    pub max_active_summaries: usize,  // 500
    pub archival_ttl_days: u32,       // 30
}
```

---

## Context Manager: Memoria Operacional

### EpisodeSummary

Compacta uma janela de eventos em resumo estruturado reutilizavel:

```rust
pub struct EpisodeSummary {
    machine_summary: MachineEpisodeSummary, // objective, actions, outcome, steps, failures, constraints
    human_summary: Option<String>,
    evidence_event_ids: Vec<String>,
    referenced_community_ids: Vec<String>,
    lifecycle: MemoryLifecycle,  // Active → Cooling → Archived
    ttl_policy: TtlPolicy,      // inferred from constraint scope
    schema_version: u32,
}
```

**Gerado deterministicamente** (sem LLM) via `from_events()`:
- Key actions de ToolCallCompleted
- Successful steps / failed attempts
- Learned constraints (explicitas + failure-derived com threshold ≥3)
- Unresolved hypotheses
- TTL promotion: workspace-local → Permanent, task-local → 24h

### MemoryLifecycle

| Tier | Elegivel para Assembler | TTL |
|------|------------------------|-----|
| **Active** | Sempre | Duracao da run |
| **Cooling** | Se usefulness > 0.3 | 24h |
| **Archived** | Nunca (lookup-only) | 30 dias ou Permanent |

Transicoes: Active → Cooling (episode boundary) → Archived (TTL ou enforce_limits).

### Hypothesis Engine

```rust
pub struct Hypothesis {
    confidence: f64,          // 0.5 (explicit) ou 0.3 (inferred)
    status: HypothesisStatus, // Active / Stale / Superseded
    source: HypothesisSource, // Explicit / Inferred
}
```

- **Active**: entra no assembler
- **Stale**: ignorado (degradacao automatica apos N iteracoes sem uso)
- **Superseded**: nunca entra (marcado por evento explicito, NUNCA deletado)
- **Fallback automatico**: padroes repetidos (3+ vezes) geram hipotese inferida

### Cognitive Events

4 variantes com invariantes causais:

| Evento | Payload Obrigatorio |
|--------|-------------------|
| `HypothesisFormed` | hypothesis + rationale |
| `HypothesisInvalidated` | prior_event_id + reason |
| `DecisionMade` | choice + evidence_refs |
| `ConstraintLearned` | constraint + scope (run/task/workspace) |

Validacao contextual: `validate_cognitive_event_in_context()` verifica que `prior_event_id` referencia evento existente.

### WorkingSet

Estado ativo do agente, salvo no RunSnapshot:

```rust
pub struct WorkingSet {
    hot_files: Vec<String>,
    recent_event_ids: Vec<String>,
    active_hypothesis: Option<String>,
    current_plan_step: Option<String>,
    constraints: Vec<String>,
    agent_id: Option<String>,  // multi-agent isolation
}
```

- `touch_file()`: dedup, mais recente no final
- `merge_from()`: combina parent + child (constraints dedup, child hypothesis preenche parent vazio)
- Incluido no checksum do RunSnapshot

### Context Metrics

```rust
pub struct ContextMetrics {
    context_sizes,      // token count per iteration
    artifact_fetches,   // path → iterations (refetch detection)
    actions,            // action → iterations (repetition detection)
    hypothesis_changes, // formation/invalidation frequency
    assembled_chunks,   // community_id → file paths
    tool_references,    // files actually used by agent
}
```

**Usefulness proxy**: `files_referenced / files_assembled` por community.
**Citation extractor**: `extract_citations(tool_args, block_map) → Vec<block_id>`.
**Report**: persiste em `.theo/metrics/{run_id}.json`.

---

## Incremental Content Hash

```
Cold: le todos os arquivos, blake3 hash
Warm: mtime+size pre-filter, cache em .theo/hash_cache.json
```

| Cenario | Tempo |
|---------|-------|
| theo-code (2K files) cold | 112ms |
| FFmpeg (4.6K files) cold | 507ms |
| Linux GPU (7.4K files) cold | 3.8s |
| **Qualquer repo warm** | **<50ms** |

---

## RunSnapshot + Checkpoint

```rust
pub struct RunSnapshot {
    run: AgentRun,
    task: Task,
    tool_calls, tool_results, events,
    budget_usage, messages, dlq,
    snapshot_at: u64,
    checksum: String,
    schema_version: u32,        // forward-compat
    working_set: Option<WorkingSet>, // context state
}
```

- Checksum inclui todos os campos (incluindo working_set e schema_version)
- Persistido em `.theo/{run_id}.json` com validacao na carga
- Legacy snapshots carregam com defaults (backward-compatible via `#[serde(default)]`)

---

## Failure Learning

Erros recorrentes (≥3 ocorrencias) geram constraints automaticas:

```rust
extract_failure_constraints(events, threshold=3) → Vec<String>
// "Avoid: compile error (seen 3 times)"
```

Normaliza erros (strip line numbers, lowercase) para detectar padroes.
Constraints entram no `learned_constraints` do EpisodeSummary.

---

## Benchmark: 14 Repos Reais

### Budget Analysis (medido)

| Repo | Lang | Files | Lines | Adaptive Budget | Coverage |
|------|------|-------|-------|-----------------|----------|
| ripgrep | Rust | 100 | 52K | 5,000 | 33% |
| gin | Go | 99 | 24K | 4,500 | 30% |
| fastapi | Python | 1,125 | 108K | 16,500 | 9.8% |
| serde | Rust | 208 | 43K | 7,000 | 22% |
| eslint | TS/JS | 1,431 | 525K | 18,500 | 8.6% |
| scikit-learn | Python | 1,010 | 438K | 15,500 | 10% |
| redis | C | 792 | 354K | 14,000 | 11.7% |
| transformers | Python | 4,231 | 1.5M | 32,000 | 5% |
| next.js | TS/JS | 21,033 | 2M | 32,000 | 1% |
| tokio | Rust | 774 | 172K | 13,500 | 11.6% |
| langchain | Python | 2,455 | 340K | 24,500 | 6.6% |
| turborepo | TS/JS | 1,555 | 256K | 19,500 | 8.4% |

### Pipeline Real (7 repos completados)

| Repo | Time | MRR | Recall@10 | Hit@5 |
|------|------|-----|-----------|-------|
| redis | 8.7s | **1.000** | **1.000** | **1.000** |
| eslint | 31s | 0.667 | 0.667 | 1.000 |
| fastapi | 27s | 0.667 | 0.500 | 1.000 |
| gin | 1.9s | 0.333 | 0.333 | 1.000 |
| serde | 4.4s | 0.333 | 0.167 | 0.333 |
| ripgrep | 3.8s | 0.000 | 0.000 | 0.000 |
| scikit-learn | 122s | 0.000 | 0.000 | 0.000 |

---

## Invariantes do Sistema

1. **TDD obrigatorio**: RED → GREEN → REFACTOR. Sem excecao.
2. **schema_version em tudo que persiste**: RunSnapshot, EpisodeSummary, cognitive events.
3. **Backward-compatible**: Campos novos com `#[serde(default)]`. Dados antigos DEVEM carregar.
4. **Penalty floor = 0.5**: Score nunca e zerado. Protege recall.
5. **Hypothesis prune somente via evento**: Nunca auto-delete por idade.
6. **Failure threshold ≥ 3**: Nao gerar constraint de erro isolado.
7. **Memory cap = 10%**: Episode content limitado a 10% do budget total.
8. **Canonical = append-only**: Supersede, never overwrite.
9. **Assembler 4 hard rules NUNCA violadas**: budget, objective, step, evidence.
10. **MRR gate ≥ 0.84**: Nenhum fix pode fazer regressao de ranking.

---

## Arquivos-Chave

| Componente | Caminho |
|-----------|---------|
| Domain types | `crates/theo-domain/src/` |
| Episode + Hypothesis | `crates/theo-domain/src/episode.rs` |
| WorkingSet | `crates/theo-domain/src/working_set.rs` |
| Events + Cognitive | `crates/theo-domain/src/event.rs` |
| Graph Context types | `crates/theo-domain/src/graph_context.rs` |
| Graph model | `crates/theo-engine-graph/src/model.rs` |
| Clustering | `crates/theo-engine-graph/src/cluster.rs` |
| Co-changes + ImpactSet | `crates/theo-engine-graph/src/cochange.rs` |
| Symbol hashing | `crates/theo-engine-graph/src/model.rs` (symbol_content_hash) |
| BM25 + Scoring | `crates/theo-engine-retrieval/src/search.rs` |
| Assembly | `crates/theo-engine-retrieval/src/assembly.rs` |
| Wiki model + tiers | `crates/theo-engine-retrieval/src/wiki/model.rs` |
| Wiki lookup + gates | `crates/theo-engine-retrieval/src/wiki/lookup.rs` |
| Wiki runtime + WAL | `crates/theo-engine-retrieval/src/wiki/runtime.rs` |
| Context Assembler | `crates/theo-application/src/use_cases/context_assembler.rs` |
| Graph Context Service | `crates/theo-application/src/use_cases/graph_context_service.rs` |
| Pipeline | `crates/theo-application/src/use_cases/pipeline.rs` |
| Impact analysis | `crates/theo-application/src/use_cases/impact.rs` |
| Run Engine | `crates/theo-agent-runtime/src/run_engine.rs` |
| Snapshot + Schema | `crates/theo-agent-runtime/src/snapshot.rs` |
| Context Metrics | `crates/theo-agent-runtime/src/context_metrics.rs` |
| Eval golden cases | `crates/theo-engine-retrieval/tests/eval_golden.rs` |
| Real repo benchmark | `crates/theo-application/tests/bench_real_repos.rs` |
