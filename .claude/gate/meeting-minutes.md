# Meeting — 2026-04-06 (Fase R3 — Cross-Encoder Reranker)

## Proposta
Adicionar cross-encoder reranker (MiniLM-L6 via fastembed TextRerank) ao pipeline. Stage 1: RRF → top-50. Stage 2: Reranker → top-20. Também: weighted RRF e test file filter no assembly.

## Participantes
- Governance (Principal Engineer)
- QA (Staff QA Engineer)
- Infra (SRE)

## Análises

### Governance
- APPROVE (confiança 85%)
- fastembed TextRerank confirmado em v4.9.1
- 8 condições: limite 50 candidatos, fallback para RRF, pipeline < 150 LOC, hard assert no eval

### QA
- validated = true (condicional)
- 5 testes obrigatórios: reranker output, degradação graceful, pipeline contrato, assembly filter, latência
- Risco: assembly filter pode regredir 8 testes existentes

### Infra
- APPROVE com restrições
- Modelo: MiniLM-L6 (22MB, ~3ms/doc, CPU) — não BGE-reranker (568MB)
- Latência: 50 × 3ms = 150ms — dentro do budget

## Conflitos
1. MiniLM-L6 treinado em MS-MARCO (NLP) — pode não ser eficaz em code identifiers
2. Assembly filter: pré-ranking vs pós-ranking — decidido pré-ranking
3. fastembed TextRerank API pode ter limitações não documentadas

## Veredito
**APPROVED**

## Escopo Aprovado
- Novo: `crates/theo-engine-retrieval/src/reranker.rs`
- Novo: `crates/theo-engine-retrieval/src/pipeline.rs`
- Mod: `crates/theo-engine-retrieval/src/tantivy_search.rs` (weighted RRF)
- Mod: `crates/theo-engine-retrieval/src/assembly.rs` (test file filter)
- Mod: `crates/theo-engine-retrieval/src/lib.rs` (register modules)
- Mod: `crates/theo-engine-retrieval/Cargo.toml` (feature reranker)
- Mod: `crates/theo-engine-retrieval/tests/eval_suite.rs` (eval_full_pipeline)

## Condições
1. Máximo 50 candidatos para reranking
2. Fallback para RRF quando reranker falha
3. pipeline.rs < 150 LOC de lógica
4. Hard assert P@5 >= 0.50 no eval
5. A/B: pipeline completo vs RRF sem reranker
6. Test file filter ANTES do ranking no assembly
7. MiniLM-L6 (22MB) — não BGE-reranker
8. 5 testes unitários obrigatórios
