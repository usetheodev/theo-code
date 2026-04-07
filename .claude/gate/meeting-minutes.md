# Meeting — 2026-04-07 (Tier 2 Dense no GraphContextService)

## Proposta
Completar Tier 2: NeuralEmbedder + EmbeddingCache no GraphState + hybrid_rrf_search no query_context.

## Participantes
- Facilitador (fast-track — extensão natural do Tier 0+1 já commitado)

## Veredito
**APPROVED**

## Escopo Aprovado
- Mod: `crates/theo-application/src/use_cases/graph_context_service.rs`

## Condições
1. Feature-gated (dense-retrieval)
2. Sem features = zero mudança (24 testes passando)
3. Fallback cascade: Tier 2 → Tier 1 → Tier 0
4. NeuralEmbedder lazy (background, não bloqueia)
