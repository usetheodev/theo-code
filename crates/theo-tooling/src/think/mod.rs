//! ThinkTool — explicit reasoning scratchpad.
//!
//! Allows the LLM to "think out loud" before acting. The thought is echoed back
//! and stays in the conversation history as a traceable planning artifact.
//! Zero side effects — purely for structured reasoning and observability.

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

pub struct ThinkTool;

impl ThinkTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ThinkTool {
    fn id(&self) -> &str {
        "think"
    }

    fn description(&self) -> &str {
        "Think through a problem step-by-step before acting. Use this to plan your approach for complex tasks. Your thought will be recorded and visible in the conversation."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "thought".to_string(),
                param_type: "string".to_string(),
                description: "Your reasoning, plan, or analysis. Be specific: what files to read, what changes to make, in what order.".to_string(),
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
        let thought = args
            .get("thought")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'thought' field".to_string()))?;

        Ok(ToolOutput {
            title: "Thought recorded".to_string(),
            output: thought.to_string(),
            metadata: serde_json::json!({"type": "think"}),
            attachments: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[tokio::test]
    async fn returns_thought_as_output() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = ThinkTool::new();
        let result = tool
            .execute(
                serde_json::json!({"thought": "I need to read main.rs first, then edit the function."}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(
            result.output,
            "I need to read main.rs first, then edit the function."
        );
        assert_eq!(result.title, "Thought recorded");
    }

    #[tokio::test]
    async fn missing_thought_returns_error() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = ThinkTool::new();
        let result = tool.execute(serde_json::json!({}), &ctx, &mut perms).await;

        assert!(result.is_err());
    }

    #[test]
    fn schema_has_required_thought() {
        let tool = ThinkTool::new();
        let schema = tool.schema();
        assert_eq!(schema.params.len(), 1);
        assert_eq!(schema.params[0].name, "thought");
        assert!(schema.params[0].required);
    }

    #[test]
    fn id_is_think() {
        assert_eq!(ThinkTool::new().id(), "think");
    }

    #[test]
    fn category_is_utility() {
        assert_eq!(ThinkTool::new().category(), ToolCategory::Utility);
    }
}
