//! MCP client trait + stdio implementation.

use async_trait::async_trait;

use crate::config::McpServerConfig;
use crate::error::McpError;
use crate::protocol::{McpRequest, McpResponse, McpTool, McpToolCallResult};
use crate::transport_stdio::StdioTransport;

/// Transport-agnostic MCP client.
#[async_trait]
pub trait McpClient: Send + Sync {
    /// Server name (for diagnostics).
    fn name(&self) -> &str;

    /// `tools/list` — discover available tools.
    async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError>;

    /// `tools/call` — invoke a tool by name with arguments.
    async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallResult, McpError>;
}

/// stdio-based MCP client.
#[derive(Debug)]
pub struct McpStdioClient {
    name: String,
    transport: StdioTransport,
    next_id: u64,
}

impl McpStdioClient {
    pub async fn from_config(config: &McpServerConfig) -> Result<Self, McpError> {
        match config {
            McpServerConfig::Stdio {
                name, command, args, env, ..
            } => {
                let transport =
                    StdioTransport::spawn(command, args, env.clone()).await?;
                Ok(Self {
                    name: name.clone(),
                    transport,
                    next_id: 1,
                })
            }
            McpServerConfig::Http { .. } => Err(McpError::InvalidConfig(
                "HTTP transport not implemented in this iteration".to_string(),
            )),
        }
    }

    fn next_request(&mut self, method: &str) -> McpRequest {
        let id = self.next_id;
        self.next_id += 1;
        McpRequest::new(id, method)
    }

    async fn rpc(&mut self, req: McpRequest) -> Result<serde_json::Value, McpError> {
        let resp: McpResponse = self.transport.request(req).await?;
        if let Some(err) = resp.error {
            return Err(McpError::ServerError {
                code: err.code,
                message: err.message,
            });
        }
        resp.result.ok_or(McpError::EmptyResponse)
    }
}

#[async_trait]
impl McpClient for McpStdioClient {
    fn name(&self) -> &str {
        &self.name
    }

    async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let req = self.next_request("tools/list");
        let result = self.rpc(req).await?;
        let tools = result
            .get("tools")
            .cloned()
            .ok_or(McpError::EmptyResponse)?;
        let parsed: Vec<McpTool> = serde_json::from_value(tools)?;
        Ok(parsed)
    }

    async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallResult, McpError> {
        let req = self
            .next_request("tools/call")
            .with_params(serde_json::json!({
                "name": tool_name,
                "arguments": arguments,
            }));
        let result = self.rpc(req).await?;
        let parsed: McpToolCallResult = serde_json::from_value(result)?;
        Ok(parsed)
    }
}

/// Phase 36 (mcp-http-and-discover-flake) — HTTP/Streamable client.
///
/// Uses `HttpTransport` underneath. The `from_config` constructor
/// validates the URL and headers, then derives a per-request HTTP
/// timeout from the server's `timeout_ms` field (falls back to 30s
/// when unset — covers slow remote MCP servers without inflating
/// fast/local latencies).
#[derive(Debug)]
pub struct McpHttpClient {
    name: String,
    transport: crate::transport_http::HttpTransport,
    next_id: u64,
}

impl McpHttpClient {
    pub fn from_config(config: &McpServerConfig) -> Result<Self, McpError> {
        match config {
            McpServerConfig::Http {
                name, url, headers, timeout_ms,
            } => {
                let req_timeout = timeout_ms
                    .map(std::time::Duration::from_millis)
                    .unwrap_or_else(|| std::time::Duration::from_secs(30));
                let transport = crate::transport_http::HttpTransport::new(
                    url.clone(),
                    headers.clone(),
                    req_timeout,
                )?;
                Ok(Self {
                    name: name.clone(),
                    transport,
                    next_id: 1,
                })
            }
            McpServerConfig::Stdio { .. } => Err(McpError::InvalidConfig(
                "McpHttpClient requires Http config; got Stdio".to_string(),
            )),
        }
    }

