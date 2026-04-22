---
id: 20260421-194325
date: 2026-04-21
topic: "Plano de implementacao de Context Management SOTA para Theo Code"
verdict: APPROVED
participants: 5
---

# Reuniao: Plano de implementacao de Context Management SOTA

## Pauta

Baseado na pesquisa academica completada (25 papers, 2022-2026), definir a estrategia de implementacao para levar o context management do Theo Code ao nivel state-of-the-art.

### Contexto atual
- RRF 3-ranker (BM25+Tantivy+Dense) com MRR=0.914
- Cross-encoder reranker (Jina v2, ONNX, ~10ms/doc)
- MCPH graph model (8 edge types, Tree-Sitter, 7 linguagens)
- Compaction at 80% threshold (PRESERVE_TAIL=6)
- ContextMetrics com causal usefulness + failure fingerprints
- Budget: 15% repo-map, 25% module-cards, 40% real-code, 15% task-history, 5% reserve

### 5 Findings da pesquisa
1. Graph retrieval colapsa em LLMs 128K+ → call-graph INLINING e SOTA (InlineCoder: +2.6x EM)
2. Remocao de contexto PREJUDICIAL da 4x mais EM que adicionar contexto (CODEFILTER)
3. Compressao code-aware (Tree-Sitter + perplexity) da 5.6x sem perda (LongCodeZip)
4. Simple observation masking = LLM summarization a 50% custo (Complexity Trap: 84% tokens sao observacoes)
5. COLA training alignment (+19.7% EM) — nao aplicavel diretamente (requer fine-tuning)

### Questoes decididas
1. Priorizacao de implementacao
2. Adaptacao do GRAPHCTX para call-graph inlining
3. Posicionamento do harm filter no pipeline
4. Integracao de observation masking com compaction existente
5. Faseamento e sequencia de PRs
6. Estrategia de benchmark

## Posicoes por Agente

### Estrategia
| Agente | Posicao | Resumo |
|--------|---------|--------|
| chief-architect | APPROVE | Subtrativo antes de aditivo. 5 fases: Benchmark → Masking → Harm Filter → Code Compression → Call-Graph Inlining. Pipeline permanece linear. Budget inviolavel. ~22-31 dias. |

### Engineering
| Agente | Posicao | Resumo |
|--------|---------|--------|
| graphctx-expert | APPROVE condicional | Infraestrutura ja existe (Calls edges, reverse_adjacency, name_index). Paradigma muda de "file" para "symbol-centered inline slice". 7 testes TDD detalhados. Gap: resolucao cross-file em call_graph.rs. |
| retrieval-engineer | APPROVE | Pipeline SOTA: RRF → Reranker → HarmFilter (Stage 3) → InlineExpander (Stage 4) → Assembly. Harm filter heuristico sem LLM. k-adaptativo. Score-proportional budget. |
| code-reviewer | CONCERN → APPROVE | 2 CRITICALs: (1) magic numbers acumulando — criar CompactionPolicy struct, (2) observation masking quebra contrato de idempotencia — implementar como estagio separado em compaction_stages.rs. Harm filter como funcao pura sem novos domain types. |
| arch-validator | APPROVE condicional | Harm filter DEVE ser heuristico (LLM-based viola boundary theo-engine-retrieval → theo-infra-llm). HarmScore/InliningMode em theo-domain OK. MaskingPolicy pertence a theo-governance. Nenhum ciclo criado. |

## Conflitos

### Conflito 1: CompactionPolicy vs implementacao direta
- **code-reviewer**: Criar CompactionPolicy struct ANTES de qualquer feature nova. Magic numbers sao tech debt.
- **chief-architect**: Iniciar com observation masking imediatamente no Fase 1.
- **Resolucao**: CONSENSO — Fase 0.5 (1 dia): extrair constantes para CompactionPolicy struct sem mudar comportamento. Testes existentes devem continuar passando. Depois Fase 1 usa a struct.

