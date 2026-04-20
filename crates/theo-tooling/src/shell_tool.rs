//! ShellTool — generic wrapper for shell-based plugin tools.
//!
//! A ShellTool executes a shell script, passing arguments as JSON via stdin,
//! and captures stdout as the tool output. Used by the plugin system.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

pub struct ShellTool {
    name: String,
    description: String,
    script_path: PathBuf,
    params: Vec<ToolParam>,
    timeout: Duration,
}

impl ShellTool {
    pub fn new(
        name: String,
        description: String,
        script_path: PathBuf,
        params: Vec<ToolParam>,
    ) -> Self {
        Self {
            name,
            description,
            script_path,
            params,
            timeout: Duration::from_secs(30),
        }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn id(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: self.params.clone(),
            input_examples: Vec::new(),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        let args_json = serde_json::to_string(&serde_json::json!({
            "args": args,
            "project_dir": ctx.project_dir.display().to_string(),
        }))
        .unwrap_or_default();

        let mut cmd = Command::new("sh");
        cmd.arg(&self.script_path);
        cmd.current_dir(&ctx.project_dir);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            ToolError::Execution(format!("Failed to run plugin tool '{}': {e}", self.name))
        })?;

        // Write args to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(args_json.as_bytes()).await;
            drop(stdin);
        }

        // Wait with timeout
        match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if output.status.success() {
                    Ok(ToolOutput {
                        title: format!("Plugin: {}", self.name),
                        output: stdout,
                        metadata: serde_json::json!({"plugin_tool": self.name}),
                        attachments: None,
                        llm_suffix: None,
                    })
                } else {
                    let msg = if stderr.is_empty() { stdout } else { stderr };
                    Err(ToolError::Execution(format!(
                        "Plugin tool '{}' failed (exit {}): {}",
                        self.name,
                        output.status.code().unwrap_or(-1),
                        msg.trim()
                    )))
                }
            }
            Ok(Err(e)) => Err(ToolError::Execution(format!(
                "Plugin tool '{}' error: {e}",
                self.name
            ))),
            Err(_) => Err(ToolError::Execution(format!(
                "Plugin tool '{}' timed out after {}s",
                self.name,
                self.timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::tool::ToolParam;

    #[test]
    fn shell_tool_has_correct_id() {
        let tool = ShellTool::new(
            "my_tool".into(),
            "A test tool".into(),
            PathBuf::from("/tmp/tool.sh"),
            vec![],
        );
        assert_eq!(tool.id(), "my_tool");
        assert_eq!(tool.description(), "A test tool");
        assert_eq!(tool.category(), ToolCategory::Utility);
    }

    #[test]
    fn shell_tool_schema_has_params() {
        let tool = ShellTool::new(
            "query".into(),
            "Run a query".into(),
            PathBuf::from("/tmp/query.sh"),
            vec![ToolParam {
                name: "sql".into(),
                param_type: "string".into(),
                description: "SQL query to run".into(),
                required: true,
            }],
        );
        let schema = tool.schema();
        assert_eq!(schema.params.len(), 1);
        assert_eq!(schema.params[0].name, "sql");
    }

    #[tokio::test]
    async fn shell_tool_executes_script() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("hello.sh");
        std::fs::write(&script, "#!/bin/sh\necho 'Hello from plugin'\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let tool = ShellTool::new("hello".into(), "test".into(), script, vec![]);
        let ctx = ToolContext::test_context(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();
        let result = tool.execute(serde_json::json!({}), &ctx, &mut perms).await;

        assert!(result.is_ok());
        assert!(result.unwrap().output.contains("Hello from plugin"));
    }

    #[tokio::test]
    async fn shell_tool_failing_script_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fail.sh");
        std::fs::write(&script, "#!/bin/sh\necho 'bad' >&2\nexit 1\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let tool = ShellTool::new("fail".into(), "test".into(), script, vec![]);
        let ctx = ToolContext::test_context(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();
        let result = tool.execute(serde_json::json!({}), &ctx, &mut perms).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("bad"));
    }
}
