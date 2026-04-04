# Meeting — 2026-04-03 (Dogfood Improvements: 6 items)

## Proposta
6 melhorias encontradas no dogfood (item 2 removido — já corrigido).

## Participantes
- governance

## Veredito
**APPROVED**

## Escopo Aprovado
- apps/theo-cli/src/main.rs (item 1: --prompt inline para agent)
- apps/theo-cli/src/repl.rs (item 1: single-shot mode + item 7: spacing fix)
- crates/theo-agent-runtime/src/pilot.rs (item 3: loop summary print)
- crates/theo-agent-runtime/src/run_engine.rs (item 4: metrics.record_delegated_tokens)
- crates/theo-agent-runtime/src/metrics.rs (item 4: new method)
- crates/theo-agent-runtime/src/subagent/mod.rs (item 6: project_dir in prompts)
- .claude/skills/dogfood/SKILL.md (item 5: Ask mode test)

## Condições
- Item 4: teste unitário para record_delegated_tokens
- Item 7: verificar spacing com grep
- cargo test 100% verde, 0 warnings
