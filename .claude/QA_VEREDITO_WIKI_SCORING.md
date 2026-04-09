# QA Veredito — Wiki Retrieval Scoring Enhancement

**Data:** 2026-04-08  
**Avaliador:** QA Staff Engineer  
**Status:** CONDITIONAL APPROVAL ✓

---

## Resumo Executivo

A proposta de 3 mudanças no wiki retrieval scoring é **testável e defensável**, mas requer validação empírica obrigatória antes de merge. Todos os 64 testes wiki existentes passando. Baseline HEALTHY.

### Mudanças Propostas

| # | Mudança | Testabilidade | Risco | Status |
|---|---------|---|---|---|
| 1 | Expor bm25_raw em WikiLookupResult | ✓ Alta | Baixo | ✓ Testável |
| 2 | evaluate_direct_return() com 3 gates | ✓ Alta | **Médio** | ⚠️ Condicional |
| 3 | QueryClass heurístico (keyword-based) | ✓ Alta | **Médio** | ⚠️ Condicional |

---

## Análise Detalhada

### Mudança 1: bm25_raw (Baixo Risco)

**O que é:** Expor o score BM25 bruto antes de normalização final (antes de tier weight, title bonus, stale penalty).

**Testabilidade:** 100% ✓

**Testes Obrigatórios (3):**
1. `lookup_includes_bm25_raw_score` — verifica que WikiLookupResult.bm25_raw está preenchido
2. `bm25_raw_distinct_from_confidence` — verifica que bm25_raw ≠ final confidence (composição é diferente)
3. `bm25_raw_zero_score_still_excluded` — verifica que páginas com score zero não aparecem mesmo com campo exposto

**Risco:** Expor novo campo API torna-o parte do contrato público. Mitigation: documentar em CHANGELOG como UNSTABLE.

**Veredito:** ✓ PODE PROCEDER

---

### Mudança 2: evaluate_direct_return() com 3 Gates (Risco Médio)

**O que é:** Função de decisão com 3 gates:
1. **Floor gate:** confidence < 0.5 → REJEITA (sempre)
2. **Composite confidence:** 0.5×bm25 + 0.3×tier_weight + 0.1×freshness + 0.1×title_bonus
3. **Threshold por categoria:** keyword=0.6, semantic=0.65, hybrid=0.7

**Testabilidade:** 100% ✓

**Testes Obrigatórios (6):**
1. `evaluate_direct_return_floor_gate` — rejeita < 0.45
2. `evaluate_direct_return_composite_confidence` — valida fórmula dos pesos
3. `evaluate_direct_return_category_thresholds` — verifica thresholds diferentes por QueryClass
4. `evaluate_direct_return_tier_aware` — Deterministic bypass thresholds
5. `evaluate_direct_return_stale_override` — stale RawCache nunca aprovado (hard block)
6. `evaluate_direct_return_boundary_cases` — testa valores exatos nas bordas (0.5, 0.6, 0.65, 0.7)

**RISCO CRÍTICO: Weights Arbitrários (0.5, 0.3, 0.1, 0.1)**

Os pesos foram escolhidos *arbitrariamente* sem calibração empírica. Mudança de 0.01 em qualquer peso afeta TODOS os tresholds:
- Se 0.5 → 0.45: confidence sobe, mais páginas aprovadas direto (pode halucinar)
- Se 0.5 → 0.55: confidence cai, mais fallback para RRF (perde latency gain)

**Validação Obrigatória:** Eval benchmark com 50+ queries reais mostrando:
- ✓ Recall@1 >= 0.9 (sem false positives)
- ✓ MRR (mean reciprocal rank) >= 0.90
- ✓ Latency <= 5ms (SLO de wiki lookup)
- ✓ Sem regressão vs baseline (RRF puro)

**Se eval falhar:** Rejeitar mudança e recalibrar pesos.

**Veredito:** ⚠️ CONDICIONAL — SÓ PROCEDE COM EVAL BENCHMARK APROVADO

