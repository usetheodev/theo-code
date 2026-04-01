use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    require_string,
};

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

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "tool".to_string(),
                    param_type: "string".to_string(),
                    description: "Name of the invalid tool".to_string(),
                    required: false,
                },
                ToolParam {
                    name: "error".to_string(),
                    param_type: "string".to_string(),
                    description: "Error message".to_string(),
                    required: false,
                },
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
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
