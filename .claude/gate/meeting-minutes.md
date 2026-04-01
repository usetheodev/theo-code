# Meeting — 2026-04-01 (Sandbox Fase 4)

## Proposta
Fase 4: Network Isolation via unshare(CLONE_NEWUSER | CLONE_NEWNET). Default deny, binary on/off. Whitelist de domínios ADIADA.

## Participantes
- governance — APPROVE
- qa — APPROVE (testes com mock + integration ignore)
- tooling — APPROVE (unshare compatível com landlock/rlimits)

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-tooling/src/sandbox/network.rs` (novo)
- `crates/theo-tooling/src/sandbox/mod.rs` (adicionar mod)
- `crates/theo-tooling/src/sandbox/executor.rs` (integrar unshare no pre_exec)
- `crates/theo-tooling/src/sandbox/probe.rs` (adicionar net_ns_available)

## Condicoes
1. Ordem no pre_exec: rlimits → unshare(NEWUSER|NEWNET) → landlock
2. Fallback graceful: se unshare falha, warning + continue sem net isolation
3. Probe detecta CLONE_NEWUSER + CLONE_NEWNET availability
4. Default: allow_network=false → aplica net ns
5. allow_network=true → NÃO aplica net ns
6. Whitelist de domínios ADIADA
7. Testes: unit com mock + integration com #[ignore]
8. Regressão: 75+ sandbox testes + 14 BashTool passam
