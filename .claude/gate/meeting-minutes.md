# Meeting — 2026-04-07 (Cleanup + Commit Mudanças Pendentes)

## Proposta
Remover código morto (expand_query), commitar mudanças testadas: new_fast(), PRF 1.3x, top_k 500, path_segments 3x, benchmark suite + ground truth JSON.

## Participantes
- Governance, QA

## Veredito
**APPROVED**

## Escopo Aprovado
- Mod: `crates/theo-engine-retrieval/src/code_tokenizer.rs` (remover expand_query)
- Mod: `crates/theo-engine-retrieval/src/dense_search.rs` (PRF 1.3x, já correto)
- Mod: `crates/theo-engine-retrieval/src/embedding/neural.rs` (new_fast + THEO_FAST_EMBED)
- Mod: `crates/theo-engine-retrieval/src/tantivy_search.rs` (top_k 500, path 3x, comments)
- Novo: `crates/theo-engine-retrieval/tests/benchmarks/` (ground truth + benchmark runner)

## Condições
1. Remover expand_query() antes do commit
2. 91+ testes passando após cleanup