### Conflito 2: Onde vive MaskingPolicy
- **arch-validator**: MaskingPolicy → theo-governance (e policy, nao tipo puro).
- **code-reviewer**: Manter simples — parametros em CompactionPolicy, sem governance overhead.
- **Resolucao**: CONSENSO — Comecar com parametros simples em CompactionPolicy (KISS). Se crescer para regras complexas, mover para theo-governance. Regra de 3: so abstrai na terceira necessidade.

### Conflito 3: Novos tipos em theo-domain
- **arch-validator**: HarmScore e InliningMode OK em theo-domain.
- **code-reviewer**: Nao criar tipos prematuramente (YAGNI, feedback_measure_before_schema.md).
- **Resolucao**: CONSENSO — Harm filter como funcao pura com `f64` threshold em theo-engine-retrieval. Tipos em theo-domain SO quando mais de um crate precisar consumir. InliningMode pode ir em theo-domain porque GraphContextProvider ja vive la.

### Conflito 4: Idempotencia do masking
- **code-reviewer**: compact_if_needed e documentada como idempotente. Masking com janela deslizante nao e.
- **Resolucao**: CONSENSO — Masking e funcao separada `apply_observation_mask()` em compaction_stages.rs, chamada ANTES de compact_if_needed pelo dispatcher compact_staged. Documentar explicitamente que masking NAO e idempotente.

## Decisoes

### D1: Faseamento aprovado (subtrativo antes de aditivo)
```
Fase 0   — Benchmark baseline (2-3 dias, zero risco)
Fase 0.5 — CompactionPolicy struct refactor (1 dia, zero risco)
Fase 1   — Observation masking (3-4 dias, baixo risco)
Fase 2   — Harm filter heuristico (5-7 dias, medio risco)
Fase 3   — Code-aware compression (5-7 dias, medio risco)
Fase 4   — Call-graph inlining (7-10 dias, alto risco)
```

### D2: Pipeline SOTA aprovado
```
Query → [Stage 1] RRF 3-ranker (existente)
      → [Stage 2] Cross-encoder reranker (existente)
      → [Stage 3] Harm filter heuristico (NOVO)
      → [Stage 4] Call-graph inline expansion (NOVO, complementa expand_from_files)
      → [Assembly] Score-proportional budget (EVOLUCAO)
```

### D3: Harm filter e HEURISTICO, nao LLM-based
- Sinais: is_test_file, is_fixture, mentions_symbol_without_defining, redundancy_score
- Funcao pura em theo-engine-retrieval/src/harm_filter.rs
- Threshold conservador: so remove com confianca alta
- Se definer ja no top-5, test files que mencionam o simbolo sao removidos
- Sem novos domain types ate instrumentacao confirmar necessidade

### D4: Observation masking como estagio separado
- Funcao `apply_observation_mask(messages, window)` em compaction_stages.rs
- NAO embutido em compact_if_needed (preserva idempotencia)
- Chamado pelo dispatcher compact_staged no nivel Warning (70-80%)
- Window M=10 como default em CompactionPolicy (configuravel por modelo)
- Bypass condicional: se task_type == debug, manter observacoes completas

### D5: Call-graph inlining como modulo novo
- Novo modulo inline_builder.rs em theo-engine-retrieval
- NAO modifica assembly.rs diretamente
- Trigger: query com hit exato no name_index do MCPH graph
- Sem trigger → fallback para pipeline atual (sem regressao)
- Hard caps: max depth 3, max 500 tokens/chain, max 10 funcoes inlined
- InlineSlice como nova unidade de contexto (complementa, nao substitui files)

### D6: Finding #5 (COLA) arquivado
- Requer fine-tuning de modelo — fora do nosso controle
- Registrar em ADR como "known improvement, not actionable"

### D7: Metricas de gate por fase
- MRR >= 0.90 (nao pode regredir)
- Fase 1: token_efficiency melhora >= 20%
- Fase 2: harm_rate diminui >= 30%
- Fase 3: token_efficiency melhora >= 40% cumulativo
- Fase 4: EM melhora >= 15% em tasks cross-function

