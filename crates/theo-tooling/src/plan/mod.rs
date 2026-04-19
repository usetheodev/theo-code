use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolSchema,
};

pub struct PlanExitTool;

impl PlanExitTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PlanExitTool {
    fn id(&self) -> &str {
        "plan_exit"
    }

    fn description(&self) -> &str {
        "Exit plan mode (experimental)"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput {
            title: "Plan mode exit".to_string(),
            output: "Switching to build agent...".to_string(),
            metadata: serde_json::json!({}),
            attachments: None,
            llm_suffix: None,
        })
    }
}
