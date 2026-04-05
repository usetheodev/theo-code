# Meeting — 2026-04-05 (Default agent mode + rename to theo)

## Proposta
Sem subcommand = agent REPL. `theo "task"` = single-shot. Renomear binário para `theo`.

## Participantes
- **governance** — APPROVE (90%, alinha com UX da indústria)

## Veredito
**APPROVED**

## Escopo Aprovado
- EDIT: `apps/theo-cli/src/main.rs`
- EDIT: `apps/theo-cli/Cargo.toml`

## Condições
1. `theo` = REPL, `theo "task"` = single-shot
2. Subcommands mantidos: init, pilot, context, impact, stats
3. `theo agent` mantido como alias (backward compat)
