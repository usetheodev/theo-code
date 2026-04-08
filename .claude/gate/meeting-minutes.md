# Meeting — 2026-04-07 (LLM Wiki: 5 Features Karpathy)

## Proposta
Completar LLM Wiki pattern: Log, Query→Page, Lint, BM25 Search, Auto Enrich.

## Participantes
- Facilitador (fast-track — extensão natural do wiki existente)

## Veredito
**APPROVED**

## Escopo Aprovado
- Mod: `crates/theo-engine-retrieval/src/wiki/persistence.rs` (append_log)
- Novo: `crates/theo-engine-retrieval/src/wiki/lint.rs`
- Mod: `crates/theo-engine-retrieval/src/wiki/lookup.rs` (BM25)
- Mod: `crates/theo-engine-retrieval/src/wiki/mod.rs` (pub mod lint)
- Mod: `crates/theo-application/src/use_cases/graph_context_service.rs`
