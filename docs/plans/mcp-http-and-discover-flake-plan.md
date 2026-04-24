# Plano: MCP HTTP Transport + Discover Timeout Flake

> **Versão 1.0** — fecha dois itens declarados como "deferred" no
> `resume-runtime-wiring-plan.md`. Após esta entrega o sistema deixa
> de depender exclusivamente de `stdio` para servers MCP e o pipeline
> de descoberta deixa de cair em flake transiente quando o `npx`
> demora mais que o timeout default.

## Contexto

Estado atual auditado em `crates/theo-infra-mcp/`:

| Componente | Estado | Bug observável |
|---|---|---|
| `McpServerConfig::Http { name, url, headers }` | Variante existe, **serialização e parsing OK** | `McpStdioClient::from_config(Http)` retorna `McpError::InvalidConfig("HTTP transport not implemented")` |
| `transport_stdio::StdioTransport` | Funcional, JSON-RPC line-delimited via `tokio::process` | — |
| `transport_http` (módulo) | **NÃO EXISTE** | Sem path para qualquer `McpServerConfig::Http` |
| `client::McpClient` trait | Bem desenhado, transport-agnostic | Cumprida; basta uma 2ª implementação `McpHttpClient` |
| `discovery::DEFAULT_PER_SERVER_TIMEOUT` | `Duration::from_secs(5)` **hardcoded** | `theo mcp discover` cai com `timed out after 5s` quando `npx` precisa baixar/extrair pacote no primeiro spawn |
| Pré-warm em `sota12-full-stress.sh` | Roda `npx --help` separadamente | `discover` ainda re-spawna o subprocess, e o cold start node ≥ 5s ainda timeout |
| `McpRegistry` per-server config | Sem campo `timeout_ms` | Não há mecanismo de override por servidor |
| CLI `theo mcp discover` | Sem flag `--timeout` | Operador não pode contornar a flake |

Evidência empírica colhida na execução `OAUTH_E2E=1 bash scripts/sota12-full-stress.sh`
do commit `9f976f6`:

```
═══ Phase C — MCP CLI + real server (gaps #1, #6, #8) ═══
Pre-warming npx cache (one-time, may take 30s)...
Running `theo mcp discover`...
  ✗ theo mcp discover succeeded for fs server
    detail: ✗ fs: fs: timed out after 5s
Discover finished: 0 successful, 1 failed.
```

A causa raiz é **dupla**: (1) `npx -y @modelcontextprotocol/server-filesystem`
precisa de >5s no cold start mesmo após pré-warm porque o *package
extract* do `npx` é por-invocação, não cacheado entre processos; (2)
o pipeline não permite override do timeout sem patch em código.

**Objetivo:** zero dependência declarada como "deferred" no MCP layer.
Cada track é atômico, TDD, plan-named tests, validação E2E real.

**Estratégia:** 2 tracks paralelos (A pode rodar enquanto B já estabilizou
a suíte; ambos compartilham 0 arquivos):

| Track | Fases | Entrega | Fecha item |
|---|---|---|---|
| **B — Discover timeout robustness** | 33-34 | Per-server `timeout_ms` no `McpServerConfig` + env override + CLI flag `--timeout-secs` + E2E real do `npx` | "MCP discover npx 5s timeout flake" |
| **A — HTTP/Streamable transport** | 35-39 | `transport_http` module + `McpHttpClient` + Streamable HTTP MCP spec (POST + SSE/JSON resp) + `from_config` aceita `Http` + DiscoveryCache aceita HTTP servers + bearer/header auth | "MCP HTTP transport (só stdio hoje)" |

Track B é menor (~2-3h, 2 fases), Track A é maior (~12-16h, 5 fases).
**Faça B primeiro** — ele desbloqueia a validação E2E do Track A.

---

## Decisões de arquitetura

### D1: Timeout é parte da config do servidor, não global

`McpServerConfig::Stdio` e `McpServerConfig::Http` ganham campo
opcional `#[serde(default)] timeout_ms: Option<u64>`. `discover_one`
prefere o valor do server; cai no `per_server_timeout` da chamada
quando `None`. Isto permite que `npx`-based servers declarem 30s
sem afetar HTTP servers que respondem em <1s.

