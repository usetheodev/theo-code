# Plano: Context Management SOTA para Theo Code

## Context

A pesquisa acadêmica (25 papers, 2022-2026) identificou 5 findings que podem levar o context management do Theo Code ao nível state-of-the-art. Reunião com 5 agentes aprovou o plano por consenso (ata: `.claude/meetings/20260421-194325-context-management-sota-plan.md`). O princípio guia é **"subtrativo antes de aditivo"**: primeiro remover/comprimir contexto ruim, depois enriquecer com contexto estrutural.

**Estado atual:** RRF 3-ranker (MRR=0.914), Jina v2 reranker, MCPH graph (8 edge types), compaction at 80%.

---

## Fase 0: Benchmark Baseline

**Objetivo:** Sem baseline não há como medir impacto.

**Crate:** `apps/theo-benchmark`

- Criar dataset de 30 ground-truth queries (10 feature, 10 bugfix, 10 refactor) extraídas do próprio repo
- Métricas: MRR, EM, token_efficiency (tokens úteis / tokens totais), harm_rate
- Snapshot dos resultados como baseline
- Gate: toda fase deve manter MRR >= 0.90

**Estimativa:** 2-3 dias | **Risco:** Nenhum

---

## Fase 0.5: CompactionPolicy Refactor

**Objetivo:** Eliminar magic numbers antes de adicionar novos (CRITICAL do code-reviewer).

**Crate:** `theo-agent-runtime`

**Arquivos:**
- `crates/theo-agent-runtime/src/config.rs` — adicionar `CompactionPolicy` struct
- `crates/theo-agent-runtime/src/compaction.rs` — receber `&CompactionPolicy` ao invés de ler consts
- `crates/theo-agent-runtime/src/compaction_stages.rs` — idem

**Implementação:**
```rust
// config.rs
#[derive(Debug, Clone)]
pub struct CompactionPolicy {
    pub preserve_tail: usize,               // default: 6
    pub truncate_tool_result_chars: usize,   // default: 200
    pub compact_threshold: f64,              // default: 0.80
    pub prune_keep_recent: usize,            // default: 3
    pub observation_mask_window: usize,      // default: 10 (Fase 1)
}

impl Default for CompactionPolicy {
    fn default() -> Self { /* valores atuais */ }
}
```

**TDD:**
1. RED: `test_compaction_policy_defaults_match_current_behavior` — compactar com policy default produz mesmo resultado que constantes atuais
2. GREEN: Criar struct, passar por parâmetro
3. REFACTOR: Remover constantes módulo-level

**Estimativa:** 1 dia | **Risco:** Zero (refactor puro, sem mudança de comportamento)

---

## Fase 1: Observation Masking

**Objetivo:** Reduzir 50% dos tokens de custo. 84% dos tokens do agente são observações de tools.

**Paper:** The Complexity Trap (2508.21433) — masking M=10 = LLM summarization a 50% do custo.

**Crate:** `theo-agent-runtime`

**Arquivo:** `crates/theo-agent-runtime/src/compaction_stages.rs` — nova função

**Implementação:**
```rust
/// Mask tool observations outside the recent window.
/// NOT idempotent (sliding window). Called BEFORE compact_if_needed.
pub fn apply_observation_mask(messages: &mut Vec<Message>, window: usize) {
    // 1. Identificar mensagens Role::Tool (observações)
    // 2. Contar de trás pra frente: manter últimas `window` completas
    // 3. Observações anteriores → substituir content por header resumido:
    //    "[masked] Tool: {name} | {original_size} chars | {first_line}"
    // 4. NÃO remover mensagens (preserva pares tool_call/tool_result)
    // 5. Bypass: se CompactionContext.current_phase == "DEBUG", skip masking
}
```

**Integração:** Chamada por `compact_staged` no nível `OptimizationLevel::Warning` (70-80%), ANTES de `compact_if_needed`.

**TDD:**
1. RED: `test_mask_preserves_last_m_observations` — M=3, 5 tools → 2 masked, 3 preservadas
2. RED: `test_mask_replaces_content_with_header` — content masked contém nome da tool e tamanho original
3. RED: `test_mask_preserves_non_tool_messages` — user/assistant messages intocadas
4. RED: `test_mask_bypass_when_debug_phase` — current_phase="DEBUG" → nenhum masking
5. RED: `test_mask_handles_empty_messages` — edge case
6. GREEN: Implementar `apply_observation_mask`
7. VERIFY: `cargo test -p theo-agent-runtime`

**Gate:** token_efficiency melhora >= 20%, MRR >= 0.90

