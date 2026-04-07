# Meeting — 2026-04-07 (Integração RRF no GraphContextService)

## Proposta
Integrar RRF 3-ranker no GraphContextService com 3 tiers, lazy enhancement, fallback cascade.

## Participantes
- Governance, QA

## Veredito
**REJECTED**

## Motivo
Working tree suja com mudanças de symbol_first pipeline (meeting anterior). Zero diff para os 3 files propostos. CLI theo-cli importa infra diretamente (bounded context violation) — feature deve ser forwarded via theo-application, não direto no CLI.

## Ação
1. Commitar mudanças atuais (symbol_first + benchmark A/B)
2. Criar branch dedicada para integração RRF
3. Feature forwarding exclusivamente via theo-application
4. Resubmeter meeting com tree limpo
