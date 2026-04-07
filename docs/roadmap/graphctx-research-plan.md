# GRAPHCTX Research Plan — From P@5=0.36 to SOTA

> Plano executável de pesquisa aplicada com DoDs mensuráveis.
> Cada fase tem hipótese, implementação, medição, e critério de sucesso.
> Toda execução na vast.ai. Zero na máquina local.

---

## Baseline (onde estamos)

| Métrica | Valor | Medido em |
|---|---|---|
| P@5 | 0.360 | Theo Code (265 files) |
| R@5 | 0.632 | Theo Code (265 files) |
| MRR | 0.869 | Theo Code (265 files) |
| Determinismo | 100% (3/3 idêntico) | Theo Code |
| Repos testados | 1 | Theo Code |

---

## Fase R1 — Tantivy Migration + Code Tokenizer

### Hipótese
Substituir BM25 custom por Tantivy com tokenizer code-aware melhora P@5 em 5-8%.

### Escopo
- Adicionar `tantivy` ao workspace
- Criar `FileTantivyIndex` em `theo-engine-retrieval/src/tantivy_search.rs`
- Code-aware tokenizer: camelCase split, snake_case split, path component split, Rust stop words
- BM25F com field boost: filename 5x, symbol name 3x, signature 1x, doc 1x
- Substituir `FileBm25::search()` pelo Tantivy search no eval

### DoD (Definition of Done)

| # | Critério | Como verificar | Obrigatório? |
|---|---|---|---|
| 1.1 | `tantivy` adicionado como workspace dependency | `grep tantivy Cargo.toml` | Sim |
| 1.2 | `FileTantivyIndex` compila sem feature gate | `cargo check -p theo-engine-retrieval` | Sim |
| 1.3 | Code tokenizer split camelCase: `getUserById` → `[get, user, by, id]` | Unit test | Sim |
| 1.4 | Code tokenizer split snake_case: `get_user_by_id` → `[get, user, by, id]` | Unit test | Sim |
| 1.5 | Code tokenizer split paths: `src/auth/oauth.rs` → `[src, auth, oauth, rs]` | Unit test | Sim |
| 1.6 | Code tokenizer filtra Rust stop words (fn, pub, struct, impl, let) | Unit test | Sim |
| 1.7 | BM25F field boost: filename 5x, symbol 3x, sig 1x, doc 1x | Unit test com scores verificáveis | Sim |
| 1.8 | Eval suite roda com Tantivy backend | `cargo test --test eval_suite -- --ignored` | Sim |
| 1.9 | **P@5 >= 0.40** no Theo Code (265 files) | Eval suite output | Sim |
| 1.10 | **MRR >= 0.85** no Theo Code | Eval suite output | Sim |
| 1.11 | Tempo de indexação < 2s para 265 files | Eval suite timing | Sim |
| 1.12 | `cargo test -p theo-engine-retrieval` todos passam | CI-equivalent | Sim |

### Artefatos
- `crates/theo-engine-retrieval/src/tantivy_search.rs` (novo)
- `crates/theo-engine-retrieval/src/code_tokenizer.rs` (novo)
- `Cargo.toml` (tantivy dependency)

### Estimativa: 2-3 dias

---

## Fase R2 — Hybrid Retrieval com RRF

### Hipótese
Dense embeddings (CodeRankEmbed 137M via fastembed) + BM25 fusionados com Reciprocal Rank Fusion melhoram P@5 em 15-20%.

### Pré-requisito
Fase R1 concluída (Tantivy como BM25 backend).

### Escopo
- Criar `DenseRetriever` em `theo-engine-retrieval/src/dense_search.rs`
- Usar `fastembed` crate (já é dependência) com modelo CodeRankEmbed ou AllMiniLM
- Embeddings pré-computados por file (cached em `.theo/embeddings.bin`)
- RRF fusion: `score(file) = 1/(k+rank_bm25) + 1/(k+rank_dense)` com k=60
- Criar `HybridRetriever` que combina Tantivy + Dense via RRF

