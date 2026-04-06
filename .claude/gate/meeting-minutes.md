# Meeting — 2026-04-06 (Q3 Simplificação GRAPHCTX)

## Proposta
Deletar 2426 linhas dead code experimental + simplificar scorer 6→4 sinais.

## Participantes
- **governance** — APPROVE (92%). Zero imports dos 7 módulos. Correlação BM25/TF-IDF justifica remoção.

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-engine-retrieval/src/experimental/bandit.rs` (deletar)
- `crates/theo-engine-retrieval/src/experimental/cascade.rs` (deletar)
- `crates/theo-engine-retrieval/src/experimental/contrastive.rs` (deletar)
- `crates/theo-engine-retrieval/src/experimental/ensemble.rs` (deletar)
- `crates/theo-engine-retrieval/src/experimental/feedback.rs` (deletar)
- `crates/theo-engine-retrieval/src/experimental/memory.rs` (deletar)
- `crates/theo-engine-retrieval/src/experimental/predictive.rs` (deletar)
- `crates/theo-engine-retrieval/src/experimental/mod.rs` (atualizar)
- `crates/theo-engine-retrieval/src/search.rs` (simplificar scorer)

## Condições
1. cargo test -p theo-engine-retrieval verde após cada task
2. compress.rs permanece (único módulo usado)
