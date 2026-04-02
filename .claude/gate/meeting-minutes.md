# Meeting — 2026-04-02 (Budget defaults → Claude Code parity)

## Proposta
Alinhar defaults com Claude Code: sem limite prático de iterações, 1M tokens, 1h time.

## Veredito
**APPROVED** — config change only, todos valores configuráveis pelo usuário.

## Escopo Aprovado
- `crates/theo-agent-runtime/src/config.rs` (max_iterations default)
- `crates/theo-domain/src/budget.rs` (Budget::default)
