# QA Validação Rápida — Wiki Scoring (2026-04-08)

## Status: CONDITIONAL APPROVAL ✓

Testável, mas requer 3 validações invioláveis antes de merge.

---

## 3 Mudanças Avaliadas

| # | Mudança | Testes | Risco | Status |
|---|---------|--------|-------|--------|
| 1 | bm25_raw field | 3 | Baixo | ✓ Proceed |
| 2 | evaluate_direct_return 3-gate | 6 | **Médio → Crítico** | ⚠️ Condicional |
| 3 | QueryClass heurístico | 5 | **Médio** | ⚠️ Condicional |

---

## Testes Obrigatórios (17 total)

### Mudança 1: bm25_raw (3 testes)
- ✓ `lookup_includes_bm25_raw_score`
- ✓ `bm25_raw_distinct_from_confidence`
- ✓ `bm25_raw_zero_score_still_excluded`

### Mudança 2: evaluate_direct_return (6 testes)
- ✓ `evaluate_direct_return_floor_gate`
- ✓ `evaluate_direct_return_composite_confidence`
- ✓ `evaluate_direct_return_category_thresholds`
- ✓ `evaluate_direct_return_tier_aware`
- 🔴 **CRITICAL** `evaluate_direct_return_stale_override`
- ✓ `evaluate_direct_return_boundary_cases`

### Mudança 3: QueryClass (5 testes)
- ✓ `classify_query_keyword_patterns`
- ✓ `classify_query_semantic_patterns`
- ✓ `classify_query_hybrid_patterns`
- ✓ `classify_query_edge_cases`
- ✓ `classify_query_case_insensitive`

### Composição (3 testes)
- ✓ `composite_formula_accuracy`
- ✓ `tier_weight_application`
- ✓ `title_bonus_stale_penalty`

---

## Riscos Críticos

### R1: Weights (0.5, 0.3, 0.1, 0.1) — Arbitrários
**Problema:** Sem validação empírica  
**Impacto:** Mudança 0.01 afeta ~25% das queries  
**Mitigação:** ✋ **EVAL BENCHMARK 50+ queries OBRIGATÓRIO**
- MRR >= 0.90
- Recall@1 >= 0.85
- Zero regressão vs baseline

### R2: QueryClass Heurística — Brittle
**Problema:** Regex em bordas pode missclassify  
**Impacto:** Wrong threshold → wrong decision  
**Mitigação:** ✋ **100% BRANCH COVERAGE OBRIGATÓRIO**

### R3: bm25_raw é Public API
**Problema:** Novo field pode virar dependência  
**Mitigação:** CHANGELOG "UNSTABLE API"

---

## Validações Pré-Merge (Invioláveis)

- [ ] 17 unit tests implementados (assertions significativas)
- [ ] Eval benchmark: MRR >= 0.90, zero regressão
- [ ] Mutation testing: kill_rate >= 0.95
- [ ] 64 wiki tests PASS (zero regressão)
- [ ] CHANGELOG "Unstable API" para bm25_raw
- [ ] Comments inline em classify_query() com exemplos

**SEM ESTAS → REJECT**

---

## Timeline

| Fase | Tempo | Bloqueador |
|------|-------|-----------|
| Unit tests | 4h | Nenhum |
| Eval benchmark | 15m | eval_ok |
| Mutation tests | 1h | kill_rate >= 0.95 |
| Code review | 2h | mutation_ok |
| **TOTAL** | **7.25h** | eval_ok |

SE EVAL FALHAR → rejeitar + recalibrar (1-2 semanas)

---

## Baseline Status

```
Testes Wiki Existentes: 64
Status: ✓ PASSED (0 falhas)
Mudanças propostas: ADITIVAS (sem regredir)
```

---

## Arquivos de Referência

1. **qa_validation_wiki_scoring.json** — Validação estruturada completa
2. **test_implementation_spec.json** — Setup/assertions para cada teste
3. **QA_VEREDITO_WIKI_SCORING.md** — Veredito técnico em português
4. **QUICK_REFERENCE.md** — Este documento

---

## Próximos Passos

1. Implementar 17 testes (3+6+5+3)
2. Rodar `cargo test -p theo-engine-retrieval --lib wiki`
3. Executar eval benchmark
4. Se MRR >= 0.90 → proceder com mutation tests
5. Se kill_rate >= 0.95 → submit para governance review

---

**Confiança:** 95%  
**Gerado:** 2026-04-08  
**QA Staff Engineer**
