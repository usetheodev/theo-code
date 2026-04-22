# Plano: Wire Context Management Fases 2-4 em Produção

> **Status:** DRAFT — 2026-04-22
> **Origem:** Relatório de validação `evolution/apr22` identificou que 3 módulos (`harm_filter`, `code_compression`, `inline_builder`) existem, compilam, e têm 24 testes unitários verdes, **mas não estão sendo chamados no hot path**. Mesmo padrão "dormant code" do plano de memória (PLAN_MEMORY_SUPERIORITY).
> **Gate global:** MRR ≥ 0.90, hygiene score ≥ 47.513, nenhum teste existente regride.

---

## Diagnóstico

```
Modulo                  | Compila | Testes | Wired? | Call sites em prod
─────────────────────── | ─────── | ────── | ────── | ──────────────────
CompactionPolicy (0.5)  | ✓       | via    | ✓      | run_engine.rs:648
observation_mask (1)    | ✓       | 10     | ✓      | compact_staged_with_policy → Warning level
harm_filter (2)         | ✓       | 9      | ✗      | ZERO — só `pub mod` em lib.rs:10
code_compression (3)    | ✓       | 7      | ✗      | ZERO — só `pub mod` em parser/lib.rs:1
inline_builder (4)      | ✓       | 8      | ✗      | ZERO — só `pub mod` em retrieval/lib.rs:11
```

**Ganho teórico não capturado**: CODEFILTER +21.7% EM, LongCodeZip 5.6× compression, InlineCoder +2.6× EM cross-function.

---

## Pontos de integração identificados

### 2.1 `file_retriever.rs` — Stage atual (linha 103-187)

```rust
pub fn retrieve_files(...) -> FileRetrievalResult {
    // Stage 2: FileBm25::search
    // Stage 3: Community flatten
    // <ghost path filter>
    // Stage 4: Rerank (6 features)
    //   ranked.sort_by(...); ranked.truncate(config.top_k);   ← linha 173
    // Stage 5: Graph expansion (Calls+Imports, depth=1)        ← linha 175
    // return FileRetrievalResult { ... }
}
```

### 2.2 `file_retriever.rs::build_context_blocks` (linha 404-487)

Hoje só concatena `node.signature` de cada child. Ignora o source completo.

### 2.3 Caller único de `retrieve_files`

`crates/theo-application/src/use_cases/graph_context_service.rs:431`. Qualquer mudança de interface precisa propagar lá.

---

## Phase 1: Wire `harm_filter` (CODEFILTER)

> **LOC estimado:** ~50 (impl) + ~40 (integration tests) = **~90**
> **Risco:** Baixo — já existe safety cap 40% e 9 unit tests cobrem todos os sinais.
> **Dependências:** Nenhuma.

### Task 1.1: Inserir harm filter como Stage 4.5

**Arquivo:** `crates/theo-engine-retrieval/src/file_retriever.rs`

**Mudança:** entre line 173 (truncate top_k) e line 175 (Stage 5 expansion), converter `Vec<RankedFile>` em `Vec<(String, f64)>`, chamar `filter_harmful_chunks`, mapear de volta para `Vec<RankedFile>` mantendo a ordem original.

```rust
// Stage 4.5: Harm filter (heurístico, sem LLM call).
// Reduz falsos positivos antes da expansão do grafo.
let candidate_pairs: Vec<(String, f64)> = ranked
    .iter()
    .map(|r| (r.path.clone(), r.score))
    .collect();
let harm_result = crate::harm_filter::filter_harmful_chunks(&candidate_pairs, graph);
let kept: HashSet<String> = harm_result.kept.iter().map(|(p, _)| p.clone()).collect();
ranked.retain(|r| kept.contains(&r.path));
// Opcional: publicar métrica — len(harm_result.removed)
```

### Task 1.2: Métrica visível

**Arquivo:** `crates/theo-engine-retrieval/src/file_retriever.rs`

Estender `FileRetrievalResult` com `pub harm_removals: usize` (default 0). Incrementa após o filter para telemetria — sem impacto em callers existentes (`Default` preserva compat).

### Task 1.3: Testes de integração

**Arquivo novo:** `crates/theo-engine-retrieval/tests/harm_filter_integration.rs`

```rust
#[test]
fn retrieve_files_removes_test_file_when_definer_present() {
    // Arrange: graph com `src/auth.rs` + `tests/auth_test.rs`
    // Act: retrieve_files("authenticate")
    // Assert: `tests/auth_test.rs` filtrado, `src/auth.rs` mantido
}

#[test]
fn retrieve_files_respects_40pct_removal_cap() { ... }

#[test]
fn retrieve_files_harm_removals_metric_exposed() { ... }
```

### Task 1.4: Guard MRR

**Arquivo:** benchmark existente (`crates/theo-engine-retrieval/tests/benchmark_suite.rs` se existir, senão criar).

Rodar em dataset de queries: verificar MRR ≥ 0.90 após ativação do harm filter.

