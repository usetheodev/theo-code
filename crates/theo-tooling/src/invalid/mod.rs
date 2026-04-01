use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{PermissionCollector, Tool, ToolContext, ToolOutput, require_string};

pub struct InvalidTool;

impl InvalidTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for InvalidTool {
    fn id(&self) -> &str {
        "invalid"
    }

    fn description(&self) -> &str {
        "Error placeholder for invalid tool calls"
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let tool = require_string(&args, "tool").unwrap_or_else(|_| "unknown".to_string());
        let error = require_string(&args, "error").unwrap_or_else(|_| "Unknown error".to_string());

        Ok(ToolOutput {
            title: format!("Invalid tool: {tool}"),
            output: format!("Error: {error}"),
            metadata: serde_json::json!({}),
            attachments: None,
        })
    }
}
