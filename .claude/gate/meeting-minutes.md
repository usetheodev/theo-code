# Meeting — 2026-04-04 (Runtime Guards)

## Proposta
Hard enforcement: Plan mode guard, Doom loop abort, AgentMode no config.

## Participantes
- governance

## Conflitos
- Ask guard removido do RunEngine (vive no REPL)
- Batch bypass: guard aplicado dentro do batch handler também
- AgentMode adicionado ao AgentConfig como pré-requisito

## Veredito
**APPROVED**

## Escopo Aprovado
- crates/theo-agent-runtime/src/config.rs (mode: AgentMode field)
- crates/theo-agent-runtime/src/run_engine.rs (Plan guard + batch guard + doom abort)
- apps/theo-cli/src/repl.rs (propagar mode ao config)

## Condições
- Plan guard: whitelist read-only tools, block write tools exceto .theo/plans/
- Batch: validar cada call contra mesma whitelist
- Doom: abort em threshold*2
- Testes: plan_guard_blocks_edit, plan_guard_allows_roadmap_write, doom_abort_at_double_threshold
- cargo test 100% verde, 0 warnings