### D2: Env var é guard-rail global, não substituto da config

`THEO_MCP_DISCOVER_TIMEOUT_SECS` aplica-se ao default geral mas é
**sempre overridable** pela config do servidor (servidor wins) e
pelo flag CLI (operador wins). Hierarquia: CLI > env > config > default 5s.

### D3: HTTP transport implementa o Streamable HTTP spec (2025-03-26)

Não usar SSE legado (deprecated 2024). Streamable HTTP:
- POST JSON-RPC request → response pode ser `application/json` (single)
ou `text/event-stream` (multi-message para tools com streaming output)
- Optional `Mcp-Session-Id` header round-tripped após initialize
- Optional GET para abrir server→client stream — out of scope hoje
(usaremos só request-response, que cobre `tools/list` e `tools/call`)

### D4: Auth HTTP usa headers genéricos, não OAuth manager dedicado

`McpServerConfig::Http { headers }` já existe e suporta `Authorization:
Bearer xyz` declarativo. OAuth 2.1 manager fica como épico separado.
Se o operador quer Bearer rotativo, ele substitui o valor antes do
spawn (ou usa um wrapper).

### D5: D2/D3 do plano anterior continuam válidos

Backward compat absoluta: `McpStdioClient::from_config` continua
funcionando para variantes Stdio. Servers HTTP que existiam apenas no
`McpServerConfig::Http` (bloqueados pelo erro `InvalidConfig`) agora
funcionam — o erro vira sucesso. Nenhum teste existente quebra.

### D6: Reqwest é a dep escolhida (já no workspace)

Workspace `Cargo.toml` já declara
`reqwest = { version = "0.12", features = ["json", "stream", ...] }`.
Não adicionar nova dep. Para SSE parsing usar `eventsource-stream` ou
parser manual (linhas começando com `data: ` separadas por `\n\n`).
Avaliar na Fase 36 — preferir parser manual se a feature couber em
~80 LOC para evitar dep nova.

---

## Track B — Discover Timeout Robustness

### Fase 33 — Per-server timeout config + env override

**Objetivo:** parar de hardcodar 5s. `McpServerConfig` aceita
`timeout_ms` opcional; `THEO_MCP_DISCOVER_TIMEOUT_SECS` ajusta o
default global; `discover_one` resolve a hierarquia.

**Arquitetura:**

```rust
// theo-infra-mcp/src/config.rs (modificar)

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpServerConfig {
    Stdio {
        name: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        /// Phase 33 (mcp-http-and-discover-flake) — per-server discover
        /// timeout override. When `None`, falls back to the global
        /// per_server_timeout passed to `discover_*`. Useful for npx-
        /// based servers that need 30s+ cold start.
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    Http {
        name: String,
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
}

impl McpServerConfig {
    pub fn name(&self) -> &str { ... }
    /// Phase 33: returns the per-server timeout override (if any).
    pub fn timeout_ms(&self) -> Option<u64> {
        match self {
            McpServerConfig::Stdio { timeout_ms, .. } => *timeout_ms,
            McpServerConfig::Http { timeout_ms, .. } => *timeout_ms,
        }
    }
}
```

```rust
// theo-infra-mcp/src/discovery.rs (modificar)

/// Phase 33: env override for the default per-server timeout.
/// Falls back to DEFAULT_PER_SERVER_TIMEOUT (5s) when unset/invalid.
pub fn effective_default_timeout() -> Duration {
    std::env::var("THEO_MCP_DISCOVER_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_PER_SERVER_TIMEOUT)
}

async fn discover_one(
    name: &str,
    cfg: &crate::config::McpServerConfig,
    per_server_timeout: Duration,
) -> Result<Vec<McpTool>, String> {
    // Phase 33: per-server override wins; otherwise use the caller's
    // value (which itself respects effective_default_timeout when
    // discover_filtered/discover_all is invoked from the CLI).
    let effective = cfg
        .timeout_ms()
        .map(Duration::from_millis)
        .unwrap_or(per_server_timeout);
    let work = async move { ... };
    match timeout(effective, work).await {
        ...
        Err(_) => Err(format!("{}: timed out after {}s", name, effective.as_secs())),
    }
}
```

