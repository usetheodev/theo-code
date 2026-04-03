# Meeting — 2026-04-03 (theo pilot — Autonomous Development Loop)

## Proposta
`theo pilot` — novo módulo PilotLoop que orquestra AgentLoop em ciclos contínuos até cumprir uma "promise". Inspirado nos patterns do Ralph (8k+ stars). Adição pura, zero mudança no RunEngine/AgentLoop.

## Participantes
- `governance` — veredito de governança
- `qa` — validação de testabilidade
- `runtime` — análise async, state management
- `infra` — custo, resiliência, limites operacionais

## Análises

### Governance
REJECT por diff uncommitted no workspace. Design do pilot é válido conceitualmente. Recomenda commitar features pendentes antes.

### QA
validated=true. Exige: (1) Clock injetável para circuit breaker, (2) ProgressDetector trait para git, (3) ~15 testes para circuit breaker + exit conditions. Base 199 testes green.

### Runtime
risk=HIGH. Identificou: (1) abort_tx é dead code — Ctrl+C não interrompe, (2) session trim perde objetivo original, (3) EventBus cresce se compartilhado, (4) AgentLoop ignora registry injetado. Recomenda: promise como System message fixa, EventBus novo por loop.

### Infra
risk=CRITICAL. max_total_calls=0 é ilimitado — custo descontrolado. Ambiguidade entre "LLM calls" vs "loop iterations". Circuit breaker permissivo (3 loops). Exige: defaults conservadores, max_tokens_per_session, observability.

## Conflitos
1. Governance REJECT por diff (override: meeting avalia design, não working tree)
2. max_total_calls: 0 vs 1000 vs 50 → DECISÃO: default 50
3. Rate limit naming: calls vs loops → DECISÃO: max_loops_per_hour (conta iterations)
4. Clock injetável: trait vs parametrizado → DECISÃO: parametrizado (simples)
5. Completion signal vazio: done() sem mudanças → DECISÃO: só conta se files_edited > 0

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-agent-runtime/src/pilot.rs` (NOVO — PilotLoop, PilotConfig, CircuitBreaker, PilotResult, ExitReason)
- `crates/theo-agent-runtime/src/lib.rs` (pub mod pilot)
- `crates/theo-agent-runtime/src/project_config.rs` (PilotConfig TOML parsing na section [pilot])
- `apps/theo-cli/src/pilot.rs` (NOVO — CLI runner, PilotRenderer)
- `apps/theo-cli/src/main.rs` (cmd_pilot subcommand, print_usage update)

## Condições
1. max_total_calls default = 50 (NÃO ilimitado)
2. max_loops_per_hour default = 20
3. Promise injetada como System message fixa (não no histórico rotativo)
4. EventBus novo por loop iteration
5. Completion signal só conta se files_edited > 0 ou git progress detected
6. Circuit breaker testável via parâmetro (cooldown_elapsed: bool)
7. Graceful shutdown: pilot loop deve checar flag de interrupção entre iterações
8. Git progress detection com fallback gracioso (projetos sem git)
9. cargo test 100% verde
10. Mínimo 10 testes: circuit breaker (5), exit conditions (3), rate limit (2)
