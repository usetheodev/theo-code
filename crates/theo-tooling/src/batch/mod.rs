use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{PermissionCollector, Tool, ToolContext, ToolOutput};

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