```rust
// apps/theo-cli/src/mcp_admin.rs (modificar parse_registry_toml)

#[derive(Deserialize)]
struct RawServer {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    timeout_ms: Option<u64>,  // ← novo
}
// reg.register(McpServerConfig::Stdio { ..., timeout_ms: raw.timeout_ms });
```

**TDD Sequence:**
```
RED:
  config_stdio_timeout_ms_defaults_to_none
  config_stdio_timeout_ms_round_trips_via_json
  config_http_timeout_ms_defaults_to_none
  config_http_timeout_ms_round_trips_via_json
  effective_default_timeout_returns_default_when_env_unset
  effective_default_timeout_returns_env_value_when_set
  effective_default_timeout_falls_back_when_env_unparseable
  discover_one_uses_per_server_timeout_when_present
  discover_one_uses_caller_timeout_when_per_server_none
  discover_one_per_server_overrides_caller_even_when_smaller
  parse_registry_toml_reads_timeout_ms_field

GREEN:
  - Add timeout_ms field to McpServerConfig variants
  - effective_default_timeout() helper reading THEO_MCP_DISCOVER_TIMEOUT_SECS
  - discover_one resolves Cfg-override-or-caller-default
  - mcp_admin RawServer struct gains timeout_ms

INTEGRATION:
  - Add timeout_ms = 30000 to fixture in sota12-full-stress.sh
    .theo/mcp.toml fs server entry
  - Re-run OAUTH_E2E=1 bash scripts/sota12-full-stress.sh
  - Verify Phase C now reports `✓ theo mcp discover succeeded for fs server`
```

**Verify:**
```bash
cargo test -p theo-infra-mcp -- config::tests::timeout
cargo test -p theo-infra-mcp -- discovery::tests::effective_default_timeout
cargo test -p theo-infra-mcp -- discovery::tests::per_server_timeout
cargo test -p theo --bin theo
THEO_MCP_DISCOVER_TIMEOUT_SECS=30 OAUTH_E2E=1 bash scripts/sota12-full-stress.sh
```

**Risco mitigado (D5):** novos campos são `#[serde(default)]` — TOML/JSON
já gravados sem `timeout_ms` continuam válidos. Função
`effective_default_timeout` é pura.

---

### Fase 34 — CLI flag `--timeout-secs` + script update

**Objetivo:** operador pode forçar timeout sem mexer em config nem
env. `theo mcp discover --timeout-secs 30` é o caminho de menor
fricção.

**Arquitetura:**

```rust
// apps/theo-cli/src/mcp_admin.rs (modificar McpCmd::Discover)

#[derive(Subcommand)]
pub enum McpCmd {
    Discover {
        server: Option<String>,
        /// Phase 34: per-call override of the discover timeout.
        /// Takes precedence over THEO_MCP_DISCOVER_TIMEOUT_SECS and
        /// per-server timeout_ms in mcp.toml. Useful in CI when the
        /// operator knows npx needs 30s on a cold runner.
        #[arg(long)]
        timeout_secs: Option<u64>,
    },
    ...
}

// In handle_mcp(McpCmd::Discover):
let global_timeout = timeout_secs
    .map(Duration::from_secs)
    .unwrap_or_else(theo_infra_mcp::effective_default_timeout);
```

```bash
# scripts/sota12-full-stress.sh (modificar Phase C)

DISCOVER_OUT=$("$CLI" mcp discover --repo "$WORK" --timeout-secs 30 2>&1)
```

**TDD Sequence:**
```
RED:
  mcp_discover_command_accepts_timeout_secs_flag
  mcp_discover_uses_flag_value_when_provided
  mcp_discover_falls_back_to_env_when_flag_omitted
  mcp_discover_falls_back_to_default_when_neither_set

GREEN:
  - Add #[arg(long)] timeout_secs: Option<u64> to Discover variant
  - Resolve flag → env → default in handle_mcp
  - Pass through to cache.discover_filtered

INTEGRATION:
  - Update sota12-full-stress.sh Phase C to use --timeout-secs 30
  - Re-run E2E script and confirm Phase C passes 26/26
```

