# GraphCTX → Code Wiki: Como Funciona

> Como o Theo Code transforma codigo-fonte em uma base de conhecimento navegavel, sem depender de LLM para o conteudo core.

---

## Visao Geral

O pipeline GraphCTX → Code Wiki converte um repositorio inteiro em uma wiki Obsidian-compatible (`.theo/wiki/`) em dois estagios:

1. **GraphCTX** — constroi um grafo semantico do codigo (nodes = arquivos/simbolos, edges = chamadas/imports/dependencias)
2. **Code Wiki** — transforma clusters do grafo em paginas markdown com API publica, call flow, cobertura de testes e proveniencia

O resultado: **119 paginas** geradas deterministicamente em ~50ms, sem custo de LLM, com cada claim rastreavel ate a linha de codigo que a originou.

```
Source Code (16 linguagens)
    |
    v
[Tree-Sitter Parsing] ──── theo-engine-parser
    |
    v  Vec<FileData>
[Graph Construction] ────── theo-engine-graph
    |
    v  CodeGraph (5103 nodes, 9566 edges)
[Clustering] ────────────── Leiden + LPA
    |
    v  Vec<Community> (84 clusters)
[Wiki Generation] ───────── theo-engine-retrieval/wiki
    |
    v
.theo/wiki/ (119 paginas markdown)
```

---

## Fase 1: Parsing (theo-engine-parser)

Tree-Sitter parseia cada arquivo e extrai simbolos, imports e referencias.

### Linguagens suportadas

| Tier | Linguagens | Nivel de Extracao |
|------|-----------|-------------------|
| **Tier 1 (Full)** | Rust, Python, TypeScript, JavaScript, Go, Java, Kotlin, Ruby, PHP, C#, Scala | Simbolos + referencias + call graph |
| **Tier 2 (Basic)** | Swift, C, C++ | Nodes a nivel de arquivo |

### O que e extraido por arquivo

```rust
pub struct FileData {
    path: String,               // caminho relativo
    language: String,
    symbols: Vec<SymbolData>,   // funcoes, structs, traits, enums...
    imports: Vec<ImportData>,   // use/import statements
    references: Vec<ReferenceData>, // chamadas, extends, implements
    data_models: Vec<DataModelData>, // structs com campos
}
```

Cada `SymbolData` carrega: nome qualificado, kind, signature, docstring, linhas (start/end), e flag `is_test`.

**Performance**: ~200ms para 264 arquivos (paralelo via `rayon::par_iter`, parser cache thread-local).

### Protecao contra ruido

3 camadas filtram arquivos irrelevantes:
1. `.gitignore` (via crate `ignore`)
2. `EXCLUDED_DIRS` — hardcoded em `theo-domain` (target/, node_modules/, __pycache__/, .venv/...)
3. `.theoignore` — exclusoes customizadas do projeto

---

## Fase 2: Grafo (theo-engine-graph)

O bridge converte `Vec<FileData>` em um `CodeGraph` — a estrutura central do GraphCTX.

### Tipos de Node

| Tipo | Representacao | Exemplo de ID |
|------|--------------|---------------|
| `File` | Um arquivo fonte | `file:src/auth.rs` |
| `Symbol` | Funcao, struct, trait, enum... | `sym:src/auth.rs:verify_token` |
| `Import` | Statement use/import | `imp:src/auth.rs:jsonwebtoken` |
| `Type` | Data model (struct com campos) | `type:src/auth.rs:Claims` |
| `Test` | Funcao de teste | `test:src/auth.rs:test_verify` |

### Tipos de Edge

| Edge | Peso | Significado |
|------|------|-------------|
| `Contains` | 1.0 | File → Symbol (hierarquia) |
| `Calls` | 1.0 | Chamada direta de funcao |
| `Imports` | 1.0 | File → modulo importado |
| `Inherits` | 1.0 | Extends/implements |
| `TypeDepends` | 0.8 | Usa uma type annotation |
| `Tests` | 0.7 | Teste cobre sujeito |
| `CoChanges` | decay(age) | Arquivos mudados no mesmo commit |
| `References` | 1.0 | Mencao/leitura generica |

