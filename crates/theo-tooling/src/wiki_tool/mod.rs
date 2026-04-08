//! Wiki tools — query and ingest via WikiBackend trait (DIP).
//!
//! These tools let the agent query the code wiki and ingest runtime insights.
//! They depend ONLY on theo-domain::WikiBackend trait, not on theo-engine-retrieval.

use std::sync::Arc;
use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    require_string, optional_string,
};
use theo_domain::wiki_backend::{WikiBackend, WikiInsightInput};

// ---------------------------------------------------------------------------
// WikiQueryTool
// ---------------------------------------------------------------------------

pub struct WikiQueryTool {
    backend: Arc<dyn WikiBackend>,
}

impl WikiQueryTool {
    pub fn new(backend: Arc<dyn WikiBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for WikiQueryTool {
    fn id(&self) -> &str {
        "wiki_query"
    }

    fn description(&self) -> &str {
        "Query the code wiki for relevant knowledge about the codebase. Returns module descriptions, API docs, runtime notes, and cached insights."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "query".to_string(),
                    param_type: "string".to_string(),
                    description: "Natural language question about the codebase (e.g., 'how does authentication work', 'what modules handle routing')".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "max_results".to_string(),
                    param_type: "number".to_string(),
                    description: "Maximum number of results to return (default: 3)".to_string(),
                    required: false,
                },
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let query = require_string(&args, "query")?;
        let max_results = args.get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;

        let results = self.backend.query(&query, max_results).await;

        if results.is_empty() {
            return Ok(ToolOutput {
                title: "Wiki Query".to_string(),
                output: format!("No wiki pages found for: \"{}\"", query),
                metadata: serde_json::json!({"query": query, "results": 0}),
                attachments: None,
            });
        }

        let mut output = format!("Found {} wiki pages for \"{}\":\n\n", results.len(), query);
        for (i, r) in results.iter().enumerate() {
            output += &format!("## {}. {} [T:{}{}]\n", i + 1, r.title, r.authority_tier,
                if r.is_stale { " STALE" } else { "" });
            if !r.summary.is_empty() {
                output += &format!("**Summary**: {}\n", r.summary);
            }
            output += &format!("**Confidence**: {:.0}%\n\n", r.confidence * 100.0);
            // First 500 chars of content
            let preview = if r.content.len() > 500 {
                format!("{}...", &r.content[..500])
            } else {
                r.content.clone()
            };
            output += &preview;
            output += "\n\n---\n\n";
        }

        Ok(ToolOutput {
            title: format!("Wiki: {}", query),
            output,
            metadata: serde_json::json!({
                "query": query,
                "results": results.len(),
                "top_slug": results.first().map(|r| r.slug.as_str()).unwrap_or(""),
                "top_confidence": results.first().map(|r| r.confidence).unwrap_or(0.0),
            }),
            attachments: None,
        })
    }
}

// ---------------------------------------------------------------------------
// WikiIngestTool
// ---------------------------------------------------------------------------

pub struct WikiIngestTool {
    backend: Arc<dyn WikiBackend>,
}

impl WikiIngestTool {
    pub fn new(backend: Arc<dyn WikiBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for WikiIngestTool {
    fn id(&self) -> &str {
        "wiki_ingest"
    }

    fn description(&self) -> &str {
        "Record a runtime execution result in the wiki. The wiki learns from test results, build outcomes, and command executions to provide better operational knowledge."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "command".to_string(),
                    param_type: "string".to_string(),
                    description: "The command that was executed (e.g., 'cargo test -p auth')".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "exit_code".to_string(),
                    param_type: "number".to_string(),
                    description: "Exit code of the command (0 = success)".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "stdout".to_string(),
                    param_type: "string".to_string(),
                    description: "Standard output (first 500 chars)".to_string(),
                    required: false,
                },
                ToolParam {
                    name: "stderr".to_string(),
                    param_type: "string".to_string(),
                    description: "Standard error output (first 500 chars)".to_string(),
                    required: false,
                },
                ToolParam {
                    name: "source".to_string(),
                    param_type: "string".to_string(),
                    description: "Source type: cargo_test, cargo_build, bash, agent".to_string(),
                    required: false,
                },
            ],
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
        let command = require_string(&args, "command")?;
        let exit_code = args.get("exit_code")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let stdout = optional_string(&args, "stdout").unwrap_or_default();
        let stderr = optional_string(&args, "stderr").unwrap_or_default();
        let source = optional_string(&args, "source").unwrap_or_else(|| "agent".to_string());

        let input = WikiInsightInput {
            source,
            command: command.clone(),
            exit_code,
            success: exit_code == 0,
            duration_ms: 0, // caller doesn't always know
            stdout,
            stderr,
        };

        match self.backend.ingest(input).await {
            Ok(result) => {
                let status = if result.ingested { "recorded" } else { "skipped" };
                Ok(ToolOutput {
                    title: format!("Wiki Ingest: {}", command),
                    output: format!(
                        "Runtime insight {}: {} (exit {})\nAffected: {} files, {} symbols\nTotal insights: {}",
                        status, command, exit_code,
                        result.affected_files.len(), result.affected_symbols.len(),
                        result.total_insights
                    ),
                    metadata: serde_json::json!({
                        "ingested": result.ingested,
                        "affected_files": result.affected_files,
                        "affected_symbols": result.affected_symbols,
                        "total_insights": result.total_insights,
                    }),
                    attachments: None,
                })
            }
            Err(e) => Err(ToolError::Execution(format!("Wiki ingest failed: {}", e))),
        }
    }
}

// ---------------------------------------------------------------------------
// WikiGenerateTool
// ---------------------------------------------------------------------------

pub struct WikiGenerateTool {
    backend: Arc<dyn WikiBackend>,
}

impl WikiGenerateTool {
    pub fn new(backend: Arc<dyn WikiBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for WikiGenerateTool {
    fn id(&self) -> &str {
        "wiki_generate"
    }

    fn description(&self) -> &str {
        "Generate or update the code wiki for the current project. Creates a new wiki if none exists, or incrementally updates the existing one. The wiki contains module documentation, API references, dependencies, and runtime notes."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema { params: vec![] } // No parameters — operates on current project
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
        match self.backend.generate().await {
            Ok(result) => {
                let mode = if result.is_incremental { "incremental update" } else { "full generation" };
                Ok(ToolOutput {
                    title: "Wiki Generated".to_string(),
                    output: format!(
                        "Wiki {} complete in {}ms:\n- {} pages generated\n- {} pages updated\n- {} pages skipped\n\nWiki at: {}",
                        mode, result.duration_ms,
                        result.pages_generated, result.pages_updated, result.pages_skipped,
                        result.wiki_dir
                    ),
                    metadata: serde_json::json!({
                        "pages_generated": result.pages_generated,
                        "pages_updated": result.pages_updated,
                        "pages_skipped": result.pages_skipped,
                        "duration_ms": result.duration_ms,
                        "is_incremental": result.is_incremental,
                        "wiki_dir": result.wiki_dir,
                    }),
                    attachments: None,
                })
            }
            Err(e) => Err(ToolError::Execution(format!("Wiki generation failed: {}", e))),
        }
    }
}