**Verify:**
```bash
cargo test -p theo --bin theo -- mcp_admin::tests::timeout
target/release/theo mcp discover --help | grep -- --timeout-secs
OAUTH_E2E=1 bash scripts/sota12-full-stress.sh
# expected: 26 PASS / 0 FAIL
```

**Risco mitigado:** flag é opcional; comando sem flag preserva
comportamento antigo (cai no env / default).

---

## Track A — HTTP/Streamable Transport

### Fase 35 — Module skeleton + reqwest dep + protocol parser

**Objetivo:** criar `transport_http.rs` e `McpHttpClient` esqueleto
com parsing dos dois formatos de resposta (`application/json` single
e `text/event-stream` multi). Sem RPC ainda — só types + parser.

**Arquitetura:**

```rust
// crates/theo-infra-mcp/Cargo.toml (modificar)

[dependencies]
...
reqwest = { workspace = true }
futures-util = { workspace = true }   # for stream extension trait
```

```rust
// crates/theo-infra-mcp/src/transport_http.rs (NOVO)

//! HTTP/Streamable transport per MCP spec 2025-03-26 §Transports.
//!
//! Single endpoint:
//!   - POST {url} with Content-Type: application/json + JSON-RPC body
//!   - Response is either:
//!       application/json    → single McpResponse
//!       text/event-stream   → SSE stream; first `data:` line carrying
//!                             a response with matching id is returned
//!   - Optional `Mcp-Session-Id` header round-tripped after initialize
//!   - Auth via static headers in McpServerConfig::Http { headers }

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};

use crate::error::McpError;
use crate::protocol::{McpRequest, McpResponse};

#[derive(Debug)]
pub struct HttpTransport {
    url: String,
    client: reqwest::Client,
    extra_headers: HeaderMap,
    session_id: std::sync::Mutex<Option<String>>,
}

impl HttpTransport {
    pub fn new(
        url: impl Into<String>,
        headers: BTreeMap<String, String>,
        request_timeout: Duration,
    ) -> Result<Self, McpError> {
        let client = reqwest::Client::builder()
            .timeout(request_timeout)
            .build()
            .map_err(|e| McpError::InvalidConfig(format!("reqwest build: {e}")))?;
        let mut hm = HeaderMap::new();
        for (k, v) in headers {
            let name = HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| McpError::InvalidConfig(format!("bad header {k}: {e}")))?;
            let val = HeaderValue::from_str(&v)
                .map_err(|e| McpError::InvalidConfig(format!("bad value for {k}: {e}")))?;
            hm.insert(name, val);
        }
        Ok(Self {
            url: url.into(),
            client,
            extra_headers: hm,
            session_id: std::sync::Mutex::new(None),
        })
    }

    pub async fn request(&self, req: McpRequest) -> Result<McpResponse, McpError> {
        let body = serde_json::to_vec(&req)?;
        let mut builder = self
            .client
            .post(&self.url)
            .header(CONTENT_TYPE, "application/json")
            .header("Accept", "application/json, text/event-stream");
        for (k, v) in self.extra_headers.iter() {
            builder = builder.header(k, v);
        }
        if let Ok(g) = self.session_id.lock()
            && let Some(s) = g.as_ref() {
            builder = builder.header("Mcp-Session-Id", s.clone());
        }
        let resp = builder.body(body).send().await
            .map_err(|e| McpError::Io(std::io::Error::other(format!("http: {e}"))))?;

        // Capture Mcp-Session-Id (first time, after initialize)
        if let Some(s) = resp.headers().get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
            .map(String::from)
            && let Ok(mut g) = self.session_id.lock() {
            *g = Some(s);
        }

        let ct = resp.headers().get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
        if ct.contains("text/event-stream") {
            parse_sse_until_id_match(resp, &req.id).await
        } else {
            let bytes = resp.bytes().await
                .map_err(|e| McpError::Io(std::io::Error::other(format!("body: {e}"))))?;
            let parsed: McpResponse = serde_json::from_slice(&bytes)?;
            Ok(parsed)
        }
    }
}

async fn parse_sse_until_id_match(
    resp: reqwest::Response,
    req_id: &serde_json::Value,
) -> Result<McpResponse, McpError> {
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk
            .map_err(|e| McpError::Io(std::io::Error::other(format!("sse chunk: {e}"))))?;
        buffer.push_str(std::str::from_utf8(&bytes)
            .map_err(|e| McpError::Io(std::io::Error::other(format!("utf8: {e}"))))?);
        // SSE events terminate on \n\n
        while let Some(end) = buffer.find("\n\n") {
            let event = buffer[..end].to_string();
            buffer.drain(..end + 2);
            // Concatenate all `data:` lines; ignore comments and other fields.
            let payload: String = event
                .lines()
                .filter_map(|l| l.strip_prefix("data:").map(|s| s.trim_start()))
                .collect::<Vec<_>>()
                .join("\n");
            if payload.is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&payload)?;
            // Skip notifications (no id) and mismatched ids
            if v.get("id").is_none() || v.get("id") == Some(&serde_json::Value::Null) {
                continue;
            }
            let resp: McpResponse = serde_json::from_value(v)?;
            if &resp.id == req_id {
                return Ok(resp);
            }
        }
    }
    Err(McpError::TransportClosed)
}
```