### DoD Phase 1
- [ ] `harm_filter::filter_harmful_chunks` chamado em `retrieve_files` no Stage 4.5
- [ ] `FileRetrievalResult.harm_removals` exposto
- [ ] 3 integration tests novos passando
- [ ] MRR benchmark ≥ 0.90
- [ ] `cargo test --workspace` green
- [ ] Hygiene ≥ 47.513

---

## Phase 2: Wire `code_compression` (LongCodeZip)

> **LOC estimado:** ~60 (impl) + ~40 (tests) = **~100**
> **Risco:** Médio — compressão adiciona latência de parse (Tree-Sitter por arquivo).
> **Dependências:** Nenhuma (Phase 1 pode ser paralela).

### Task 2.1: Estender `build_context_blocks` com compressão

**Arquivo:** `crates/theo-engine-retrieval/src/file_retriever.rs`

Hoje line 413-439 só junta signatures. Substituir por:

```rust
for ranked in &result.primary_files {
    // Identifica símbolos relevantes (query tokens ∩ node names do arquivo).
    let relevant: HashSet<String> = query_tokens
        .iter()
        .filter(|t| graph_has_symbol_in_file(graph, &ranked.path, t))
        .cloned()
        .collect();

    // Lê source do disco.
    let Ok(source) = std::fs::read_to_string(workspace_root.join(&ranked.path)) else {
        continue;
    };

    // Extrai symbols via parser (Tree-Sitter já integrado).
    let symbols = extract_symbols_for_path(&source, &ranked.path);

    // Comprime: relevantes full, outros signature-only.
    let compressed = theo_engine_parser::code_compression::compress_for_context(
        &source, &symbols, &relevant, &ranked.path,
    );

    // Guard budget. Se compressed ainda excede, pula.
    if tokens_used + compressed.compressed_tokens > budget_tokens {
        break;
    }

    blocks.push(ContextBlock {
        block_id: format!("blk-file-{}", ranked.path.replace('/', "-")),
        source_id: ranked.path.clone(),
        content: compressed.text,
        token_count: compressed.compressed_tokens,
        score: ranked.score,
    });
    tokens_used += compressed.compressed_tokens;
}
```

**Obs:** `build_context_blocks` precisa receber `workspace_root: &Path` como arg — mudança de interface propaga para `graph_context_service.rs:441`.

### Task 2.2: Fallback quando source não é lido

Se `fs::read_to_string` falha OU `symbols.is_empty()` (parser sem hit), cair de volta no comportamento antigo (só signatures). `compress_for_context` já faz isso (retorna source inteiro com header quando `symbols.is_empty()`), mas o caller precisa decidir entre chamar compress ou pular.

### Task 2.3: Testes de integração

**Arquivo novo:** `crates/theo-engine-retrieval/tests/compression_integration.rs`

```rust
#[test]
fn build_context_blocks_compresses_irrelevant_symbols() {
    // Arrange: arquivo com 3 funções, query menciona só 1
    // Assert: content tem body completo da relevante, só signature das outras
}

#[test]
fn build_context_blocks_falls_back_when_no_symbols_extracted() { ... }

#[test]
fn build_context_blocks_respects_token_budget_after_compression() { ... }
```

### Task 2.4: Métrica de compressão

Estender `FileRetrievalResult` com `compression_savings_tokens: usize` (= `original_tokens - compressed_tokens` somado). Permite validar o gate do plano original (token_efficiency +40%).

### DoD Phase 2
- [ ] `compress_for_context` chamado em `build_context_blocks` por primary_file
- [ ] Fallback para comportamento antigo quando source não disponível
- [ ] 3 integration tests novos passando
- [ ] Compressão efetiva medida ≥ 3× em pelo menos 1 arquivo de teste
- [ ] MRR benchmark ≥ 0.90
- [ ] `cargo test --workspace` green

---

## Phase 3: Wire `inline_builder` (InlineCoder)

> **LOC estimado:** ~80 (impl) + ~50 (tests) = **~130**
> **Risco:** Alto — resolução cross-file, interação com reverse boost.
> **Dependências:** Phases 1 e 2 completas (ganho marginal sem harm filter + compression).

### Task 3.1: Implementar `FsSourceProvider`

**Arquivo novo:** `crates/theo-engine-retrieval/src/fs_source_provider.rs`

Trait `SourceProvider` já existe em `inline_builder.rs:106`. Implementar adapter de filesystem:

```rust
pub struct FsSourceProvider<'a> { pub root: &'a Path }

impl<'a> SourceProvider for FsSourceProvider<'a> {
    fn get_lines(&self, file: &str, start: usize, end: usize) -> String {
        let full = self.root.join(file);
        let Ok(src) = std::fs::read_to_string(&full) else { return String::new() };
        src.lines()
            .skip(start.saturating_sub(1))
            .take(end.saturating_sub(start) + 1)
            .collect::<Vec<_>>()
            .join("\n")
    }
}
```

### Task 3.2: Adicionar Stage 4.5b em `retrieve_files`

Entre Stage 4.5 (harm filter) e Stage 5 (graph expansion), tentar inline slice:

```rust
// Stage 4.5b: Inline expansion — se query bate exato em name_index.
let inline_policy = crate::inline_builder::InliningPolicy::default();
let source_provider = crate::fs_source_provider::FsSourceProvider { root: workspace_root };
let inline_result = crate::inline_builder::build_inline_slices(
    query, graph, &source_provider, &inline_policy,
);
// inline_result.slices vai em FileRetrievalResult.inline_slices (novo campo)
```

Estender `FileRetrievalResult` com `pub inline_slices: Vec<InlineSlice>` (default empty).

### Task 3.3: Mutual exclusion com reverse boost

**Arquivo:** `crates/theo-engine-retrieval/src/assembly.rs` (função `assemble_files_direct`)

Quando `FileRetrievalResult.inline_slices` não está vazio para o arquivo focal, **não aplicar** reverse dependency boost (evita dupla contagem — plan original line 263).

```rust
let has_inline = result.inline_slices.iter().any(|s| s.focal_file == file_path);
let reverse_boost = if has_inline { 0.0 } else { compute_reverse_boost(...) };
```

### Task 3.4: Integrar inline slices em `build_context_blocks`

Adicionar um bloco por `InlineSlice` antes dos primary_files (score maior, focalizado):

```rust
for slice in &result.inline_slices {
    if tokens_used + slice.token_count > budget_tokens { break; }
    blocks.push(ContextBlock {
        block_id: format!("blk-inline-{}", slice.focal_symbol_id),
        source_id: slice.focal_file.clone(),
        content: slice.content.clone(),
        token_count: slice.token_count,
        score: 1.0, // prioritário — query teve hit exato
    });
    tokens_used += slice.token_count;
}
```

### Task 3.5: Testes de integração

**Arquivo novo:** `crates/theo-engine-retrieval/tests/inline_builder_integration.rs`

```rust
#[test]
fn retrieve_files_produces_inline_slice_for_exact_hit() { ... }

#[test]
fn retrieve_files_no_inline_slice_when_no_name_match() { ... }

#[test]
fn inline_slice_disables_reverse_boost_for_same_file() { ... }

#[test]
fn build_context_blocks_inline_slice_has_highest_score() { ... }
```

### Task 3.6: Guard MRR cross-function

**Arquivo:** benchmark suite

Validar que EM em queries cross-function melhora ≥ 15% vs baseline sem inline slice (plano original line 278).

### DoD Phase 3
- [ ] `FsSourceProvider` implementa `SourceProvider`
- [ ] `build_inline_slices` chamado em `retrieve_files` no Stage 4.5b
- [ ] Reverse boost desabilitado quando inline slice presente
- [ ] 4 integration tests novos passando
- [ ] EM cross-function ≥ +15% vs baseline
- [ ] MRR ≥ 0.90
- [ ] `cargo test --workspace` green

---

## Phase 4: Observabilidade e métricas

> **LOC estimado:** ~40
> **Risco:** Zero (só instrumentação).
> **Dependências:** Phases 1-3.

Expor os 3 counters em `FileRetrievalResult`:
- `harm_removals: usize` — Phase 1
- `compression_savings_tokens: usize` — Phase 2
- `inline_slices_count: usize` — Phase 3

Publicar como `DomainEvent` no `EventBus` quando `retrieve_files` retorna, permitindo CLI/Desktop mostrar telemetria e benchmark validar os gates.

---

## Sequenciamento

```
Phase 1 (harm_filter)
    ↓
Phase 2 (code_compression)   ← pode ser paralela com Phase 1
    ↓
Phase 3 (inline_builder)     ← depende de 1+2 para ganho real
    ↓
Phase 4 (observability)
```

**Total:** ~4 commits, ~320 LOC, ~13 testes novos. Estimativa 4-6 horas de implementação focada.

---

## DoD Global

- [ ] `grep -r "filter_harmful_chunks\|compress_for_context\|build_inline_slices" crates/` retorna ≥ 3 call sites fora de `*_test.rs`
- [ ] Todos os 24 unit tests existentes continuam green
- [ ] ≥ 10 integration tests novos cobrindo as 3 integrações
- [ ] MRR benchmark ≥ 0.90
- [ ] Hygiene score ≥ 47.513 (o atual)
- [ ] Zero dependências externas novas
- [ ] Dependency direction preservada: `retrieval → graph/parser → domain`, nada cruza

---

## Não faz parte deste plano

1. **LLM-based summarization na compressão** — LongCodeZip paper usa LLM para summary; nosso `compress_for_context` é estritamente AST-based por simplicidade. Evolutivo.
2. **NLI para harm detection** — harm filter atual é heurístico (filename-based); NLI embeddings são upgrade futuro.
3. **Call-graph profundidade > 3** — `MAX_INLINE_DEPTH = 3` é conservador; aumentar exige benchmark de token budget vs precision.
4. **Cache de source lidos** — Phase 2 lê `fs::read_to_string` em cada `build_context_blocks`. Se benchmarks mostrarem I/O como gargalo, adicionar LRU cache.