**Estimativa:** 3-4 dias | **Risco:** Baixo

---

## Fase 2: Harm Filter Heurístico

**Objetivo:** Remover chunks prejudiciais. EM salta de 7.23% para 28.92% (4x) com remoção.

**Paper:** CODEFILTER (2508.05970)

**Crate:** `theo-engine-retrieval`

**Arquivo novo:** `crates/theo-engine-retrieval/src/harm_filter.rs`

**Posição no pipeline:**
```
RRF top-50 → Reranker top-20 → [HarmFilter top-N] → Assembly
```

**Implementação:**
```rust
/// Heuristic harm filter. Removes chunks that are likely to confuse the LLM.
/// Conservative: only removes with high confidence. No LLM dependency.
pub fn filter_harmful_chunks(
    query: &str,
    candidates: &mut Vec<(String, f64)>,  // (file_path, score)
    graph: &CodeGraph,
    threshold: f64,  // default: 0.3 — remove se harm_score > threshold
) {
    // Sinais de harm:
    // 1. is_test_file que MENCIONA símbolo do query mas NÃO define → harm
    //    (SÓ se o definer já está no top-5)
    // 2. is_fixture/mock sem lógica de negócio → harm
    // 3. Redundância: similarity > 0.95 com outro candidato de score maior → harm
    // 4. Arquivo de config/build que matched por keyword → harm
    
    // Conservador: máximo 5 remoções por query
}
```

**TDD:**
1. RED: `test_harm_filter_removes_test_when_definer_present`
2. RED: `test_harm_filter_keeps_user_when_definer_absent`
3. RED: `test_harm_filter_noop_when_all_definers`
4. RED: `test_harm_filter_removes_redundant_high_similarity`
5. RED: `test_harm_filter_max_removals_capped_at_5`
6. RED: `test_harm_filter_empty_input`
7. GREEN: Implementar `filter_harmful_chunks`
8. REFACTOR: Integrar no pipeline entre reranker e assembly
9. VERIFY: `cargo test -p theo-engine-retrieval`

**Gate:** harm_rate diminui >= 30%, MRR >= 0.90

**Estimativa:** 5-7 dias | **Risco:** Médio (falso positivo pode remover contexto necessário)

---

## Fase 3: Code-Aware Compression

**Objetivo:** 5.6x compressão sem perda usando Tree-Sitter (que já temos).

**Papers:** LongCodeZip (2510.00446), HCP (2406.18294)

**Crate:** `theo-engine-parser` (compressor) + `theo-engine-retrieval` (integração)

**Arquivo novo:** `crates/theo-engine-parser/src/compress.rs`

**Implementação:**
```rust
/// Compress source code for context injection.
/// Keeps relevant symbols full, compresses irrelevant to signatures.
pub fn compress_for_context(
    source: &str,
    language: Language,          // Tree-Sitter language
    relevant_symbols: &[&str],  // símbolos que o query referencia
    max_tokens: usize,
) -> String {
    // 1. Parse com Tree-Sitter (já existe em theo-engine-parser)
    // 2. Identificar funções/classes no AST
    // 3. Símbolos em relevant_symbols → manter body completo
    // 4. Outros símbolos → manter APENAS signature + docstring, omitir body
    // 5. Imports → colapsar em lista compacta
    // 6. Respeitar max_tokens
}
```

**Integração:** Chamar `compress_for_context` no assembly antes de inserir code blocks no prompt.

**TDD:**
1. RED: `test_compress_keeps_full_body_for_relevant_symbol`
2. RED: `test_compress_reduces_irrelevant_to_signature`
3. RED: `test_compress_collapses_imports`
4. RED: `test_compress_respects_max_tokens`
5. RED: `test_compress_handles_multiple_languages` (Rust, Python, TypeScript)
6. GREEN: Implementar usando Tree-Sitter queries existentes
7. VERIFY: `cargo test -p theo-engine-parser && cargo test -p theo-engine-retrieval`

**Gate:** token_efficiency >= 40% cumulativo (com Fase 1), MRR >= 0.90

**Estimativa:** 5-7 dias | **Risco:** Médio

---

## Fase 4: Call-Graph Inlining

**Objetivo:** Mudar paradigma de "arquivo como unidade" para "símbolo + callers/callees como unidade".

**Paper:** InlineCoder (2601.00376) — +2.6x EM no RepoExec com DeepSeek-V3

**Crates:** `theo-engine-graph` (helpers) + `theo-engine-retrieval` (builder)