**TDD Sequence:**
```
RED:
  http_transport_new_validates_url_format
  http_transport_new_rejects_invalid_header_name
  http_transport_new_rejects_invalid_header_value
  parse_sse_until_id_match_returns_first_matching_id
  parse_sse_until_id_match_skips_notifications
  parse_sse_until_id_match_skips_mismatched_ids
  parse_sse_until_id_match_handles_multi_line_data_field
  parse_sse_until_id_match_returns_transport_closed_on_eof
  http_transport_request_against_mock_returns_json_response
  http_transport_request_against_mock_returns_sse_response
  http_transport_request_propagates_session_id_header
  http_transport_request_includes_extra_headers_in_request

GREEN:
  - Create transport_http.rs as above
  - Add reqwest + futures-util to theo-infra-mcp deps
  - parse_sse_until_id_match standalone (testable without reqwest mock)
  - Use a tiny mock server (tokio::net::TcpListener) for end-to-end tests

INTEGRATION:
  - Tests use mock HTTP server bound to ephemeral port
  - No external network — mock returns fixtures from a Vec<String>
```

**Verify:**
```bash
cargo test -p theo-infra-mcp -- transport_http
cargo build -p theo-infra-mcp
```

**Risco mitigado:** Fase 35 NÃO altera `McpStdioClient` nem
`from_config`. Build inteiro permanece verde.

---

### Fase 36 — `McpHttpClient` impl + `from_config(Http)` rota

**Objetivo:** segunda implementação de `McpClient` trait, e
`McpStdioClient::from_config` continua só para Stdio (renomear path
seria breaking — em vez disso adicionar `McpHttpClient::from_config`
e tornar a chamada call-site decidir). Alternativa: novo enum dispatcher.

**Decisão:** seguir o enum dispatcher pattern para minimizar mudanças
em call-sites.

```rust
// crates/theo-infra-mcp/src/client.rs (extender)

/// Phase 36 (mcp-http-and-discover-flake) — HTTP client.
#[derive(Debug)]
pub struct McpHttpClient {
    name: String,
    transport: crate::transport_http::HttpTransport,
    next_id: u64,
}

impl McpHttpClient {
    pub fn from_config(config: &McpServerConfig) -> Result<Self, McpError> {
        match config {
            McpServerConfig::Http { name, url, headers, timeout_ms } => {
                let req_timeout = timeout_ms
                    .map(Duration::from_millis)
                    .unwrap_or(Duration::from_secs(30));  // request-level, not discover
                let transport = crate::transport_http::HttpTransport::new(
                    url, headers.clone(), req_timeout,
                )?;
                Ok(Self { name: name.clone(), transport, next_id: 1 })
            }
            McpServerConfig::Stdio { .. } => Err(McpError::InvalidConfig(
                "McpHttpClient requires Http config; got Stdio".into(),
            )),
        }
    }
}

#[async_trait]
impl McpClient for McpHttpClient {
    fn name(&self) -> &str { &self.name }
    async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> { ... }
    async fn call_tool(...) -> Result<McpToolCallResult, McpError> { ... }
}

/// Phase 36: enum dispatcher used by discover_one/dispatch and any
/// other call-site that needs transport-agnostic spawn.
pub enum McpAnyClient {
    Stdio(McpStdioClient),
    Http(McpHttpClient),
}

impl McpAnyClient {
    pub async fn from_config(cfg: &McpServerConfig) -> Result<Self, McpError> {
        match cfg {
            McpServerConfig::Stdio { .. } =>
                Ok(Self::Stdio(McpStdioClient::from_config(cfg).await?)),
            McpServerConfig::Http { .. } =>
                Ok(Self::Http(McpHttpClient::from_config(cfg)?)),
        }
    }
}

#[async_trait]
impl McpClient for McpAnyClient {
    fn name(&self) -> &str {
        match self {
            Self::Stdio(c) => c.name(),
            Self::Http(c) => c.name(),
        }
    }
    async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        match self { Self::Stdio(c) => c.list_tools().await, Self::Http(c) => c.list_tools().await }
    }
    async fn call_tool(...) -> ... { ... }
}
```

