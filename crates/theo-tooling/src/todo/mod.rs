use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

pub struct TodoTool;

impl TodoTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TodoTool {
    fn id(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "Update the session todo list"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "todos".to_string(),
                param_type: "array".to_string(),
                description: "Array of todo items".to_string(),
                required: true,
            }],
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
        let todos = args.get("todos").cloned().unwrap_or(serde_json::json!([]));
        Ok(ToolOutput {
            title: "Updated todos".to_string(),
            output: serde_json::to_string(&todos).unwrap_or_default(),
            metadata: serde_json::json!({"todos": todos}),
            attachments: None,
        })
    }
}
