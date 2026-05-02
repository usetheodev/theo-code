//! MCP JSON-RPC 2.0 message types.

use serde::{Deserialize, Serialize};

/// JSON-RPC request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl McpRequest {
    pub fn new(id: impl Into<serde_json::Value>, method: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            method: method.into(),
            params: None,
        }
    }

    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = Some(params);
        self
    }
}

/// JSON-RPC response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<McpRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Tool descriptor returned by `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema for the tool's input parameters.
    #[serde(rename = "inputSchema", default)]
    pub input_schema: serde_json::Value,
}

/// Result of `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCallResult {
    /// MCP returns content as an array of typed parts.
    pub content: Vec<McpContentPart>,
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpContentPart {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { resource: serde_json::Value },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_request_jsonrpc_field_is_2_0() {
        let req = McpRequest::new(1, "tools/list");
        assert_eq!(req.jsonrpc, "2.0");
    }

    #[test]
    fn mcp_request_with_params_sets_params() {
        let req = McpRequest::new(1, "tools/call")
            .with_params(serde_json::json!({"name": "x"}));
        assert!(req.params.is_some());
    }

    #[test]
    fn mcp_request_serde_roundtrip() {
        let req = McpRequest::new(42, "tools/list");
        let json = serde_json::to_string(&req).unwrap();
        let back: McpRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, serde_json::json!(42));
        assert_eq!(back.method, "tools/list");
    }

    #[test]
    fn mcp_response_serde_roundtrip_success() {
        let resp = McpResponse {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            result: Some(serde_json::json!({"tools": []})),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: McpResponse = serde_json::from_str(&json).unwrap();
        assert!(back.result.is_some());
        assert!(back.error.is_none());
    }

    #[test]
    fn mcp_response_serde_roundtrip_error() {
        let resp = McpResponse {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            result: None,
            error: Some(McpRpcError {
                code: -32601,
                message: "Method not found".into(),
                data: None,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: McpResponse = serde_json::from_str(&json).unwrap();
        assert!(back.error.is_some());
        assert_eq!(back.error.unwrap().code, -32601);
    }

    #[test]
    fn mcp_tool_parses_input_schema() {
        let json = r#"{
            "name": "search",
            "description": "Web search",
            "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}}
        }"#;
        let tool: McpTool = serde_json::from_str(json).unwrap();
        assert_eq!(tool.name, "search");
        assert_eq!(tool.input_schema["type"], "object");
    }

    #[test]
    fn mcp_content_part_text_serde() {
        let part = McpContentPart::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let back: McpContentPart = serde_json::from_str(&json).unwrap();
        match back {
            McpContentPart::Text { text } => assert_eq!(text, "hello"),
            _ => panic!(),
        }
    }
}