### DoD

| # | Critério | Como verificar | Obrigatório? |
|---|---|---|---|
| 2.1 | `DenseRetriever` compila e indexa 265 files | `cargo check` | Sim |
| 2.2 | Embeddings cacheados em `.theo/embeddings.bin` | File exists após build | Sim |
| 2.3 | Tempo de embedding < 30s para 265 files (CPU) | Timing no build | Sim |
| 2.4 | RRF fusion implementado (~30 linhas) | Unit test com 2 rankings → merged | Sim |
| 2.5 | `HybridRetriever` combina BM25+Dense+RRF | Integration test | Sim |
| 2.6 | **P@5 >= 0.50** no Theo Code | Eval suite output | **Sim — gate** |
| 2.7 | **MRR >= 0.87** no Theo Code | Eval suite output | Sim |
| 2.8 | A/B: P@5(hybrid) > P@5(BM25-only) em >= 3 queries | Eval per-query comparison | Sim |
| 2.9 | Tempo de query < 200ms (indexação + search + fusion) | Eval timing | Sim |
| 2.10 | `cargo test -p theo-engine-retrieval` todos passam | CI-equivalent | Sim |

### Artefatos
- `crates/theo-engine-retrieval/src/dense_search.rs` (novo)
- `crates/theo-engine-retrieval/src/hybrid.rs` (novo — RRF fusion)
- `.theo/embeddings.bin` (cached embeddings)

### Estimativa: 3-5 dias

---

## Fase R3 — CPU Reranker

### Hipótese
Reranking dos top-20 candidates com MiniLM-L-6 ONNX melhora P@5 em 10-15%.

### Pré-requisito
Fase R2 concluída (Hybrid retrieval).

### Escopo
- Criar `CpuReranker` em `theo-engine-retrieval/src/reranker.rs`
- Usar `fastembed` built-in reranker (BGE-reranker-base ou MiniLM-L-6)
- Rerank top-20 candidates do HybridRetriever
- Cache de model weights em `.theo/models/`

### DoD

| # | Critério | Como verificar | Obrigatório? |
|---|---|---|---|
| 3.1 | `CpuReranker` compila e carrega modelo ONNX | `cargo check` | Sim |
| 3.2 | Model download automático na primeira execução | Smoke test | Sim |
| 3.3 | Rerank de 20 candidates em < 100ms (CPU) | Timing test | Sim |
| 3.4 | **P@5 >= 0.55** no Theo Code | Eval suite output | Sim |
| 3.5 | **P@5 >= 0.60** no Theo Code | Eval suite output | **Stretch goal** |
| 3.6 | **MRR >= 0.90** no Theo Code | Eval suite output | Sim |
| 3.7 | A/B: P@5(reranked) > P@5(hybrid-only) | Eval comparison | Sim |
| 3.8 | Pipeline completa: BM25 → Dense → RRF → Rerank funciona E2E | Integration test | Sim |
| 3.9 | Feature flag: reranker opt-in via `THEO_RERANK=1` | Env var check | Sim |
| 3.10 | `cargo test -p theo-engine-retrieval` todos passam | CI-equivalent | Sim |

### Artefatos
- `crates/theo-engine-retrieval/src/reranker.rs` (novo)
- `.theo/models/` (cached ONNX model)

### Estimativa: 2-3 dias

---

## Fase R4 — Multi-Repo Benchmark Suite

### Hipótese
O sistema generaliza para repos de diferentes linguagens e tamanhos com P@5 >= 0.45 médio.

### Pré-requisito
Fases R1-R3 concluídas.

### Escopo

#### 10 repos de benchmark

