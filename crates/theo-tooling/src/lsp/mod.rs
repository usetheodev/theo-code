// LSP tool - experimental, requires Language Server Protocol integration
// TODO: Implement LSP operations (goToDefinition, references, hover, etc.)

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

pub struct LspTool;

impl Default for LspTool {
    fn default() -> Self {
        Self::new()
    }
}

impl LspTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for LspTool {
    fn id(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Language Server Protocol operations (experimental)"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "operation".to_string(),
                    param_type: "string".to_string(),
                    description: "LSP operation: goToDefinition, references, hover".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "filePath".to_string(),
                    param_type: "string".to_string(),
                    description: "Path to the source file".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "line".to_string(),
                    param_type: "integer".to_string(),
                    description: "Line number (0-based)".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "character".to_string(),
                    param_type: "integer".to_string(),
                    description: "Character offset (0-based)".to_string(),
                    required: true,
                },
            ],
        input_examples: Vec::new(),
    }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::Execution(
            "LSP tool not yet implemented".to_string(),
        ))
    }
}
