# Meeting — 2026-04-06 (Reverse Dependency Boost — P@5 breakthrough)

## Proposta
Reverse Dependency Boost: após BM25 encontrar file #1, boost files que CHAMAM símbolos definidos no file #1. Filtros: só funções (não types), hub filter (lib.rs/mod.rs), cap 0.6.

## Participantes
- **graphctx** — Design concreto: reverse_neighbors de Function symbols, hub filter, cap

## Evidência
- LocAgent (ACL 2025): graph traversal from BM25 seed via call/import edges
- Aider: Personalized PageRank seeded from context files
- CodeCompass: BM25 + Graph combination = 99%

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-engine-retrieval/src/assembly.rs` (assemble_files_direct — reverse dep boost)

## Condições
1. Boost apenas para Function/Method symbols (não types/traits)
2. Hub filter: skip lib.rs, mod.rs, main.rs
3. Cap MAX_BOOST=0.6 por file
4. Eval P@5 deve subir de 0.360
