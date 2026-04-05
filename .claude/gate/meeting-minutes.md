# Meeting — 2026-04-04 (Pilot Fase 1: HeuristicReflector)

## Proposta
Fase 1 do Pilot Self-Improving: enum FailurePattern (2 variantes) + HeuristicReflector com classify_failure() pure fn. Substituir build_corrective_guidance() inline.

## Participantes
- **governance** — NEEDS_REVISION → APPROVE com escopo reduzido (2 variantes, no runtime)
- **qa** — validated=true (10 DoDs verificáveis)

## Veredito
**APPROVED**

## Escopo Aprovado
- NEW: `crates/theo-agent-runtime/src/reflector.rs` (FailurePattern enum + HeuristicReflector + classify_failure + guidance_for_pattern)
- EDIT: `crates/theo-agent-runtime/src/pilot.rs` (campo reflector, substituir build_corrective_guidance)
- EDIT: `crates/theo-agent-runtime/src/lib.rs` (pub mod reflector)

## Escopo REMOVIDO vs proposta original
- NÃO criar failure_pattern.rs em theo-domain (enum fica no runtime)
- NÃO criar 7 variantes (só 2: NoProgressLoop, RepeatedSameError)

## Condições
1. classify_failure() é pure fn (sem IO, sem async, sem &self)
2. FailurePattern: apenas NoProgressLoop e RepeatedSameError
3. Threshold como constante nomeada (GUIDANCE_THRESHOLD = 2)
4. same_error >= 2 com last_error = None → retorna None
5. NoProgressLoop prioridade sobre RepeatedSameError quando ambos ativos
6. 8+ testes: success→None, below threshold→None, no_progress, same_error, both active, no last_error, guidance text
7. Os 2 testes existentes (corrective_guidance_*) passam sem modificação
8. 278 testes do runtime passam
9. Zero warnings
