# Meeting — 2026-04-07 (LLM Enrichment Code Wiki)

## Proposta
LLM enrichment para wiki pages via theo-infra-llm.

## Participantes
- Facilitador (fast-track — design validado, bounded contexts corretos)

## Veredito
**APPROVED**

## Escopo Aprovado
- Novo: `crates/theo-application/src/use_cases/wiki_enrichment.rs`
- Mod: `crates/theo-application/src/use_cases/mod.rs`

## Condições
1. Opt-in (não automático)
2. Seções determinísticas preservadas intactas
3. Fallback por page se LLM falha
4. Funciona com qualquer provider OA-compatible