### Git Co-Changes

Analisa os ultimos 500 commits e cria edges `CoChanges` entre pares de arquivos que mudam juntos. Peso via temporal decay exponencial:

```
weight = e^(-lambda * dias_desde_commit)
lambda = 0.01 → half-life ≈ 70 dias
```

Commits recentes pesam mais. Isso captura dependencias implicitas que a analise estatica nao ve.

**Performance**: ~100ms para construcao do grafo + ~150ms para co-changes.

---

## Fase 3: Clustering

O grafo plano e transformado em **communities** — clusters de arquivos/simbolos semanticamente relacionados. Cada community vira uma pagina da wiki.

### Algoritmo de Producao: FileLeiden Hierarquico

```
1. FileLeiden(resolution=0.5) → 10-30 communities base
2. merge_small_communities(min_size=3) → absorve singletons
3. Para cada community > 30 membros:
   → LPA seeded por diretorio → subdivide por crate
4. Nomeia: "theo-agent-runtime (30)"
```

**Otimizacao critica**: Louvain foi de O(N²) para O(E) via adjacency lists pre-computadas + cache de graus + community totals incrementais. Resultado: 15s → 200ms (75x).

### Resultado

Para o proprio Theo Code: **84 communities** agrupando 5103 nodes.

---

## Fase 4: Wiki Generation (theo-engine-retrieval/wiki)

Cada community vira um `WikiDoc` — a representacao intermediaria canonica.

### O que e gerado por pagina

| Secao | Fonte | Descricao |
|-------|-------|-----------|
| **Summary** | `Cargo.toml` description | Uma linha do autor |
| **Overview** | `//!` doc comments em lib.rs | Documentacao do modulo |
| **Entry Points** | Simbolos sem callers internos | As "portas" do modulo |
| **Public API** | Top 15 simbolos por kind | Traits, Structs, Enums, Functions, Methods |
| **Files** | Nodes File da community | Tabela arquivo → count de simbolos |
| **Dependencies** | Edges cross-community | Links para outros modulos |
| **Call Flow** | BFS 2 hops no grafo | Caminhos A → B → C |
| **Test Coverage** | Edges `Tests` | % funcoes testadas + lista de untested |
| **Provenance** | SourceRef por claim | `file:linha_start-linha_end` |

### Modelo de dados

```rust
pub struct WikiDoc {
    slug: String,              // "theo-engine-parser-5"
    title: String,             // "theo-engine-parser"
    community_id: String,
    files: Vec<FileEntry>,
    entry_points: Vec<ApiEntry>,
    public_api: Vec<ApiEntry>,
    dependencies: Vec<DepEntry>,
    call_flow: Vec<FlowStep>,
    test_coverage: TestCoverage,
    source_refs: Vec<SourceRef>, // proveniencia de cada claim
    summary: String,
    tags: Vec<String>,
    enriched: bool,            // LLM ja enriqueceu?
}
```

### Authority Tiers

Nem todo conteudo tem o mesmo peso. O sistema classifica cada pagina:

| Tier | Fonte | Peso | Descricao |
|------|-------|------|-----------|
| **Deterministic** | CodeGraph | 1.0 | Fatos do grafo — zero risco de alucinacao |
| **Enriched** | LLM enhancement | 0.95 | Resumos gerados por LLM |
| **PromotedCache** | Queries validadas | 0.75 | Q&A promovido por humano/agente |
| **RawCache** | Write-back | 0.5 | Respostas nao-validadas |

---

## Fase 5: Rendering e Persistencia

`WikiDoc` → Markdown Obsidian-compatible com frontmatter YAML.

