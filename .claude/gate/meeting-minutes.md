# Meeting — 2026-04-07 (Wiki Cache Semântico)

## Proposta
Wiki como primeira camada de retrieval no query_context.

## Participantes
- Governance (APPROVE 85%)

## Veredito
**APPROVED**

## Escopo Aprovado
- Novo: `crates/theo-engine-retrieval/src/wiki/lookup.rs`
- Mod: `crates/theo-engine-retrieval/src/wiki/mod.rs`
- Mod: `crates/theo-application/src/use_cases/graph_context_service.rs`

## Condições
1. Threshold calibrado para evitar false positives
2. Fallback transparente (zero regressão)
3. Testes unitários para lookup (match/miss/threshold)
