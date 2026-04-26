use async_trait::async_trait;
use std::path::PathBuf;
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    optional_string,
};

pub struct LsTool;

impl Default for LsTool {
    fn default() -> Self {
        Self::new()
    }
}

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
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        // T2.3: canonicalise the user-supplied path against the project
        // directory before touching the filesystem. Prevents a `..`-based
        // escape from reading arbitrary directories without an
        // `ExternalDirectory` permission.
        let raw = optional_string(&args, "path");
        let path: PathBuf = match raw {
            Some(p) => match crate::path::absolutize(&ctx.project_dir, &p) {
                Ok(canonical) => canonical,
                Err(_) => PathBuf::from(p),
            },
            None => ctx.project_dir.clone(),
        };

        let inside = crate::path::is_contained(&path, &ctx.project_dir)
            .unwrap_or_else(|_| path.starts_with(&ctx.project_dir));
        if !inside {
            let pattern = format!("{}/*", path.display()).replace('\\', "/");
            permissions.record(PermissionRequest {
                permission: PermissionType::ExternalDirectory,
                patterns: vec![pattern.clone()],
                always: vec![pattern],
                metadata: serde_json::json!({}),
            });
        }

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
