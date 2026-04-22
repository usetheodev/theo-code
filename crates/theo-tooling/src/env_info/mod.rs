//! Env Info builtin plugin — system and environment information.

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolSchema,
};

pub struct EnvInfoTool;

#[async_trait]
impl Tool for EnvInfoTool {
    fn id(&self) -> &str {
        "env_info"
    }
    fn description(&self) -> &str {
        "Show system environment info: OS, architecture, Rust version, project dir, available tools."
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema::new()
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        ctx: &ToolContext,
        _p: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let mut info = String::new();

        info.push_str(&format!("OS: {}\n", std::env::consts::OS));
        info.push_str(&format!("Arch: {}\n", std::env::consts::ARCH));
        info.push_str(&format!("Project: {}\n", ctx.project_dir.display()));

        // Check available tools
        let tools = ["cargo", "rustc", "node", "npm", "python3", "git", "docker"];
        for tool in &tools {
            let available = tokio::process::Command::new("which")
                .arg(tool)
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false);
            info.push_str(&format!(
                "{}: {}\n",
                tool,
                if available { "available" } else { "not found" }
            ));
        }

        // Rust version if available
        if let Ok(output) = tokio::process::Command::new("rustc")
            .arg("--version")
            .output()
            .await
            && output.status.success() {
                info.push_str(&format!(
                    "Rust: {}",
                    String::from_utf8_lossy(&output.stdout).trim()
                ));
            }

        Ok(ToolOutput {
            title: "Environment Info".into(),
            output: info,
            metadata: serde_json::json!({"os": std::env::consts::OS, "arch": std::env::consts::ARCH}),
            attachments: None,
            llm_suffix: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_info_tool_id() {
        assert_eq!(EnvInfoTool.id(), "env_info");
    }

    #[tokio::test]
    async fn env_info_returns_os_info() {
        let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let result = EnvInfoTool
            .execute(serde_json::json!({}), &ctx, &mut perms)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap().output;
        assert!(output.contains("OS:"));
        assert!(output.contains("Arch:"));
    }
}
