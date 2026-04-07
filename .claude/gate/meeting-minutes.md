# Meeting — 2026-04-07 (Otimização Memória Tier 2)

## Proposta
3 otimizações: quantized model, eliminar scorer, Tantivy mmap.

## Participantes
- Facilitador, Infra

## Veredito
**APPROVED** (parcial: otimizações 1+2. Mmap deferido.)

## Escopo Aprovado
- Mod: `crates/theo-engine-retrieval/src/embedding/neural.rs` (AllMiniLML6V2Q)
- Mod: `crates/theo-application/src/use_cases/graph_context_service.rs` (scorer → Option)

## Condições
1. MRR >= 0.85 após modelo quantizado (validar no benchmark)
2. Tier 0 fallback preservado (FileBm25 não depende de scorer)
3. Mmap deferido até ter baseline de latência
