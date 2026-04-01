use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{PermissionCollector, Tool, ToolContext, ToolOutput};

pub struct MultiEditTool;

impl MultiEditTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for MultiEditTool {
    fn id(&self) -> &str {
        "multiedit"
    }

    fn description(&self) -> &str {
        "Apply multiple edits to a single file"
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        // TODO: Implement multi-edit (sequential edits on one file)
        Err(ToolError::Execution("MultiEdit tool not yet implemented".to_string()))
    }
}