**TDD Sequence:**
```
RED:
  http_client_from_config_rejects_stdio_variant
  http_client_from_config_accepts_http_variant
  http_client_list_tools_against_mock_returns_tools
  http_client_call_tool_against_mock_returns_result
  any_client_from_config_routes_stdio_to_stdio
  any_client_from_config_routes_http_to_http
  any_client_dispatches_list_tools_through_inner

GREEN:
  - Add McpHttpClient impl
  - Add McpAnyClient enum dispatcher
  - Update lib.rs pub use exports

INTEGRATION:
  - Mock HTTP server fixture in tests/ for tools/list + tools/call
  - No new external deps beyond Phase 35
```

**Verify:**
```bash
cargo test -p theo-infra-mcp -- client::tests::http
cargo test -p theo-infra-mcp -- client::tests::any_client
```

---

### Fase 37 — `discover_one` usa `McpAnyClient`

**Objetivo:** o pipeline de descoberta deixa de hardcodar
`McpStdioClient` — passa a usar `McpAnyClient::from_config` que
roteia automaticamente. Servers HTTP no registro agora são
descobertos de verdade.

**Arquitetura:**

```rust
// crates/theo-infra-mcp/src/discovery.rs (modificar)

use crate::client::{McpAnyClient, McpClient};

async fn discover_one(
    name: &str,
    cfg: &crate::config::McpServerConfig,
    per_server_timeout: Duration,
) -> Result<Vec<McpTool>, String> {
    let effective = cfg.timeout_ms()
        .map(Duration::from_millis)
        .unwrap_or(per_server_timeout);
    let work = async move {
        let mut client = McpAnyClient::from_config(cfg).await?;
        client.list_tools().await
    };
    match timeout(effective, work).await { ... }
}
```

**TDD Sequence:**
```
RED:
  discover_one_routes_stdio_through_any_client
  discover_one_routes_http_through_any_client
  discover_filtered_caches_http_server_tools_after_success
  discover_filtered_records_http_failure_with_explicit_message

GREEN:
  - Replace McpStdioClient::from_config call with McpAnyClient::from_config
  - Update doc comments to reflect dual-transport support

INTEGRATION:
  - Mock HTTP server fixture serves tools/list
  - Add Http variant to one of the existing discover tests
```

**Verify:**
```bash
cargo test -p theo-infra-mcp -- discovery::tests::http
cargo test -p theo-infra-mcp --lib   # full regression
```

---

### Fase 38 — `McpDispatcher::dispatch` aceita HTTP

**Objetivo:** `dispatch.rs` é o caminho de runtime que sub-agents
chamam quando emitem `mcp:<server>:<tool>`. Hoje ele só constrói
`McpStdioClient`. Mesma migração: `McpAnyClient`.

**Arquitetura:** análoga à Fase 37, em `crates/theo-infra-mcp/src/dispatch.rs`.

