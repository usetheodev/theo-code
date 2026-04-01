use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{PermissionCollector, Tool, ToolContext, ToolOutput, optional_string};
use std::path::PathBuf;

pub struct LsTool;

impl LsTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for LsTool {
    fn id(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        "List directory contents"
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let path = optional_string(&args, "path")
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.project_dir.clone());

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&path)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read directory: {e}")))?;

        while let Some(entry) = dir.next_entry().await.map_err(|e| ToolError::Execution(format!("{e}")))? {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.metadata().await.map(|m| m.is_dir()).unwrap_or(false);
            if is_dir {
                entries.push(format!("{name}/"));
            } else {
                entries.push(name);
            }
        }

        entries.sort();

        Ok(ToolOutput {
            title: format!("ls: {}", path.display()),
            output: entries.join("\n"),
            metadata: serde_json::json!({"count": entries.len()}),
            attachments: None,
        })
    }
}
