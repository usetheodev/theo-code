use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multiple: bool,
}

/// Trait for asking questions to the user (injectable for testing)
#[async_trait]
pub trait QuestionAsker: Send + Sync {
    async fn ask(&self, questions: &[Question]) -> Vec<Vec<String>>;
}

pub struct QuestionTool {
    asker: Box<dyn QuestionAsker>,
}

impl QuestionTool {
    pub fn new(asker: Box<dyn QuestionAsker>) -> Self {
        Self { asker }
    }
}

#[async_trait]
impl Tool for QuestionTool {
    fn id(&self) -> &str {
        "question"
    }

    fn description(&self) -> &str {
        "Ask the user a question"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "questions".to_string(),
                param_type: "array".to_string(),
                description: "Array of questions to ask the user".to_string(),
                required: true,
            }],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let questions: Vec<Question> = serde_json::from_value(
            args.get("questions")
                .cloned()
                .ok_or_else(|| ToolError::InvalidArgs("Missing 'questions' field".to_string()))?,
        )
        .map_err(|e| ToolError::InvalidArgs(format!("Invalid questions: {e}")))?;

        let answers = self.asker.ask(&questions).await;

        let count = questions.len();
        let title = if count == 1 {
            "Asked 1 question".to_string()
        } else {
            format!("Asked {count} questions")
        };

        let output_parts: Vec<String> = questions
            .iter()
            .zip(answers.iter())
            .map(|(q, a)| format!("\"{}\"=\"{}\"", q.question, a.join(", ")))
            .collect();

        Ok(ToolOutput {
            title,
            output: output_parts.join("\n"),
            metadata: serde_json::json!({"answers": answers}),
            attachments: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use std::sync::Mutex;

    struct MockAsker {
        responses: Mutex<Vec<Vec<Vec<String>>>>,
    }

    impl MockAsker {
        fn new(responses: Vec<Vec<Vec<String>>>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl QuestionAsker for MockAsker {
        async fn ask(&self, _questions: &[Question]) -> Vec<Vec<String>> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                vec![]
            } else {
                responses.remove(0)
            }
        }
    }

    fn question_tool(responses: Vec<Vec<Vec<String>>>) -> QuestionTool {
        QuestionTool::new(Box::new(MockAsker::new(responses)))
    }

    #[tokio::test]
    async fn successfully_executes_with_valid_question_parameters() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = question_tool(vec![vec![vec!["Red".to_string()]]]);
        let result = tool
            .execute(
                serde_json::json!({
                    "questions": [{
                        "question": "What is your favorite color?",
                        "header": "Color",
                        "options": [
                            {"label": "Red", "description": "The color of passion"},
                            {"label": "Blue", "description": "The color of sky"},
                        ],
                        "multiple": false,
                    }]
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(result.title, "Asked 1 question");
    }

    #[tokio::test]
    async fn header_longer_than_12_but_less_than_30_passes() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = question_tool(vec![vec![vec!["Dog".to_string()]]]);
        let result = tool
            .execute(
                serde_json::json!({
                    "questions": [{
                        "question": "What is your favorite animal?",
                        "header": "This Header is Over 12",
                        "options": [{"label": "Dog", "description": "Man's best friend"}],
                    }]
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(
            result
                .output
                .contains("\"What is your favorite animal?\"=\"Dog\"")
        );
    }
}
