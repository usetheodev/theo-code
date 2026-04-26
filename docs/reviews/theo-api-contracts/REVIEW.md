# theo-api-contracts — Revisao

> **Contexto**: DTOs e eventos do contrato publico de API. Consumido por `theo-application` e pelos apps.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Status global**: deep-review concluido em 2026-04-25. 13 tests passando, 0 falhas. `cargo clippy -p theo-api-contracts --lib --tests` zero warnings em codigo proprio (1 fix aplicado nesta auditoria).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `events` | Definicao canonica de eventos emitidos pela API (payloads serializaveis). | Revisado |

---

## Notas de Deep-Review

### 1. events
`FrontendEvent` enum com `#[serde(tag = "type", rename_all = "snake_case")]` para every-variant-is-distinguishable wire format. Variants: Token{text}, ToolStart{name, args}, ToolEnd{name, ok, output_preview}, AgentStart, AgentEnd, MetricsUpdate, ErrorEvent, etc. Tests inline incluem `every_variant_is_distinguishable_on_the_wire` que ronda-trip JSON serialize+deserialize.

**Iter desta revisao**: `events.rs:179` — `let variants = vec![...]` em test que so chama `.iter()` → array literal `[...]` (clippy::useless_vec).

**Validacao:**
- 13 tests passando, 0 falhas
- `cargo clippy -p theo-api-contracts --lib --tests` zero warnings em codigo proprio
- ADR dep invariant preservada: apenas theo-domain (workspace) + serde/serde_json (external)

Sem follow-ups bloqueadores.
