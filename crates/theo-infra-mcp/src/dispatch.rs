//! `McpDispatcher` — bridge between tool names of the form
//! `mcp:<server>:<tool>` and `McpClient::call_tool`.
//!
//! Used by the agent runtime when it sees a tool call whose name starts
//! with `mcp:` — `dispatch()` parses the qualified name, spawns (or
//! reuses) a client, calls the tool, and returns the response as text.

use std::sync::Arc;

use crate::client::{parse_mcp_tool_name, McpAnyClient, McpClient};
use crate::error::McpError;
use crate::protocol::{McpContentPart, McpToolCallResult};
use crate::registry::McpRegistry;

/// Result of a dispatch attempt.
#[derive(Debug)]
pub struct DispatchOutcome {
    /// Concatenated text content from the MCP server response.
    pub text: String,
    /// True if the server marked the call as an error.
    pub is_error: bool,
}

/// Async dispatcher that resolves `mcp:<server>:<tool>` invocations
/// against a `McpRegistry` and a (server-name → live client) cache.
///
/// Phase 38 (mcp-http-and-discover-flake): every dispatch spawns a
/// transient `McpAnyClient` (transport-agnostic) — both stdio and
/// HTTP servers in the registry are honored. A connection pool can be
/// added later — the type signature already supports it (Arc<Self>).
#[derive(Debug)]
pub struct McpDispatcher {
    registry: Arc<McpRegistry>,
}

impl McpDispatcher {
    pub fn new(registry: Arc<McpRegistry>) -> Self {
        Self { registry }
    }

    /// True if `name` is in the MCP namespace (`mcp:server:tool`).
    pub fn handles(name: &str) -> bool {
        parse_mcp_tool_name(name).is_some()
    }

    /// Parse + dispatch. Returns `Err` for: malformed name, unknown
    /// server, transport failure, or RPC error.
    pub async fn dispatch(
        &self,
        qualified_name: &str,
        args: serde_json::Value,
    ) -> Result<DispatchOutcome, McpError> {
        let (server, tool) = parse_mcp_tool_name(qualified_name).ok_or_else(|| {
            McpError::InvalidConfig(format!(
                "tool name '{}' is not in the mcp:<server>:<tool> namespace",
                qualified_name
            ))
        })?;

        let server_owned = server.to_string();
        let tool_owned = tool.to_string();

        let cfg = self.registry.get(&server_owned).ok_or_else(|| {
            McpError::InvalidConfig(format!("unknown MCP server: '{}'", server_owned))
        })?;

        // Phase 38: transport-agnostic spawn. Routes Stdio→McpStdioClient,
        // Http→McpHttpClient via McpAnyClient::from_config.
        let mut client = McpAnyClient::from_config(&cfg).await?;
        let result = client.call_tool(&tool_owned, args).await?;
        Ok(format_outcome(result))
    }
}

