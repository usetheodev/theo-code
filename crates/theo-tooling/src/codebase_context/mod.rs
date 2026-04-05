//! Codebase Context tool — on-demand code intelligence via GRAPHCTX.
//!
//! The LLM calls this tool when it needs a map of the codebase structure
//! (function signatures, struct definitions, module layout). For simple tasks
//! like fixing a typo, the LLM skips this tool entirely — zero cost.

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

/// Default token budget for codebase context queries.
const DEFAULT_BUDGET: usize = 4000;

/// Timeout for waiting on graph context query (seconds).
const QUERY_TIMEOUT_SECS: u64 = 30;

pub struct CodebaseContextTool;

impl CodebaseContextTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CodebaseContextTool {
    fn id(&self) -> &str {
        "codebase_context"
    }

    fn description(&self) -> &str {
        "Get a map of the codebase structure: function signatures, struct definitions, and module layout. \
         Call this BEFORE editing multiple files or performing refactoring. \
         Returns the most relevant code structures for your current task. \
         For simple single-file tasks (fix typo, add one function), skip this tool."
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "query".into(),
                    param_type: "string".into(),
                    description: "What you're looking for (e.g., 'authentication flow', 'database models', 'API endpoints'). Used to rank the most relevant modules.".into(),
                    required: true,
                },
                ToolParam {
                    name: "budget_tokens".into(),
                    param_type: "number".into(),
                    description: "Maximum tokens of context to return (default: 4000).".into(),
                    required: false,
                },
            ],
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        _perms: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let budget = args
            .get("budget_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_BUDGET);

        // Check if graph context provider is available.
        let provider = match &ctx.graph_context {
            Some(p) => p,
            None => {
                return Ok(ToolOutput {
                    title: "Codebase Context".into(),
                    output: "Code intelligence is not available for this project. Use grep and glob to explore the codebase manually.".into(),
                    metadata: serde_json::json!({"status": "unavailable"}),
                    attachments: None,
                });
            }
        };

        // If not ready (Building state), return immediately with status.
        if !provider.is_ready() {
            return Ok(ToolOutput {
                title: "Codebase Context (building)".into(),
                output: "Code graph is being built in the background. Try again in a few seconds, or use grep/glob to explore manually while waiting.".into(),
                metadata: serde_json::json!({"status": "building"}),
                attachments: None,
            });
        }

        // Query with timeout.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(QUERY_TIMEOUT_SECS),
            provider.query_context(query, budget),
        )
        .await;

        match result {
            Ok(Ok(ctx_result)) => {
                let text = ctx_result.to_prompt_text();
                if text.is_empty() {
                    return Ok(ToolOutput {
                        title: "Codebase Context".into(),
                        output: "No relevant code structures found for this query. Try a different search term or use grep/glob.".into(),
                        metadata: serde_json::json!({
                            "status": "empty",
                            "query": query,
                            "budget_tokens": budget,
                        }),
                        attachments: None,
                    });
                }

                Ok(ToolOutput {
                    title: format!("Codebase Context ({} tokens)", ctx_result.total_tokens),
                    output: text,
                    metadata: serde_json::json!({
                        "status": "ok",
                        "total_tokens": ctx_result.total_tokens,
                        "budget_tokens": ctx_result.budget_tokens,
                        "blocks": ctx_result.blocks.len(),
                        "query": query,
                    }),
                    attachments: None,
                })
            }
            Ok(Err(e)) => Ok(ToolOutput {
                title: "Codebase Context (error)".into(),
                output: format!(
                    "Code intelligence query failed: {e}. Use grep and glob to explore the codebase manually."
                ),
                metadata: serde_json::json!({"status": "error", "error": e.to_string()}),
                attachments: None,
            }),
            Err(_timeout) => Ok(ToolOutput {
                title: "Codebase Context (timeout)".into(),
                output: format!(
                    "Code graph query timed out after {QUERY_TIMEOUT_SECS}s. The codebase may be too large. Use grep and glob instead."
                ),
                metadata: serde_json::json!({"status": "timeout", "timeout_secs": QUERY_TIMEOUT_SECS}),
                attachments: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_id_and_schema() {
        let tool = CodebaseContextTool::new();
        assert_eq!(tool.id(), "codebase_context");
        assert_eq!(tool.category(), ToolCategory::Search);
        assert!(tool.schema().validate().is_ok());

        let schema = tool.schema();
        let query_param = schema.params.iter().find(|p| p.name == "query").unwrap();
        assert!(query_param.required);

        let budget_param = schema.params.iter().find(|p| p.name == "budget_tokens").unwrap();
        assert!(!budget_param.required);
    }

    #[tokio::test]
    async fn execute_without_provider_returns_unavailable() {
        let tool = CodebaseContextTool::new();
        let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();

        let result = tool
            .execute(serde_json::json!({"query": "test"}), &ctx, &mut perms)
            .await
            .unwrap();

        assert!(result.output.contains("not available"));
        assert_eq!(result.metadata["status"], "unavailable");
    }
}
