use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolContext, ToolOutput, require_string,
};
use std::path::{Path, PathBuf};

pub struct WriteTool;

impl WriteTool {
    pub fn new() -> Self {
        Self
    }

    fn resolve_path(file_path: &str, project_dir: &Path) -> PathBuf {
        let path = PathBuf::from(file_path);
        if path.is_absolute() {
            path
        } else {
            project_dir.join(path)
        }
    }

    fn relative_path(absolute: &Path, project_dir: &Path) -> String {
        absolute
            .strip_prefix(project_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| absolute.display().to_string())
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn id(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file"
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let file_path_str = require_string(&args, "filePath")?;
        let content = require_string(&args, "content")?;

        let resolved = Self::resolve_path(&file_path_str, &ctx.project_dir);
        let exists = resolved.exists();

        // Create parent directories if needed
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::Execution(format!("Failed to create directories: {e}")))?;
        }

        // Ask for edit permission
        let rel = Self::relative_path(&resolved, &ctx.project_dir);
        permissions.record(PermissionRequest {
            permission: PermissionType::Edit,
            patterns: vec![rel.clone()],
            always: vec![],
            metadata: serde_json::json!({}),
        });

        // Write the file
        tokio::fs::write(&resolved, &content)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to write file: {e}")))?;

        let title = Self::relative_path(&resolved, &ctx.project_dir);

        Ok(ToolOutput {
            title,
            output: "Wrote file successfully".to_string(),
            metadata: serde_json::json!({
                "filepath": resolved.display().to_string(),
                "exists": exists,
            }),
            attachments: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    fn write_tool() -> WriteTool {
        WriteTool::new()
    }

    // --- New file creation ---

    #[tokio::test]
    async fn writes_content_to_new_file() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("newfile.txt");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": "Hello, World!",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("Wrote file successfully"));
        assert_eq!(result.metadata["exists"], false);
        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "Hello, World!");
    }

    #[tokio::test]
    async fn creates_parent_directories_if_needed() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("nested/deep/file.txt");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": "nested content",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "nested content");
    }

    #[tokio::test]
    async fn handles_relative_paths_resolving_to_project_dir() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        write_tool()
            .execute(
                serde_json::json!({
                    "filePath": "relative.txt",
                    "content": "relative content",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(tmp.path().join("relative.txt")).unwrap();
        assert_eq!(content, "relative content");
    }

    // --- Existing file overwrite ---

    #[tokio::test]
    async fn overwrites_existing_file_content() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("existing.txt");
        std::fs::write(&filepath, "old content").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": "new content",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("Wrote file successfully"));
        assert_eq!(result.metadata["exists"], true);
        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn returns_filepath_in_metadata_for_existing_files() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("file.txt");
        std::fs::write(&filepath, "old").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": "new",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(
            result.metadata["filepath"].as_str().unwrap(),
            filepath.to_string_lossy().as_ref()
        );
        assert_eq!(result.metadata["exists"], true);
    }

    // --- Content types ---

    #[tokio::test]
    async fn writes_json_content() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("data.json");
        let data = serde_json::json!({"key": "value", "nested": {"array": [1, 2, 3]}});
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": serde_json::to_string_pretty(&data).unwrap(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(&filepath).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed, data);
    }

    #[tokio::test]
    async fn writes_empty_content() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("empty.txt");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": "",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "");
        assert_eq!(std::fs::metadata(&filepath).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn writes_multiline_content() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("multiline.txt");
        let lines = "Line 1\nLine 2\nLine 3\n";
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": lines,
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, lines);
    }

    #[tokio::test]
    async fn handles_different_line_endings() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("crlf.txt");
        let content = "Line 1\r\nLine 2\r\nLine 3";
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": content,
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let buf = std::fs::read(&filepath).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), content);
    }

    // --- Error handling ---

    #[tokio::test]
    async fn throws_error_when_os_denies_write_access() {
        let tmp = TestDir::new();
        let readonly_path = tmp.path().join("readonly.txt");
        std::fs::write(&readonly_path, "test").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&readonly_path, std::fs::Permissions::from_mode(0o444))
                .unwrap();
        }

        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = write_tool()
            .execute(
                serde_json::json!({
                    "filePath": readonly_path.to_string_lossy().to_string(),
                    "content": "new content",
                }),
                &ctx,
                &mut perms,
            )
            .await;

        #[cfg(unix)]
        assert!(result.is_err());
    }

    // --- Title generation ---

    #[tokio::test]
    async fn returns_relative_path_as_title() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("src/components/Button.tsx");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": "export const Button = () => {}",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.title.ends_with("src/components/Button.tsx"));
    }
}
