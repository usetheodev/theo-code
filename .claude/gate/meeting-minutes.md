# Meeting — 2026-04-02 (Task tools: replace_all → incremental by ID)

## Proposta
Refactor todowrite (replace_all, perde tasks) → task_create + task_update (incremental, nunca perde).

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-tooling/src/todo/mod.rs` (rewrite)
- `crates/theo-tooling/src/registry/mod.rs` (swap tools)
- `crates/theo-agent-runtime/src/config.rs` (system prompt)
- `apps/theo-cli/src/renderer.rs` (display)