| # | Repo | Linguagem | Size | Queries |
|---|---|---|---|---|
| 1 | pytorch/pytorch | Python/C++ | 8000+ | 20 |
| 2 | BurntSushi/ripgrep | Rust | 300 | 15 |
| 3 | tokio-rs/tokio | Rust | 800 | 15 |
| 4 | microsoft/vscode | TypeScript | 5000+ | 20 |
| 5 | django/django | Python | 3500 | 20 |
| 6 | etcd-io/etcd | Go | 1500 | 15 |
| 7 | spring-projects/spring-boot | Java | 4000+ | 20 |
| 8 | google/leveldb | C++ | 200 | 10 |
| 9 | scikit-learn/scikit-learn | Python | 1500 | 15 |
| 10 | grafana/grafana | Go/TypeScript | 6000+ | 20 |

**Total: 170 queries com ground truth.**

#### Ground truth automático (3 fontes)
1. SCIP cross-references (Rust, TS, Go, Java, Python)
2. Git co-change mining (todos)
3. Docstring extraction (Python, Java)
4. LLM validation (verificação)

### DoD

| # | Critério | Como verificar | Obrigatório? |
|---|---|---|---|
| 4.1 | 10 repos clonados na vast.ai | `ls /repos/` | Sim |
| 4.2 | SCIP index gerado para repos Rust (ripgrep, tokio) | `index.scip` exists | Sim |
| 4.3 | Ground truth: >= 15 queries por repo com expected files | JSON ground truth files | Sim |
| 4.4 | Eval runner roda em todos os 10 repos | Script bash | Sim |
| 4.5 | **P@5 médio >= 0.45** across 10 repos | Aggregate metrics | **Sim — gate** |
| 4.6 | **MRR médio >= 0.70** across 10 repos | Aggregate metrics | Sim |
| 4.7 | Nenhum repo com P@5 < 0.20 | Per-repo metrics | Sim |
| 4.8 | Resultados publicados em `docs/benchmarks/` | Markdown report | Sim |
| 4.9 | Comparação com CodeSearchNet BM25 baseline | Table comparison | Sim |
| 4.10 | Report com breakdown por linguagem e tamanho | Categorized metrics | Sim |

### Artefatos
- `docs/benchmarks/multi-repo-results.md` (report)
- `apps/theo-benchmark/eval_multi_repo.rs` (runner)
- `apps/theo-benchmark/ground_truth/` (10 JSON files)

### Estimativa: 5-7 dias

---

## Fase R5 — Language Parity com Sourcegraph

### Hipótese
Suportar as mesmas 12+ linguagens que Sourcegraph com SCIP indexers.

### Pré-requisito
Fase R4 concluída (benchmark multi-repo valida a infraestrutura).

### Escopo

| Linguagem | SCIP Indexer | Tree-Sitter | Status atual | Ação |
|---|---|---|---|---|
| Rust | rust-analyzer scip | ✅ Full | Integrado | Manter |
| TypeScript/JS | scip-typescript | ✅ Full | Tree-Sitter only | Adicionar SCIP |
| Python | scip-python | ✅ Full | Tree-Sitter only | Adicionar SCIP |
| Go | scip-go | ✅ Full | Tree-Sitter only | Adicionar SCIP |
| Java | scip-java | ✅ Full | Tree-Sitter only | Adicionar SCIP |
| Kotlin | scip-java | ❌ Basic | Zero symbols | Adicionar extractor + SCIP |
| Scala | scip-java | ❌ Basic | Zero symbols | Adicionar extractor + SCIP |
| C/C++ | scip-clang | ❌ Basic | Zero symbols | Adicionar extractor + SCIP |
| C# | scip-dotnet | ✅ Full | Tree-Sitter | Adicionar SCIP |
| Ruby | scip-ruby | ✅ Full | Tree-Sitter | Adicionar SCIP |
| PHP | scip-php | ✅ Full | Tree-Sitter | Adicionar SCIP |
| Dart | scip-dart | ❌ None | Não suportado | Adicionar tudo |

### DoD

