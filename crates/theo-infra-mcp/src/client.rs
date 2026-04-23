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
                name, command, args, env,
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
        };
        let err = McpStdioClient::from_config(&cfg).await.unwrap_err();
        assert!(matches!(err, McpError::InvalidConfig(_)));
    }
}