**Arquivos:**
- `crates/theo-engine-graph/src/model.rs` — adicionar `calls_children()` e `symbol_source_range()`
- `crates/theo-engine-retrieval/src/inline_builder.rs` — **NOVO** módulo

**Pré-requisito no graph (model.rs):**
```rust
// Análogo a contains_children_index, mas para Calls edges
pub fn calls_children(&self, source_id: &str) -> Vec<String> { /* ... */ }

// Retorna (file_path, line_start, line_end) para um símbolo
pub fn symbol_source_range(&self, node_id: &str) -> Option<(&str, usize, usize)> { /* ... */ }
```

**InlineBuilder:**
```rust
pub struct InlineSlice {
    pub focal_symbol_id: String,
    pub focal_source: String,
    pub upstream_callers: Vec<SymbolSnippet>,   // quem chama o focal
    pub downstream_callees: Vec<SymbolSnippet>, // quem o focal chama
    pub token_count: usize,
}

pub struct SymbolSnippet {
    pub symbol_id: String,
    pub file_path: String,
    pub source: String,       // código fonte (possivelmente comprimido via Fase 3)
    pub depth: u8,
}

/// Build an inline slice centered on a focal symbol.
pub fn build_inline_slice(
    focal_symbol: &str,
    graph: &CodeGraph,
    repo_root: &Path,
    budget_tokens: usize,
) -> Result<InlineSlice, InlineError> {
    // 1. Localizar focal no graph via name_index
    // 2. calls_children(focal) → callees (downstream)
    // 3. reverse_adjacency filtrando EdgeType::Calls → callers (upstream)
    // 4. Para cada: ler source via symbol_source_range + fs::read
    // 5. Comprimir via compress_for_context (Fase 3)
    // 6. Budget: 40% focal, 35% callees, 25% callers
    // 7. Hard caps: max depth 3, max 500 tokens/chain, max 10 funções
    // 8. Degradação graceful: callee não resolvido → signature-only
}
```

**Trigger:** Query com hit exato no `name_index` do MCPH graph. Sem hit → fallback pipeline atual.

**Integração com file_retriever:** Novo Stage 4 entre HarmFilter e Assembly. `FileRetrievalResult` ganha campo `inline_slices: Vec<InlineSlice>`.

**Mutuamente exclusivo com reverse boost:** Se inline_slices disponível para um arquivo, NÃO aplicar reverse dependency boost em `assemble_files_direct` (evita dupla contagem).

**TDD:**
1. RED: `test_calls_children_returns_only_calls_targets` (model.rs)
2. RED: `test_symbol_source_range_returns_correct_location` (model.rs)
3. RED: `test_inline_builder_includes_callee_source`
4. RED: `test_inline_builder_includes_caller_snippets`
5. RED: `test_inline_builder_degrades_for_unresolved_callee`
6. RED: `test_inline_builder_respects_token_budget`
7. RED: `test_inline_builder_noop_when_no_symbol_match`
8. RED: `test_mrr_does_not_regress_below_0_86` (benchmark)
9. GREEN: Implementar calls_children, symbol_source_range, build_inline_slice
10. VERIFY: `cargo test -p theo-engine-graph && cargo test -p theo-engine-retrieval`

**Gate:** EM melhora >= 15% em tasks cross-function, MRR >= 0.90

**Estimativa:** 7-10 dias | **Risco:** Alto (complexidade, resolução cross-file)

---

## Resumo

| Fase | O quê | Crates | Dias | Risco | Gate |
|------|-------|--------|------|-------|------|
| 0 | Benchmark baseline | theo-benchmark | 2-3 | Zero | Snapshot salvo |
| 0.5 | CompactionPolicy struct | theo-agent-runtime | 1 | Zero | Testes existentes passam |
| 1 | Observation masking | theo-agent-runtime | 3-4 | Baixo | token_eff +20% |
| 2 | Harm filter heurístico | theo-engine-retrieval | 5-7 | Médio | harm_rate -30% |
| 3 | Code compression | theo-engine-parser | 5-7 | Médio | token_eff +40% cum. |
| 4 | Call-graph inlining | theo-engine-graph, theo-engine-retrieval | 7-10 | Alto | EM +15% |

**Total: ~23-32 dias, 6 PRs incrementais.**

## Verificação

Após cada fase:
```bash
cargo test                                    # Todos os testes passam
cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored  # Benchmark não regride
```

Após Fase 4 completa:
- MRR >= 0.90 (inviolável)
- token_efficiency >= 40% melhor que baseline
- harm_rate >= 30% menor que baseline
- EM >= 15% melhor em tasks cross-function
