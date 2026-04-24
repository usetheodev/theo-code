# theo-infra-mcp — Revisao

> **Contexto**: Model Context Protocol client (modelcontextprotocol.io 2025-03-26). JSON-RPC 2.0 sobre stdio ou HTTP Streamable.
>
> **Dependencias permitidas**: `theo-domain`.
>
> **Escopo atual**: JSON-RPC types, `McpClient` trait, `StdioTransport`, `McpServerConfig`, `tools/list` + `tools/call`.
>
> **Fora de escopo (futuro)**: Resources protocol, OAuth 2.1 manager.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `client` | `McpClient` trait + `McpAnyClient` / `McpStdioClient` / `McpHttpClient`. | Pendente |
| 2 | `config` | `McpServerConfig` (name, command, env, args). | Pendente |
| 3 | `discovery` | `DiscoveryCache`, `DiscoveryReport`, `DEFAULT_PER_SERVER_TIMEOUT`. | Pendente |
| 4 | `dispatch` | `McpDispatcher` + `DispatchOutcome` — invocacao de tools MCP. | Pendente |
| 5 | `error` | `McpError` tipado. | Pendente |
| 6 | `protocol` | `McpRequest`, `McpResponse`, `McpTool` — JSON-RPC 2.0. | Pendente |
| 7 | `registry` | `McpRegistry` — catalogo de servidores MCP configurados. | Pendente |
| 8 | `transport_http` | Transport HTTP Streamable. | Pendente |
| 9 | `transport_stdio` | Transport stdio (subprocess, kill-on-drop). | Pendente |
