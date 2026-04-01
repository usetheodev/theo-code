use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

pub struct BatchTool;

impl BatchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for BatchTool {
    fn id(&self) -> &str {
        "batch"
    }

    fn description(&self) -> &str {
        "Execute multiple tool calls in parallel (experimental)"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "calls".to_string(),
                param_type: "array".to_string(),
                description: "Array of tool calls to execute in parallel (max 25)".to_string(),
                required: true,
            }],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        // TODO: Implement batch tool execution (up to 25 parallel calls)
        Err(ToolError::Execution("Batch tool not yet implemented".to_string()))
    }
}
