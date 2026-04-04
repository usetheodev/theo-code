# Meeting — 2026-04-04 (Hooks System)

## Proposta
Hooks system para extensibilidade. Plugin loading ADIADO (YAGNI).

## Participantes
- governance

## Conflitos
- Plugins rejeitados (YAGNI — zero use cases concretos)
- run_engine.rs refactor desejável mas não bloqueante

## Veredito
**APPROVED** (Hooks only)

## Escopo Aprovado
- crates/theo-agent-runtime/src/hooks.rs (NOVO — HookRunner + HookConfig)
- crates/theo-agent-runtime/src/lib.rs (pub mod hooks)
- crates/theo-agent-runtime/src/run_engine.rs (hook calls via HookRunner)
- crates/theo-agent-runtime/src/project_config.rs (hook config from .theo/config.toml)

## Condições
- Hooks herdam SandboxConfig (segurança)
- Timeout 5s default, 30s max
- Pre-hooks podem bloquear (exit != 0)
- Post-hooks fire-and-forget
- Mínimo 5 testes
- 0 warnings