**TDD Sequence:**
```
RED:
  dispatcher_dispatches_to_http_server_when_config_is_http
  dispatcher_returns_invalid_config_error_when_server_unknown (regression guard)
  dispatcher_uses_per_server_timeout_for_http_call

GREEN:
  - Replace McpStdioClient with McpAnyClient in dispatch path
  - Per-server timeout honored for call_tool too (not just list_tools)

INTEGRATION:
  - Mock HTTP server returns tools/call result
  - Smoke: real npx-based HTTP server (or filesystem MCP if it exposes HTTP)
    skipped if not available
```

**Verify:**
```bash
cargo test -p theo-infra-mcp -- dispatch::tests::http
cargo test -p theo-agent-runtime --lib   # full regression
cargo build --workspace   # except theo-desktop (gobject pre-existing)
```

---

### Fase 39 — `mcp_admin` aceita HTTP servers no `mcp.toml`

**Objetivo:** o operador pode declarar
```toml
[[server]]
transport = "http"
name = "company-internal"
url = "https://mcp.example.com/api"
headers = { Authorization = "Bearer ..." }
```
em `.theo/mcp.toml`. Hoje o parser só conhece Stdio.

**Arquitetura:**

```rust
// apps/theo-cli/src/mcp_admin.rs (modificar parse_registry_toml)

#[derive(Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
enum RawServer {
    #[serde(alias = "stdio")]
    Stdio {
        name: String,
        command: String,
        #[serde(default)] args: Vec<String>,
        #[serde(default)] env: BTreeMap<String, String>,
        #[serde(default)] timeout_ms: Option<u64>,
    },
    #[serde(alias = "http")]
    Http {
        name: String,
        url: String,
        #[serde(default)] headers: BTreeMap<String, String>,
        #[serde(default)] timeout_ms: Option<u64>,
    },
}

// Default — when `transport` is absent, infer Stdio for backward compat.
// Achieved with a custom Deserialize impl OR by post-processing the raw
// value (less elegant but safer for #[serde(default)] interactions).
```

**Backward compat:** `mcp.toml` files without `transport = "stdio"`
field continue to work because we keep the existing `RawServer`
struct AS the Stdio default branch via custom deserializer.

**TDD Sequence:**
```
RED:
  parse_registry_toml_reads_http_server_with_explicit_transport
  parse_registry_toml_defaults_missing_transport_to_stdio (backcompat)
  parse_registry_toml_reads_http_server_headers
  parse_registry_toml_reads_http_server_timeout_ms
  parse_registry_toml_emits_clear_error_on_unknown_transport
  parse_registry_toml_reads_mixed_stdio_and_http_servers

GREEN:
  - Replace RawServer with enum variant struct
  - Custom Deserialize / two-pass parse to default to Stdio when
    transport field absent (maintain D5 backward compat)
  - Update register loop to construct correct McpServerConfig variant

INTEGRATION:
  - Add fixture .theo/mcp.toml with mixed transports
  - sota12-full-stress.sh Phase C accepts HTTP server (skip if no
    real HTTP MCP server available; gate via SKIP_HTTP=1)
```

**Verify:**
```bash
cargo test -p theo --bin theo -- mcp_admin::tests::parse
target/release/theo mcp discover --repo /tmp/http-fixture --timeout-secs 30
```

---

## Riscos e mitigações

| Risco | Mitigação |
|---|---|
| `reqwest` adiciona TLS deps pesadas a `theo-infra-mcp` | Já usa `rustls-tls` no workspace; sem novo overhead vs OpenSSL. Build size +2MB aceitável. |
| Mock HTTP server flake em CI | Bind em porta efêmera via `TcpListener::bind("127.0.0.1:0")` e `local_addr()`; um teste por vez (sem race). |
| SSE parser manual quebra em CRLF servers | Aceitar tanto `\r\n\r\n` quanto `\n\n` como event terminator; teste explícito. |
| HTTP server retorna `application/json; charset=utf-8` (Content-Type com parâmetros) | `ct.contains("text/event-stream")` é tolerante; `to_lowercase()` normaliza. |
| Header com bytes inválidos do operador | `HeaderName::from_bytes` retorna `Err`; surface como `InvalidConfig` — não panic. |
| Mcp-Session-Id pode ser per-connection | Para esta entrega armazenamos no `Mutex<Option<String>>` da `HttpTransport` — vida do client. Suficiente para `tools/list` + `tools/call`. |
| TOML `transport = "stdio"` quebra arquivos antigos | Custom Deserialize trata variante sem campo como Stdio (D5). 2 testes explícitos. |
| Per-server `timeout_ms` no `mcp.toml` muda hash de cache de discovery | Cache é por nome de servidor, não por config hash. Sem impacto. |
| `THEO_MCP_DISCOVER_TIMEOUT_SECS=0` causa timeout instantâneo | Validar `> 0` em `effective_default_timeout`; cair no default se 0/inválido. |