/// Concatenate all text parts; report error flag.
fn format_outcome(result: McpToolCallResult) -> DispatchOutcome {
    let mut text = String::new();
    for (i, part) in result.content.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        match part {
            McpContentPart::Text { text: t } => text.push_str(t),
            McpContentPart::Image { mime_type, .. } => {
                text.push_str(&format!("[image: {}]", mime_type));
            }
            McpContentPart::Resource { resource } => {
                text.push_str(&format!("[resource: {}]", resource));
            }
        }
    }
    DispatchOutcome {
        text,
        is_error: result.is_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpServerConfig;
    use std::collections::BTreeMap;

    #[test]
    fn handles_recognizes_mcp_prefix() {
        assert!(McpDispatcher::handles("mcp:github:search"));
        assert!(!McpDispatcher::handles("read"));
        assert!(!McpDispatcher::handles("bash"));
        assert!(!McpDispatcher::handles("mcp:onlyserver"));
    }

    #[tokio::test]
    async fn dispatch_unknown_server_returns_invalid_config() {
        let reg = Arc::new(McpRegistry::new());
        let d = McpDispatcher::new(reg);
        let err = d
            .dispatch("mcp:nonexistent:search", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::InvalidConfig(_)));
    }

    #[tokio::test]
    async fn dispatch_malformed_name_returns_invalid_config() {
        let reg = Arc::new(McpRegistry::new());
        let d = McpDispatcher::new(reg);
        let err = d
            .dispatch("not-a-mcp-name", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::InvalidConfig(_)));
    }

    #[tokio::test]
    async fn dispatch_known_server_attempts_call() {
        // Register a stdio config with a fake command so spawn errors at
        // transport (not at registry lookup). Validates the routing path.
        let mut reg = McpRegistry::new();
        reg.register(McpServerConfig::Stdio {
            name: "fake".to_string(),
            command: "/nonexistent/command/xyz".to_string(),
            args: vec![],
            env: BTreeMap::new(),
            timeout_ms: None,
        });
        let d = McpDispatcher::new(Arc::new(reg));
        let err = d
            .dispatch("mcp:fake:test", serde_json::json!({}))
            .await
            .unwrap_err();
        // Should be Io error from spawn, not InvalidConfig (routing OK)
        assert!(matches!(err, McpError::Io(_)));
    }

    #[test]
    fn format_outcome_concatenates_text_parts() {
        let result = McpToolCallResult {
            content: vec![
                McpContentPart::Text {
                    text: "hello".into(),
                },
                McpContentPart::Text {
                    text: "world".into(),
                },
            ],
            is_error: false,
        };
        let out = format_outcome(result);
        assert_eq!(out.text, "hello\nworld");
        assert!(!out.is_error);
    }

    #[test]
    fn format_outcome_propagates_error_flag() {
        let result = McpToolCallResult {
            content: vec![McpContentPart::Text {
                text: "boom".into(),
            }],
            is_error: true,
        };
        let out = format_outcome(result);
        assert!(out.is_error);
    }

    #[test]
    fn format_outcome_handles_image_parts() {
        let result = McpToolCallResult {
            content: vec![McpContentPart::Image {
                data: "BASE64".into(),
                mime_type: "image/png".into(),
            }],
            is_error: false,
        };
        let out = format_outcome(result);
        assert!(out.text.contains("image/png"));
    }

    // ── Phase 38 (mcp-http-and-discover-flake) — HTTP dispatch routing ──

    pub mod http {
        use super::*;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        async fn spawn_one_shot(response: &'static [u8]) -> String {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let mut acc: Vec<u8> = Vec::new();
                loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    acc.extend_from_slice(&buf[..n]);
                    if let Some(idx) = acc.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = std::str::from_utf8(&acc[..idx]).unwrap_or("");
                        let len = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        let body_so_far = acc.len() - (idx + 4);
                        if body_so_far < len {
                            let mut more = vec![0u8; len - body_so_far];
                            sock.read_exact(&mut more).await.unwrap();
                        }
                        break;
                    }
                }
                let _ = sock.write_all(response).await;
                let _ = sock.shutdown().await;
            });
            format!("http://{addr}")
        }

        // Body length: 101 bytes precisely.
        const TOOLS_CALL_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\n\
            Content-Type: application/json\r\n\
            Content-Length: 101\r\n\
            \r\n\
            {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"ok-from-http\"}],\"isError\":false}}";

        #[tokio::test]
        async fn dispatcher_dispatches_to_http_server_when_config_is_http() {
            let url = spawn_one_shot(TOOLS_CALL_RESPONSE).await;
            let mut reg = McpRegistry::new();
            reg.register(McpServerConfig::Http {
                name: "remote".into(),
                url,
                headers: BTreeMap::new(),
                timeout_ms: None,
            });
            let d = McpDispatcher::new(Arc::new(reg));
            let outcome = d
                .dispatch("mcp:remote:do_thing", serde_json::json!({}))
                .await
                .expect("dispatch must reach the mock HTTP server");
            assert!(!outcome.is_error);
            assert!(outcome.text.contains("ok-from-http"));
        }

        #[tokio::test]
        async fn dispatcher_returns_invalid_config_for_unknown_server() {
            let reg = Arc::new(McpRegistry::new());
            let d = McpDispatcher::new(reg);
            let err = d
                .dispatch("mcp:absent:do_thing", serde_json::json!({}))
                .await
                .unwrap_err();
            assert!(matches!(err, McpError::InvalidConfig(_)));
        }
    }
}
