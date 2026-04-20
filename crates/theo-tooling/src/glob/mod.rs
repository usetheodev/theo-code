use async_trait::async_trait;
use std::path::PathBuf;
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    optional_string, require_string,
};

const MAX_RESULTS: usize = 100;

pub struct GlobTool;

impl GlobTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn id(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        concat!(
            "Find files by path/name pattern. Returns a list of matching paths. ",
            "Use this when you need to locate files by their NAME or PATH — wildcards like `**/*.rs`, `src/**/mod.rs`, `*.toml`. ",
            "Use `grep` instead to match FILE CONTENT. ",
            "Use `read` instead if you already know the file and want its contents. ",
            "Default root is the project directory; pass `path` to narrow. Broad patterns (`**/*`) on large repos are expensive — prefer narrower patterns. ",
            "Example: glob({pattern: 'crates/**/Cargo.toml'}). ",
            "Example: glob({pattern: '**/README*.md'})."
        )
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "pattern".to_string(),
                    param_type: "string".to_string(),
                    description: "Glob pattern to match files (e.g. 'src/**/*.rs')".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "path".to_string(),
                    param_type: "string".to_string(),
                    description: "Base directory to search in".to_string(),
                    required: false,
                },
            ],
        input_examples: Vec::new(),
    }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    /// Glob output is an alphabetically-sorted file list — head is the
    /// stable prefix the agent can use to see representative matches.
    fn truncation_rule(&self) -> Option<theo_domain::tool::TruncationRule> {
        Some(theo_domain::tool::TruncationRule {
            max_chars: 3_000,
            strategy: theo_domain::tool::TruncationStrategy::Head,
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let pattern = require_string(&args, "pattern")?;
        let base_path = optional_string(&args, "path")
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.project_dir.clone());

        permissions.record(PermissionRequest {
            permission: PermissionType::Glob,
            patterns: vec![pattern.clone()],
            always: vec![pattern.clone()],
            metadata: serde_json::json!({}),
        });

        let full_pattern = base_path.join(&pattern).display().to_string();
        let paths: Vec<PathBuf> = glob::glob(&full_pattern)
            .map_err(|e| ToolError::Execution(format!("Invalid glob pattern: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        let total = paths.len();
        let truncated = total > MAX_RESULTS;
        let displayed: Vec<String> = paths
            .iter()
            .take(MAX_RESULTS)
            .map(|p| {
                p.strip_prefix(&ctx.project_dir)
                    .unwrap_or(p)
                    .display()
                    .to_string()
            })
            .collect();

        let mut output = displayed.join("\n");
        if truncated {
            output.push_str(&format!(
                "\n\n(showing {MAX_RESULTS} of {total} results, pattern too broad)"
            ));
        }

        Ok(ToolOutput {
            title: format!("glob: {pattern}"),
            output,
            metadata: serde_json::json!({
                "count": total,
                "truncated": truncated,
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

    fn glob_tool() -> GlobTool {
        GlobTool::new()
    }

    #[tokio::test]
    async fn finds_matching_files() {
        let tmp = TestDir::new();
        tmp.write_file("src/main.rs", "fn main() {}");
        tmp.write_file("src/lib.rs", "pub mod foo;");
        tmp.write_file("README.md", "# Hello");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = glob_tool()
            .execute(
                serde_json::json!({
                    "pattern": "src/*.rs",
                    "path": tmp.path().to_string_lossy().to_string(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.metadata["count"].as_u64().unwrap() >= 2);
        assert!(result.output.contains("main.rs"));
        assert!(result.output.contains("lib.rs"));
    }

    #[tokio::test]
    async fn returns_empty_for_no_matches() {
        let tmp = TestDir::new();
        tmp.write_file("test.txt", "hello");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = glob_tool()
            .execute(
                serde_json::json!({
                    "pattern": "*.nonexistent",
                    "path": tmp.path().to_string_lossy().to_string(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(result.metadata["count"], 0);
    }
}