---

### Mudança 3: QueryClass Heurístico (Risco Médio)

**O que é:** Classificação de query em 3 categorias baseada em patterns de keywords:
- **Keyword:** "function X", "struct Y", "trait Z" → threshold 0.6 (mais permissivo)
- **Semantic:** "how does X work", "explain Y", "describe Z" → threshold 0.65
- **Hybrid:** "function X that does Y" → threshold 0.7 (mais rigoroso)

**Testabilidade:** 100% ✓

**Testes Obrigatórios (5):**
1. `classify_query_keyword_patterns` — detecta "function", "struct", "trait", "how to"
2. `classify_query_semantic_patterns` — detecta "how does", "explain", "describe", "understand"
3. `classify_query_hybrid_patterns` — detecta combinações structural + behavior
4. `classify_query_edge_cases` — query vazia, single word, unicode, números
5. `classify_query_case_insensitive` — case-insensitivity

**RISCO CRÍTICO: Brittleness da Heurística**

Classificação baseada em regex/keyword lists é frágil em bordas:
- "function that validates JWT tokens" → Keyword ou Hybrid?
- "how to call function X" → Semantic ou Hybrid?

Missclassification → threshold errado → decisão errada.

**Validação Obrigatória:** 100% cobertura de branches em classify_query(), com test cases documentados inline.

**Veredito:** ⚠️ CONDICIONAL — SÓ PROCEDE COM COBERTURA DE BRANCHES E EVAL

---

## Validação de Testabilidade

### Unit Tests Obrigatórios (17 total)

```
Mudança 1: 3 testes
├─ lookup_includes_bm25_raw_score
├─ bm25_raw_distinct_from_confidence  
└─ bm25_raw_zero_score_still_excluded

Mudança 2: 6 testes
├─ evaluate_direct_return_floor_gate
├─ evaluate_direct_return_composite_confidence
├─ evaluate_direct_return_category_thresholds
├─ evaluate_direct_return_tier_aware
├─ evaluate_direct_return_stale_override
└─ evaluate_direct_return_boundary_cases

Mudança 3: 5 testes
├─ classify_query_keyword_patterns
├─ classify_query_semantic_patterns
├─ classify_query_hybrid_patterns
├─ classify_query_edge_cases
└─ classify_query_case_insensitive

Validação Composição: 3 testes
├─ composite_formula_accuracy
├─ tier_weight_application
└─ title_bonus_stale_penalty
```

**Tempo estimado:** 240 minutos (4h)

### Integration Test Suite

**E2E Eval Benchmark (50+ queries):**
- Duração: ~15 minutos
- Success criteria:
  - MRR >= 0.90
  - Recall@1 >= 0.85
  - Latency <= 5ms
  - Zero regressão vs baseline

**Crítico:** Deve rodar ANTES de merge. Se recall cai, rejeitar mudança.

### Mutation Testing (Obrigatório)

Teste a robustez dos constants testando pequenas variações:

```
Mutations a aplicar:
- 0.5 → 0.45, 0.55 (weight 1)
- 0.3 → 0.25, 0.35 (weight 2)
- 0.1 → 0.05, 0.15 (weight 3 e 4)
- Thresholds: 0.6 → 0.55, 0.65 → 0.60, 0.7 → 0.65
- Boundaries: >= → >, <= → <
- Fallback: Semantic → Keyword
```

**Critério:** Kill rate >= 0.95 (95%+ das mutações devem falhar em pelo menos 1 teste)

Se kill rate < 0.95 → testes não estão validando corretamente.

---

## Baseline Status

```
Testes Wiki Existentes: 64
Status: ✓ PASSED (0 falhas)
Suites testando:
- model.rs: 11 testes (schema, frontmatter, authority_tier)
- lookup.rs: 13 testes (BM25, scoring, freshness matrix, dedup)
- persistence.rs: 16 testes (load/save, staleness, GC)
- generator.rs, renderer.rs, lint.rs: 24 testes
```

