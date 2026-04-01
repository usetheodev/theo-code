use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{PermissionCollector, Tool, ToolContext, ToolOutput, require_string};

pub struct CodeSearchTool;

impl CodeSearchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CodeSearchTool {
    fn id(&self) -> &str {
        "codesearch"
    }

    fn description(&self) -> &str {
        "Search code context"
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let query = require_string(&args, "query")?;
        // TODO: Implement code search via external API
        Err(ToolError::Execution(format!("Code search not yet implemented for query: {query}")))
    }
}