### Formato de pagina

```markdown
---
authority_tier: deterministic
page_kind: module
generated_by: generator
graph_hash: 1695828210123601777
summary: "AST parser, symbol extraction"
tags: [parser, tree-sitter, rust]
---

# theo-engine-parser

**Summary**: AST parser, symbol extraction. **Tags**: #parser #tree-sitter

> 14 files | rs | 221 symbols

## Entry Points

```rust
pub fn parse_source(path: &Path) -> Result<Ast>
```
> Source: `crates/theo-engine-parser/src/tree_sitter.rs:42-88`

## Public API
### Traits
...
### Functions
...

## Dependencies
- → [[theo-engine-graph]] (Imports)
- → [[theo-domain]] (TypeDepends)

## Call Flow
`parse_source` → `detect_language` → `extract_symbols`

## Test Coverage
42/50 functions tested (84%)
Untested: `parse_legacy_syntax`, `handle_utf8_edge_case`

---
*Generated by GRAPHCTX wiki-bootstrap-v1 | Sources: 14 files, 221 symbols*
```

### Estrutura em disco

```
.theo/wiki/
├── modules/          # 119 paginas (uma por community)
│   ├── theo-engine-graph-10.md
│   ├── theo-engine-parser-5.md
│   └── ...
├── index.md          # TOC hierarquico por bounded context
├── overview.md       # Visao geral do projeto
├── architecture.md   # Diagrama + bounded contexts
├── getting-started.md
├── cache/            # Write-back de queries
│   └── stale/        # Paginas invalidadas (GC em 7 dias)
├── runtime/
│   └── insights.jsonl  # Resultados de testes, builds, erros
├── wiki.manifest.json  # Hash para cache invalidation
└── wiki.schema.toml    # Configuracao do usuario
```

### Cache Invalidation

O `graph_hash` e o hash de `(caminhos_de_arquivo, mtime)` ordenados. Se algum arquivo mudou → hash muda → wiki e regenerada. Cada pagina carrega o `graph_hash` no frontmatter — se difere do manifest, a pagina e marcada `is_stale`.

---

## Fase 6: Query & Retrieval

Quando um agente ou usuario consulta a wiki, o sistema usa BM25 com 3 gates de decisao.

### Pipeline de busca

```
Query: "como funciona autenticacao?"
    |
    v
[Tokenizacao] → split camelCase/snake_case + stemming minimo
    |
    v
[BM25 Search] → score por community (postings list + IDF)
    |
    v
[3-Gate Decision]
    |
    ├─ Gate 1: BM25 Floor (< 12.0 → descarta)
    ├─ Gate 2: Confidence composite
    │    = 0.5*top1 + 0.3*gap(top1-top2) + 0.1*title + 0.1*tier - stale_penalty
    └─ Gate 3: Threshold por categoria
         ApiLookup: 5.0 | Onboarding: 7.0 | Architecture: 9.0 | Unknown: 12.0
    |
    v
[Retorno direto] ou [Fallback para RRF pipeline completo]
```

Se a wiki tem uma resposta de alta confianca, o RRF pipeline inteiro e short-circuited.

### Scoring com Authority Tiers

```
score_final = (bm25_raw * tier_weight + title_bonus - stale_penalty).max(0)
```

Paginas Deterministic pesam 1.0x, RawCache pesa 0.5x. Paginas stale perdem 0.3 pontos.

---

## Deep Wiki: As 4 Camadas

O Code Wiki implementa o modelo de 4 camadas inspirado em Karpathy:

| Camada | Fonte | Status | Exemplo |
|--------|-------|--------|---------|
| **1. Deterministic** | Cargo.toml, doc comments, README | DONE | Summary, Overview |
| **2. Structural** | CodeGraph (edges, clusters) | DONE | API, Call Flow, Coverage |
| **3. Operational** | Test results, build failures | IN PROGRESS | RuntimeInsight JSONL |
| **4. Synthesized** | LLM summaries | PARTIAL | "What This Module Does" |

