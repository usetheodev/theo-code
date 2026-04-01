use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    require_string,
};

pub struct WebSearchTool;

impl WebSearchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn id(&self) -> &str {
        "websearch"
    }

    fn description(&self) -> &str {
        "Search the web"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "query".to_string(),
                param_type: "string".to_string(),
                description: "Search query".to_string(),
                required: true,
            }],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Web
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let query = require_string(&args, "query")?;
        // TODO: Implement web search via external API
        Err(ToolError::Execution(format!("Web search not yet implemented for query: {query}")))
    }
}
