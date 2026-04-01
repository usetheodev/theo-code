// LSP tool - experimental, requires Language Server Protocol integration
// TODO: Implement LSP operations (goToDefinition, references, hover, etc.)

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{PermissionCollector, Tool, ToolContext, ToolOutput};

pub struct LspTool;

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

    async fn execute(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::Execution("LSP tool not yet implemented".to_string()))
    }
}
