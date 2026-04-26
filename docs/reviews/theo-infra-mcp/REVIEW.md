# theo-infra-mcp — Revisao

> **Contexto**: Model Context Protocol client (modelcontextprotocol.io 2025-03-26). JSON-RPC 2.0 sobre stdio ou HTTP Streamable.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Escopo atual**: JSON-RPC types, `McpClient` trait, `StdioTransport`, `McpServerConfig`, `tools/list` + `tools/call`.
>
> **Fora de escopo (futuro)**: Resources protocol, OAuth 2.1 manager.
>
> **Status global**: deep-review concluido em 2026-04-25. 108 tests passando, 0 falhas. `cargo clippy -p theo-infra-mcp --lib --tests` zero warnings em codigo proprio (10+ fixes aplicados nesta auditoria).

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `client` | `McpClient` trait + `McpAnyClient` / `McpStdioClient` / `McpHttpClient`. | Revisado |
| 2 | `config` | `McpServerConfig` (name, command, env, args). | Revisado |
| 3 | `discovery` | `DiscoveryCache`, `DiscoveryReport`, `DEFAULT_PER_SERVER_TIMEOUT`. | Revisado |
| 4 | `dispatch` | `McpDispatcher` + `DispatchOutcome` — invocacao de tools MCP. | Revisado |
| 5 | `error` | `McpError` tipado. | Revisado |
| 6 | `protocol` | `McpRequest`, `McpResponse`, `McpTool` — JSON-RPC 2.0. | Revisado |
| 7 | `registry` | `McpRegistry` — catalogo de servidores MCP configurados. | Revisado |
| 8 | `transport_http` | Transport HTTP Streamable. | Revisado |
| 9 | `transport_stdio` | Transport stdio (subprocess, kill-on-drop). | Revisado |

---

## Notas de Deep-Review

### 1. client
`McpClient` trait async + `McpAnyClient` (enum wrapper de Stdio/Http variants) + concrete impls. `from_config` factory rejeita variants invalidos com `McpError::InvalidConfig`.

### 2. config
`McpServerConfig::{Stdio, Http}` enum. Stdio: `name, command, env, args`. Http: `name, url, headers, timeout_ms`. Carregado de `.theo/mcp_servers.toml`.

### 3. discovery
`DiscoveryCache` memoiza `tools/list` por server name. `DiscoveryReport { reachable, unreachable, total_tools }` + `DEFAULT_PER_SERVER_TIMEOUT = 10s`.

### 4. dispatch
`McpDispatcher` invoca `tools/call` com namespace `mcp:<server>:<tool>`. `DispatchOutcome::{Result, Error}`. Wired no theo-agent-runtime via subagent::mcp_tools.

### 5. error
`McpError::{Network, Serde, Timeout, InvalidConfig, ServerError, Disconnect, Other}`.

### 6. protocol
`McpRequest { jsonrpc, id, method, params }`, `McpResponse { jsonrpc, id, result, error }`, `McpTool { name, description, input_schema }`. JSON-RPC 2.0 strict.

### 7. registry
`McpRegistry` BTreeMap<name, server>. `render_prompt_hint(filter)` formata o XML hint para system prompt. Iter desta revisao: `.iter().map(|(n, _)| ...)` → `.keys().map(|n| ...)` (clippy::iter_kv_map).

### 8. transport_http
HTTP Streamable transport (SSE-based). `decode_sse_event(line, request_id) -> Result<Option<McpResponse>>`. Iter desta revisao: 3x `.err().expect(...)` → `.expect_err(...)` (clippy::err_expect); 1x `.map(|v| v.parse().ok()).flatten()` → `.and_then(|v| v.parse().ok())` (clippy::map_flatten).

### 9. transport_stdio
Subprocess via `tokio::process::Command` com kill-on-drop. Iter desta revisao: `is_alive` matchando `Ok(None) => true, _ => false` → `matches!(self.child.try_wait(), Ok(None))` (clippy::match_like_matches_macro).

**Hygiene fixes total nesta auditoria:**
- `registry.rs:63` — `.iter().map(|(n, _)| ...)` → `.keys().map(|n| ...)`
- `transport_stdio.rs:86` — match → matches!
- `client.rs:332,353` — 2x `.err().expect()` → `.expect_err()`
- `transport_http.rs:218,232,288` — 3x `.err().expect()` → `.expect_err()`
- `transport_http.rs:341` — `.map(...).flatten()` → `.and_then(...)`
- `tests/real_server.rs:82` — `.iter().any(|n| *n == x)` → `.contains(&x)`

**Validacao:**
- 108 tests passando, 0 falhas
- `cargo clippy -p theo-infra-mcp --lib --tests` zero warnings em codigo proprio (era 11 warnings antes desta auditoria)
- ADR dep invariant preservada: theo-domain (workspace) + tokio/serde/serde_json/reqwest/thiserror/async-trait/futures (external)

Sem follow-ups bloqueadores.
