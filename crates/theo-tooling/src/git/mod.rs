//! Git builtin plugin — typed git operations (not generic bash).
//!
//! Tools: git_status, git_diff, git_log, git_commit
//! All execute via tokio::process::Command with safety checks.

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

async fn run_git(args: &[&str], project_dir: &std::path::Path) -> Result<String, ToolError> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(project_dir)
        .output()
        .await
        .map_err(|e| ToolError::Execution(format!("git not available: {e}")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(ToolError::Execution(format!("git error: {}", stderr.trim())))
    }
}

// ---------------------------------------------------------------------------
// GitStatusTool
// ---------------------------------------------------------------------------

pub struct GitStatusTool;

#[async_trait]
impl Tool for GitStatusTool {
    fn id(&self) -> &str { "git_status" }
    fn description(&self) -> &str { "Show git status: modified, staged, and untracked files." }
    fn category(&self) -> ToolCategory { ToolCategory::Utility }
    fn schema(&self) -> ToolSchema { ToolSchema { params: vec![] } }

    async fn execute(&self, _args: serde_json::Value, ctx: &ToolContext, _p: &mut PermissionCollector) -> Result<ToolOutput, ToolError> {
        let output = run_git(&["status", "--short"], &ctx.project_dir).await?;
        Ok(ToolOutput { title: "git status".into(), output, metadata: serde_json::json!({}), attachments: None })
    }
}

// ---------------------------------------------------------------------------
// GitDiffTool
// ---------------------------------------------------------------------------

pub struct GitDiffTool;

#[async_trait]
impl Tool for GitDiffTool {
    fn id(&self) -> &str { "git_diff" }
    fn description(&self) -> &str { "Show git diff of current changes. Use file param to diff a specific file." }
    fn category(&self) -> ToolCategory { ToolCategory::Utility }
    fn schema(&self) -> ToolSchema {
        ToolSchema { params: vec![
            ToolParam { name: "file".into(), param_type: "string".into(), description: "Optional: specific file to diff".into(), required: false },
            ToolParam { name: "staged".into(), param_type: "boolean".into(), description: "Show staged changes (--cached)".into(), required: false },
        ] }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext, _p: &mut PermissionCollector) -> Result<ToolOutput, ToolError> {
        let mut git_args = vec!["diff"];
        if args.get("staged").and_then(|v| v.as_bool()).unwrap_or(false) {
            git_args.push("--cached");
        }
        let file = args.get("file").and_then(|v| v.as_str()).map(String::from);
        if let Some(ref f) = file {
            git_args.push("--");
            git_args.push(f);
        }
        let output = run_git(&git_args, &ctx.project_dir).await?;
        Ok(ToolOutput { title: "git diff".into(), output, metadata: serde_json::json!({}), attachments: None })
    }
}

// ---------------------------------------------------------------------------
// GitLogTool
// ---------------------------------------------------------------------------

pub struct GitLogTool;

#[async_trait]
impl Tool for GitLogTool {
    fn id(&self) -> &str { "git_log" }
    fn description(&self) -> &str { "Show recent git commits. Default: last 10." }
    fn category(&self) -> ToolCategory { ToolCategory::Utility }
    fn schema(&self) -> ToolSchema {
        ToolSchema { params: vec![
            ToolParam { name: "count".into(), param_type: "number".into(), description: "Number of commits to show (default 10)".into(), required: false },
        ] }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext, _p: &mut PermissionCollector) -> Result<ToolOutput, ToolError> {
        let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(10);
        let count_str = format!("-{}", count.min(50));
        let output = run_git(&["log", "--oneline", "--graph", &count_str], &ctx.project_dir).await?;
        Ok(ToolOutput { title: "git log".into(), output, metadata: serde_json::json!({}), attachments: None })
    }
}

// ---------------------------------------------------------------------------
// GitCommitTool
// ---------------------------------------------------------------------------

pub struct GitCommitTool;

#[async_trait]
impl Tool for GitCommitTool {
    fn id(&self) -> &str { "git_commit" }
    fn description(&self) -> &str { "Stage files and create a git commit. NEVER force pushes. NEVER amends unless explicit." }
    fn category(&self) -> ToolCategory { ToolCategory::Utility }
    fn schema(&self) -> ToolSchema {
        ToolSchema { params: vec![
            ToolParam { name: "message".into(), param_type: "string".into(), description: "Commit message".into(), required: true },
            ToolParam { name: "files".into(), param_type: "string".into(), description: "Space-separated files to stage (default: all modified)".into(), required: false },
        ] }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext, _p: &mut PermissionCollector) -> Result<ToolOutput, ToolError> {
        let message = args.get("message").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Execution("commit message required".into()))?;
        let files = args.get("files").and_then(|v| v.as_str()).unwrap_or(".");

        // Safety: never commit .env or credentials
        if files.contains(".env") || files.contains("credentials") || files.contains("secret") {
            return Err(ToolError::Execution("BLOCKED: cannot commit files containing secrets (.env, credentials, secret)".into()));
        }

        // Stage files
        let stage_args: Vec<&str> = if files == "." {
            vec!["add", "-A"]
        } else {
            let mut a = vec!["add"];
            a.extend(files.split_whitespace());
            a
        };
        run_git(&stage_args, &ctx.project_dir).await?;

        // Commit
        let output = run_git(&["commit", "-m", message], &ctx.project_dir).await?;

        Ok(ToolOutput {
            title: "git commit".into(),
            output,
            metadata: serde_json::json!({"message": message}),
            attachments: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_tools_have_correct_ids() {
        assert_eq!(GitStatusTool.id(), "git_status");
        assert_eq!(GitDiffTool.id(), "git_diff");
        assert_eq!(GitLogTool.id(), "git_log");
        assert_eq!(GitCommitTool.id(), "git_commit");
    }

    #[test]
    fn git_commit_schema_requires_message() {
        let schema = GitCommitTool.schema();
        let msg_param = schema.params.iter().find(|p| p.name == "message").unwrap();
        assert!(msg_param.required);
    }

    #[test]
    fn git_diff_schema_has_optional_params() {
        let schema = GitDiffTool.schema();
        assert!(schema.params.iter().all(|p| !p.required));
    }
}
