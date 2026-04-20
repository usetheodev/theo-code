use async_trait::async_trait;
use std::path::PathBuf;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    optional_string,
};

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

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "path".to_string(),
                param_type: "string".to_string(),
                description: "Directory path to list".to_string(),
                required: false,
            }],
        input_examples: Vec::new(),
    }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
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

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| ToolError::Execution(format!("{e}")))?
        {
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
            llm_suffix: None,
        })
    }
}