---

## Verificação final agregada

```bash
# Track B
cargo test -p theo-infra-mcp -- config::tests::timeout
cargo test -p theo-infra-mcp -- discovery::tests::effective_default_timeout
cargo test -p theo-infra-mcp -- discovery::tests::per_server_timeout
cargo test -p theo --bin theo -- mcp_admin::tests::timeout

# Track A
cargo test -p theo-infra-mcp -- transport_http
cargo test -p theo-infra-mcp -- client::tests::http
cargo test -p theo-infra-mcp -- client::tests::any_client
cargo test -p theo-infra-mcp -- discovery::tests::http
cargo test -p theo-infra-mcp -- dispatch::tests::http
cargo test -p theo --bin theo -- mcp_admin::tests::parse

# Regression sweep
cargo test -p theo-infra-mcp --lib --tests
cargo test -p theo-agent-runtime --lib --tests
cargo build -p theo-agent-runtime -p theo-isolation -p theo

# E2E real (gates)
THEO_MCP_DISCOVER_TIMEOUT_SECS=30 OAUTH_E2E=1 bash scripts/sota12-full-stress.sh
# expected: 26 PASS / 0 FAIL (Phase C agora aprova com timeout estendido)

# Optional HTTP E2E (when reachable HTTP MCP server available):
THEO_MCP_HTTP_TEST_URL=https://your-server bash scripts/mcp-http-smoke.sh
```

---

## Cronograma

```
Sprint A (Track B — fix flake): ~3-4h
  Fase 33 → Fase 34 sequenciais
  Ganho imediato: Phase C do stress test passa

Sprint B (Track A — HTTP transport): ~12-16h
  Fase 35 (skeleton + parser)        ~3-4h
  Fase 36 (HttpClient + AnyClient)   ~3-4h
  Fase 37 (discovery rota)           ~1-2h
  Fase 38 (dispatcher rota)          ~2-3h
  Fase 39 (mcp_admin TOML)           ~3h

Total: 15-20h se feito sequencial; ~12h em paralelo (B em background)
```

---

## Compromisso de cobertura final

Após este plano: **0 itens "deferred" no MCP layer**.

| Item | Status pós-plano |
|---|---|
| MCP HTTP transport | ✓ Streamable HTTP (POST + SSE/JSON) ativo, dispatch + discovery + admin |
| MCP discover npx 5s timeout flake | ✓ Per-server `timeout_ms` + env var + CLI flag; CI gate via `--timeout-secs 30` |

Plus:
- 30+ novos testes (TDD obrigatório por fase)
- Backward compat absoluta (D5)
- Real OAuth Codex stress passa 26/26 com `THEO_MCP_DISCOVER_TIMEOUT_SECS=30`
- HTTP transport validado contra mock + (opcionalmente) servidor HTTP real

---

## Trabalho fora deste plano

Confirmados como épicos separados, **NÃO** parte deste escopo:
- OAuth 2.1 manager para MCP HTTP (refresh tokens, dynamic client reg) — headers estáticos hoje
- Resources protocol (`resources/list`, `resources/read`) — só tools hoje
- Server→client GET stream (notifications) — só request-response
- WebSocket transport (deprecated pelo spec atual)
- A2A protocol entre sub-agents

---

## Referências

- MCP Spec 2025-03-26 — https://modelcontextprotocol.io/specification
- `crates/theo-infra-mcp/src/lib.rs` — surface atual
- `docs/plans/resume-runtime-wiring-plan.md` — plano antecedente (closes gaps #3/#10)
- TDD: RED → GREEN → REFACTOR (sem exceções)
- Evidência da flake: `9f976f6` `OAUTH_E2E=1 bash scripts/sota12-full-stress.sh` Phase C