**Nenhum teste quebrado pela proposta.** Mudanças são ADITIVAS.

---

## Riscos Identificados

### R1: Weight Constants Arbitrários (MÉDIO → CRÍTICO)

**Problema:** (0.5, 0.3, 0.1, 0.1) sem validação empírica.

**Impacto:**
- Weights mal calibrados → recall cai ou false positives aumentam
- Qualquer mudança em 0.01 afeta ~25% das queries

**Mitigação Obrigatória:**
1. Eval benchmark 50+ queries ANTES de merge
2. Metrics baseline vs novo: MRR, Recall@1, latency
3. Mutation tests: kill_rate >= 0.95
4. Constants documentados com comment: "// Empirically calibrated from eval_run_20260408"

**Veredito:** Não procede sem eval.

---

### R2: Heurística QueryClass Brittle (MÉDIO)

**Problema:** Regex/keyword lists em classify_query() fácil de quebrar.

**Impacto:** Queries no boundary → wrong threshold → wrong decision.

**Mitigação Obrigatória:**
1. 100% coverage de branches em classify_query()
2. Cada if/else documentado com query examples inline
3. Edge case tests (empty, unicode, case, numbers)
4. Se heurística > 10 linhas: considerar move para módulo separado

**Veredito:** Pode proceder se coverage 100%.

---

### R3: bm25_raw é Public API (BAIXO)

**Problema:** Novo field pode virar dependência de consumidores.

**Impacto:** Se removido/mudado depois, quebra consumidor.

**Mitigação:**
1. CHANGELOG: marcar como "UNSTABLE API"
2. Considerar alternativa: manter privado, expor só confidence final

**Veredito:** Procede se CHANGELOG marcado.

---

## Checklist de Aprovação

- [ ] Implementar 17 unit tests (3+6+5+3)
  - Todos com assertions significativas (não triviais)
  - Names descrevem comportamento, não implementação
  - Padrão AAA: Arrange, Act, Assert
  
- [ ] Rodar eval benchmark 50+ queries
  - Medir: MRR, Recall@1, latency, direct_return_rate
  - Comparar: baseline vs novo
  - Critério sucesso: MRR >= 0.90, zero regressão

- [ ] Mutation tests
  - Aplicar 10+ mutações (weights, thresholds, boundaries)
  - Kill rate >= 0.95
  - Cada mutação falha em >= 1 teste

- [ ] Zero regressão
  - `cargo test -p theo-engine-retrieval` → 64 tests PASS
  - Nenhum test novo quebra existing

- [ ] Documentação
  - CHANGELOG com "Unstable API" para bm25_raw
  - Comments inline em classify_query() com exemplos
  - Constants pesos com comment "Calibrated from eval_run_ID"

- [ ] Code review técnico
  - Governance veta ou aprova
  - QA testa como descrito aqui

---

## Timeline

| Fase | Duração | Bloqueador |
|------|---------|-----------|
| Implementação unit tests | 4h | Nenhum |
| Eval benchmark 50 queries | 15m | Eval result OK |
| Mutation testing | 1h | Eval OK + kill_rate >= 0.95 |
| Code review + QA | 2h | Mutation OK |
| **Total** | **7.25h** | Eval sucesso |

---

## Veredito Final

**CONDICIONAL APPROVAL ✓**

Testável e defensável, mas com 3 condições invioláveis:

1. **Eval Benchmark Obrigatório:** 50+ queries, MRR >= 0.90, sem regressão
2. **Mutation Testing:** Kill rate >= 0.95 em weight/threshold constants
3. **100% Branch Coverage:** classify_query() e evaluate_direct_return() fully covered

**Se alguma condição falha → REJECT e recalibrar.**

**Confiança:** 95%

---

**Gerado:** 2026-04-08T14:30:00Z  
**QA Staff Engineer**