    fn next_request(&mut self, method: &str) -> McpRequest {
        let id = self.next_id;
        self.next_id += 1;
        McpRequest::new(id, method)
    }

    async fn rpc(&mut self, req: McpRequest) -> Result<serde_json::Value, McpError> {
        let resp: McpResponse = self.transport.request(req).await?;
        if let Some(err) = resp.error {
            return Err(McpError::ServerError {
                code: err.code,
                message: err.message,
            });
        }
        resp.result.ok_or(McpError::EmptyResponse)
    }
}

#[async_trait]
impl McpClient for McpHttpClient {
    fn name(&self) -> &str {
        &self.name
    }

    async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let req = self.next_request("tools/list");
        let result = self.rpc(req).await?;
        let tools = result.get("tools").cloned().ok_or(McpError::EmptyResponse)?;
        let parsed: Vec<McpTool> = serde_json::from_value(tools)?;
        Ok(parsed)
    }

    async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallResult, McpError> {
        let req = self
            .next_request("tools/call")
            .with_params(serde_json::json!({
                "name": tool_name,
                "arguments": arguments,
            }));
        let result = self.rpc(req).await?;
        let parsed: McpToolCallResult = serde_json::from_value(result)?;
        Ok(parsed)
    }
}

/// Phase 36 (mcp-http-and-discover-flake) — transport-agnostic enum
/// dispatcher used by `discover_one`, `McpDispatcher::dispatch`, and
/// any other call-site that needs to spawn a client without caring
/// which transport the registry config uses.
#[derive(Debug)]
pub enum McpAnyClient {
    Stdio(McpStdioClient),
    Http(McpHttpClient),
}