| # | Critério | Como verificar | Obrigatório? |
|---|---|---|---|
| 5.1 | `scip_indexer.rs` detecta linguagem e invoca indexer correto | Unit test per language | Sim |
| 5.2 | SCIP index gerado para TypeScript (vscode) | `index.scip` on vast.ai | Sim |
| 5.3 | SCIP index gerado para Python (django) | `index.scip` on vast.ai | Sim |
| 5.4 | SCIP index gerado para Go (etcd) | `index.scip` on vast.ai | Sim |
| 5.5 | SCIP index gerado para Java (spring-boot) | `index.scip` on vast.ai | Sim |
| 5.6 | Tree-Sitter extractors para Kotlin/Scala (symbols básicos) | Unit tests | Sim |
| 5.7 | Tree-Sitter extractor para C/C++ (functions/structs/includes) | Unit tests | Sim |
| 5.8 | **12 linguagens suportadas** (9 full + 3 com extractors novos) | Doc atualizado | Sim |
| 5.9 | Benchmark multi-repo roda com SCIP em 5+ repos | Eval output | Sim |
| 5.10 | P@5 melhora em repos com SCIP vs sem SCIP | A/B comparison | Sim |

### Estimativa: 7-10 dias

---

## Fase R6 — Publicação e Documentação

### Escopo
- Technical report com resultados completos
- Comparação formal com CodeSearchNet baseline
- Documentação de arquitetura atualizada
- Benchmark reproducível (scripts + ground truth commitados)

### DoD

| # | Critério | Obrigatório? |
|---|---|---|
| 6.1 | `docs/benchmarks/technical-report.md` com métricas, comparações, limitações | Sim |
| 6.2 | `docs/current/graphctx-architecture.md` atualizado com pipeline R1-R3 | Sim |
| 6.3 | `apps/theo-benchmark/` com scripts reproduzíveis | Sim |
| 6.4 | Ground truth JSON commitado para 10 repos | Sim |
| 6.5 | CHANGELOG atualizado | Sim |

### Estimativa: 2 dias

---

## Timeline

```
Semana 1:  R1 (Tantivy + tokenizer)           → P@5 ~0.42
Semana 2:  R2 (Hybrid BM25+Dense+RRF)         → P@5 ~0.55
Semana 3:  R3 (CPU Reranker)                   → P@5 ~0.65
Semana 4:  R4 (10-repo benchmark)              → P@5 ~0.45 médio
Semana 5:  R5 (Language parity)                → 12 linguagens
Semana 6:  R6 (Report + docs)                  → Publicável
```

## Gates (pontos de decisão)

| Gate | Critério | Se falhar |
|---|---|---|
| **G1** (após R1) | P@5 >= 0.40 | Investigar tokenizer, não prosseguir com R2 |
| **G2** (após R2) | P@5 >= 0.50 | Investigar modelo dense, considerar SPLADE |
| **G3** (após R3) | P@5 >= 0.55 | Investigar reranker model, considerar ColBERT |
| **G4** (após R4) | P@5 médio >= 0.45 em 10 repos | Investigar per-language gaps |

## Riscos

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| fastembed model download lento na vast.ai | Média | Atraso | Cache models em volume persistente |
| Tantivy API breaking changes | Baixa | Médio | Pin version, read docs |
| Dense embeddings não melhoram code search | Média | Alto | Testar 2-3 modelos antes de commitar |
| 10 repos não cabem no disco da vast.ai | Média | Médio | Shallow clone, repos menores primeiro |
| SCIP indexer falha em repos grandes | Média | Médio | Fallback Tree-Sitter (já implementado) |

## Métricas de sucesso final

| Métrica | Baseline | Target | SOTA reference |
|---|---|---|---|
| P@5 (single repo) | 0.360 | **>= 0.60** | CodeCompass: ~0.85 |
| MRR (single repo) | 0.869 | **>= 0.90** | CodeCompass: ~0.95 |
| P@5 (10 repos médio) | N/A | **>= 0.45** | Inédito para local-first |
| Linguagens suportadas | 9 full + 5 basic | **12 full** | Sourcegraph: 12+ |
| Tempo de query | 22ms warm | **< 200ms** | Sourcegraph: ~100ms |
| Repos testados | 1 | **10** | Padrão acadêmico |
