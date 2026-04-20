//! ReflectTool — structured self-assessment.
//!
//! Allows the LLM to evaluate its own progress, identify blockers,
//! and decide next steps. Forces structured self-reflection with
//! confidence scoring and explicit next-action planning.

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

pub struct ReflectTool;

impl ReflectTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReflectTool {
    fn id(&self) -> &str {
        "reflect"
    }

    fn description(&self) -> &str {
        "Assess your progress on the current task. Rate your confidence, identify blockers, and plan next steps. Use this when stuck or before calling done()."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "progress".to_string(),
                    param_type: "string".to_string(),
                    description: "What has been accomplished so far".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "blockers".to_string(),
                    param_type: "string".to_string(),
                    description: "What is preventing progress (empty if none)".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "confidence".to_string(),
                    param_type: "integer".to_string(),
                    description: "Confidence the task can be completed (0-100)".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "next_action".to_string(),
                    param_type: "string".to_string(),
                    description: "What to do next".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "should_stop".to_string(),
                    param_type: "boolean".to_string(),
                    description: "Whether to give up and call done() with partial results"
                        .to_string(),
                    required: false,
                },
            ],
        input_examples: Vec::new(),
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
        let progress = args
            .get("progress")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'progress' field".to_string()))?;

        let blockers = args
            .get("blockers")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'blockers' field".to_string()))?;

        let confidence = args
            .get("confidence")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ToolError::InvalidArgs("Missing or invalid 'confidence' field (0-100)".to_string())
            })?;

        let next_action = args
            .get("next_action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'next_action' field".to_string()))?;

        let should_stop = args
            .get("should_stop")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let confidence = confidence.min(100);

        let mut output = format!(
            "## Reflection\n\
             **Progress:** {progress}\n\
             **Blockers:** {blockers}\n\
             **Confidence:** {confidence}%\n\
             **Next:** {next_action}\n"
        );

        if confidence < 30 {
            output.push_str("\n⚠️ LOW CONFIDENCE — Consider changing your approach entirely.\n");
        }

        if should_stop {
            output
                .push_str("\n🛑 STOP REQUESTED — Consider calling done() with partial results.\n");
        }

        Ok(ToolOutput {
            title: format!("Reflection (confidence: {}%)", confidence),
            output,
            metadata: serde_json::json!({
                "type": "reflect",
                "confidence": confidence,
                "should_stop": should_stop,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[tokio::test]
    async fn returns_structured_reflection() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = ReflectTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "progress": "Read 3 files, identified the bug",
                    "blockers": "None",
                    "confidence": 85,
                    "next_action": "Edit calc.rs to fix divide function",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("**Confidence:** 85%"));
        assert!(result.output.contains("Read 3 files"));
        assert!(!result.output.contains("LOW CONFIDENCE"));
    }

    #[tokio::test]
    async fn low_confidence_generates_warning() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = ReflectTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "progress": "Tried 3 approaches",
                    "blockers": "All edits fail",
                    "confidence": 20,
                    "next_action": "Try different strategy",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("LOW CONFIDENCE"));
        assert!(result.title.contains("20%"));
    }

    #[tokio::test]
    async fn should_stop_generates_suggestion() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = ReflectTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "progress": "Partial fix applied",
                    "blockers": "Cannot complete without more context",
                    "confidence": 40,
                    "next_action": "Give up",
                    "should_stop": true,
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("STOP REQUESTED"));
        assert_eq!(result.metadata["should_stop"], true);
    }

    #[tokio::test]
    async fn missing_progress_returns_error() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = ReflectTool::new();
        let result = tool
            .execute(serde_json::json!({"confidence": 50}), &ctx, &mut perms)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn confidence_clamped_to_100() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = ReflectTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "progress": "Done",
                    "blockers": "",
                    "confidence": 999,
                    "next_action": "call done",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.title.contains("100%"));
    }

    #[test]
    fn id_is_reflect() {
        assert_eq!(ReflectTool::new().id(), "reflect");
    }

    #[test]
    fn schema_has_required_fields() {
        let tool = ReflectTool::new();
        let schema = tool.schema();
        let required: Vec<&str> = schema
            .params
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        assert!(required.contains(&"progress"));
        assert!(required.contains(&"blockers"));
        assert!(required.contains(&"confidence"));
        assert!(required.contains(&"next_action"));
    }
}