## Plano TDD

### Fase 0.5 — CompactionPolicy refactor
1. RED: `test_compaction_policy_defaults_match_current_constants`
2. GREEN: Criar CompactionPolicy struct, mover 4 constantes
3. REFACTOR: Funções de compaction recebem &CompactionPolicy
4. VERIFY: `cargo test -p theo-agent-runtime`

### Fase 1 — Observation Masking
1. RED: `test_observation_mask_preserves_last_m_observations`
2. RED: `test_observation_mask_replaces_old_observations_with_header`
3. RED: `test_observation_mask_preserves_non_tool_messages`
4. RED: `test_observation_mask_bypass_when_debug_task`
5. GREEN: Implementar `apply_observation_mask` em compaction_stages.rs
6. REFACTOR: Integrar com dispatcher compact_staged
7. VERIFY: `cargo test -p theo-agent-runtime`

### Fase 2 — Harm Filter
1. RED: `test_harm_filter_removes_test_file_when_definer_present`
2. RED: `test_harm_filter_keeps_user_when_definer_absent`
3. RED: `test_harm_filter_noop_when_all_definers`
4. RED: `test_harm_filter_removes_redundant_chunks`
5. GREEN: Implementar `filter_harmful_chunks` em harm_filter.rs
6. REFACTOR: Integrar como Stage 3 no pipeline
7. VERIFY: `cargo test -p theo-engine-retrieval`

### Fase 3 — Code Compression
1. RED: `test_compress_keeps_signature_removes_body_for_irrelevant`
2. RED: `test_compress_keeps_full_body_for_relevant_symbols`
3. RED: `test_compress_collapses_imports_to_list`
4. GREEN: Implementar `compress_for_context` em theo-engine-parser
5. REFACTOR: Integrar com assembly
6. VERIFY: `cargo test -p theo-engine-parser && cargo test -p theo-engine-retrieval`

### Fase 4 — Call-Graph Inlining
1. RED: `test_calls_children_returns_only_calls_targets` (model.rs)
2. RED: `test_symbol_source_range_returns_file_and_lines` (model.rs)
3. RED: `test_inline_builder_includes_callee_source`
4. RED: `test_inline_builder_includes_caller_snippets`
5. RED: `test_inline_builder_degrades_for_unresolved_callee`
6. RED: `test_inline_builder_respects_token_budget`
7. RED: `test_mrr_does_not_regress_below_0_86` (benchmark)
8. GREEN: Implementar calls_children, symbol_source_range, InlineBuilder
9. REFACTOR: Integrar como Stage 4 no file_retriever
10. VERIFY: `cargo test -p theo-engine-graph && cargo test -p theo-engine-retrieval`

## Action Items
- [ ] Fase 0: Criar benchmark baseline dataset (30 tasks reais) — `theo-benchmark`
- [ ] Fase 0.5: Extrair CompactionPolicy struct — `theo-agent-runtime/src/config.rs`
- [ ] Fase 1: Implementar apply_observation_mask — `theo-agent-runtime/src/compaction_stages.rs`
- [ ] Fase 2: Implementar harm_filter.rs — `theo-engine-retrieval/src/harm_filter.rs`
- [ ] Fase 3: Implementar compress_for_context — `theo-engine-parser/src/compress.rs`
- [ ] Fase 4: Implementar inline_builder.rs — `theo-engine-retrieval/src/inline_builder.rs`
- [ ] ADR: Registrar Finding #5 (COLA) como arquivado — `docs/adr/`

## Veredito Final
**APPROVED**: Plano de 5 fases aprovado por consenso. Ordem: subtrativo antes de aditivo. Todos os concerns resolvidos: CompactionPolicy como Fase 0.5, masking separado da idempotencia, harm filter heuristico sem LLM, novos domain types so com instrumentacao. TDD obrigatorio em todas as fases. Gate de MRR >= 0.90 inviolavel.
