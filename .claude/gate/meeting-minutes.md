# Meeting — 2026-04-03 (Agent Modes: Agent/Plan/Ask)

## Proposta
3 modos de interação: Agent (default), Plan, Ask via system prompt + config.

## Participantes
- governance, qa

## Veredito
**APPROVED**

## Escopo Aprovado
- crates/theo-agent-runtime/src/config.rs (enum AgentMode, system_prompt_for_mode fn)
- apps/theo-cli/src/repl.rs (mode state, prompt override antes de execute_task)
- apps/theo-cli/src/commands.rs (/mode handler — mode state no Repl, não no config)
- apps/theo-cli/src/main.rs (--mode flag)

## Condições
- Opção A: mode vive no Repl, system_prompt sobrescrito antes de cada execute_task
- AgentMode::default() == Agent testado
- 3 prompts distintos testados
- cargo test 100% verde