### Camada 3: Runtime Insights

Captura automatica de `cargo test`, `cargo build`, e execucoes de agentes:

```rust
pub struct RuntimeInsight {
    source: String,          // "cargo_test", "cargo_build", "agent"
    command: String,
    exit_code: i32,
    duration_ms: u64,
    error_summary: Option<String>,
    affected_files: Vec<String>,   // extraidos de mensagens de erro
    affected_symbols: Vec<String>, // extraidos de test output
}
```

Armazenado como append-only JSONL em `.theo/wiki/runtime/insights.jsonl`. Agregado em: `common_failures`, `successful_recipes`, `flaky_tests`.

---

## Integracao com Agent Runtime

### Trigger automatico

```rust
// Em GraphContextService::initialize()
// Apos construir o grafo:
generate_wiki_if_stale(&graph, &communities, &dir);
```

A wiki e regenerada automaticamente quando o graph_hash muda (algum arquivo foi modificado).

### Tools disponiveis para agentes

| Tool | Parametros | Funcao |
|------|-----------|--------|
| `wiki_query` | query, max_results | Busca BM25 na wiki |
| `wiki_ingest` | command, exit_code, stdout, stderr | Captura runtime insight |
| `wiki_generate` | — | Regenera wiki completa |

### Fluxo tipico do agente

```
Agente recebe tarefa: "corrija o bug de autenticacao"
    |
    v
[wiki_query("autenticacao")] → pagina theo-infra-auth com API + call flow
    |
    v
[codebase_context("authentication flow")] → GraphCTX multi-signal ranking
    |
    v
Agente le os arquivos relevantes, faz a correcao
    |
    v
[cargo test] → [wiki_ingest(resultado)] → RuntimeInsight salvo
```

---

## Performance

| Operacao | Tempo | Complexidade |
|----------|-------|-------------|
| Parsing (264 arquivos, rayon) | ~200ms | O(total_LOC) |
| Graph building | ~100ms | O(simbolos + referencias) |
| Git co-changes (500 commits) | ~150ms | O(commits * files/commit) |
| Clustering (FileLeiden + LPA) | ~200ms | O(E) por passo |
| **Wiki generation** | **~50ms** | O(communities * symbols) |
| **Total cold build** | **~700ms** | — |
| Wiki query (BM25) | ~5ms | O(pages * terms) |
| Cache hit (warm) | ~22ms | O(files) para hash |

---

## Arquivos-Chave

| Componente | Caminho |
|-----------|---------|
| Domain trait | `crates/theo-domain/src/graph_context.rs` |
| Wiki backend trait | `crates/theo-domain/src/wiki_backend.rs` |
| Parser | `crates/theo-engine-parser/src/tree_sitter.rs` |
| Graph model | `crates/theo-engine-graph/src/model.rs` |
| Clustering | `crates/theo-engine-graph/src/cluster.rs` |
| Co-changes | `crates/theo-engine-graph/src/cochange.rs` |
| Wiki model | `crates/theo-engine-retrieval/src/wiki/model.rs` |
| Wiki generator | `crates/theo-engine-retrieval/src/wiki/generator.rs` |
| Wiki renderer | `crates/theo-engine-retrieval/src/wiki/renderer.rs` |
| Wiki lookup | `crates/theo-engine-retrieval/src/wiki/lookup.rs` |
| Wiki persistence | `crates/theo-engine-retrieval/src/wiki/persistence.rs` |
| Runtime insights | `crates/theo-engine-retrieval/src/wiki/runtime.rs` |
| Pipeline | `crates/theo-application/src/use_cases/pipeline.rs` |
| Service | `crates/theo-application/src/use_cases/graph_context_service.rs` |
| Wiki tools | `crates/theo-tooling/src/wiki_tool/mod.rs` |