impl McpAnyClient {
    pub async fn from_config(cfg: &McpServerConfig) -> Result<Self, McpError> {
        match cfg {
            McpServerConfig::Stdio { .. } => {
                Ok(Self::Stdio(McpStdioClient::from_config(cfg).await?))
            }
            McpServerConfig::Http { .. } => Ok(Self::Http(McpHttpClient::from_config(cfg)?)),
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
        match self {
            Self::Stdio(c) => c.list_tools().await,
            Self::Http(c) => c.list_tools().await,
        }
    }

    async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallResult, McpError> {
        match self {
            Self::Stdio(c) => c.call_tool(tool_name, arguments).await,
            Self::Http(c) => c.call_tool(tool_name, arguments).await,
        }
    }
}

/// Prefix MCP tool names with `mcp:<server>:` to avoid collisions with
/// native tools (per agents-plan.md decision).
pub fn mcp_tool_name(server: &str, tool: &str) -> String {
    format!("mcp:{}:{}", server, tool)
}

/// Inverse of `mcp_tool_name`. Returns `None` if the name is not in the
/// MCP namespace.
pub fn parse_mcp_tool_name(qualified: &str) -> Option<(&str, &str)> {
    let rest = qualified.strip_prefix("mcp:")?;
    rest.split_once(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_tool_name_prefixes_correctly() {
        assert_eq!(mcp_tool_name("github", "search"), "mcp:github:search");
    }

    #[test]
    fn parse_mcp_tool_name_extracts_server_and_tool() {
        let (server, tool) = parse_mcp_tool_name("mcp:github:search").unwrap();
        assert_eq!(server, "github");
        assert_eq!(tool, "search");
    }

    #[test]
    fn parse_mcp_tool_name_returns_none_for_native_tools() {
        assert!(parse_mcp_tool_name("read").is_none());
        assert!(parse_mcp_tool_name("bash").is_none());
    }

    #[test]
    fn parse_mcp_tool_name_returns_none_for_malformed() {
        assert!(parse_mcp_tool_name("mcp:noserver").is_none());
        assert!(parse_mcp_tool_name("mcp:").is_none());
    }

    #[tokio::test]
    async fn http_config_returns_invalid_config_error() {
        let cfg = McpServerConfig::Http {
            name: "x".into(),
            url: "http://localhost".into(),
            headers: Default::default(),
            timeout_ms: None,
        };
        let err = McpStdioClient::from_config(&cfg).await.unwrap_err();
        assert!(matches!(err, McpError::InvalidConfig(_)));
    }

    // ── Phase 36 (mcp-http-and-discover-flake) — McpHttpClient + AnyClient ──

    pub mod http {
        use super::*;
        use std::collections::BTreeMap;

        fn http_cfg(name: &str, url: &str) -> McpServerConfig {
            McpServerConfig::Http {
                name: name.into(),
                url: url.into(),
                headers: BTreeMap::new(),
                timeout_ms: None,
            }
        }

        fn stdio_cfg(name: &str) -> McpServerConfig {
            McpServerConfig::Stdio {
                name: name.into(),
                command: "echo".into(),
                args: vec![],
                env: BTreeMap::new(),
                timeout_ms: None,
            }
        }

        #[test]
        fn http_client_from_config_accepts_http_variant() {
            let c = McpHttpClient::from_config(&http_cfg("x", "http://x"));
            assert!(c.is_ok());
        }

        #[test]
        fn http_client_from_config_rejects_stdio_variant() {
            let err = McpHttpClient::from_config(&stdio_cfg("x"))
                .err()
                .expect("Stdio config must be rejected by McpHttpClient");
            match err {
                McpError::InvalidConfig(msg) => {
                    assert!(msg.contains("McpHttpClient requires Http"))
                }
                _ => panic!("expected InvalidConfig"),
            }
        }

        #[test]
        fn http_client_from_config_propagates_invalid_header_error() {
            let mut headers = BTreeMap::new();
            headers.insert("X-Bad".into(), "bad\nvalue".into());
            let cfg = McpServerConfig::Http {
                name: "x".into(),
                url: "http://x".into(),
                headers,
                timeout_ms: None,
            };
            let err = McpHttpClient::from_config(&cfg)
                .err()
                .expect("invalid header value must surface");
            assert!(matches!(err, McpError::InvalidConfig(_)));
        }
    }

    pub mod any_client {
        use super::*;
        use std::collections::BTreeMap;

        #[tokio::test]
        async fn any_client_from_config_routes_stdio_to_stdio_variant() {
            // Use /bin/cat: spawnable but doesn't return a JSON-RPC reply.
            // We only assert the constructor returns the Stdio variant.
            let cfg = McpServerConfig::Stdio {
                name: "x".into(),
                command: "cat".into(),
                args: vec![],
                env: BTreeMap::new(),
                timeout_ms: None,
            };
            let c = McpAnyClient::from_config(&cfg).await;
            match c {
                Ok(McpAnyClient::Stdio(_)) => (),
                Ok(McpAnyClient::Http(_)) => panic!("Http variant for Stdio config"),
                Err(_) => {
                    // /bin/cat may not exist on some envs — accept skip,
                    // but still verify the routing logic via the Http path.
                }
            }
        }

        #[tokio::test]
        async fn any_client_from_config_routes_http_to_http_variant() {
            let cfg = McpServerConfig::Http {
                name: "x".into(),
                url: "http://localhost:1".into(),
                headers: BTreeMap::new(),
                timeout_ms: None,
            };
            let c = McpAnyClient::from_config(&cfg).await.expect("http ctor ok");
            assert!(
                matches!(c, McpAnyClient::Http(_)),
                "Http config must yield McpAnyClient::Http"
            );
        }

        #[test]
        fn any_client_name_dispatches_through_inner_http() {
            let cfg = McpServerConfig::Http {
                name: "company-mcp".into(),
                url: "http://x".into(),
                headers: BTreeMap::new(),
                timeout_ms: None,
            };
            let inner = McpHttpClient::from_config(&cfg).unwrap();
            let any = McpAnyClient::Http(inner);
            assert_eq!(any.name(), "company-mcp");
        }
    }
}
