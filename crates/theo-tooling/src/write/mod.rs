use async_trait::async_trait;
use std::path::{Path, PathBuf};
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    require_string,
};

pub struct WriteTool;

impl Default for WriteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteTool {
    pub fn new() -> Self {
        Self
    }

    /// Canonicalize the user-supplied path against `project_dir`.
    ///
    /// Mirrors `ReadTool::resolve_path`: uses `crate::path::absolutize` so
    /// `..` traversal, symlinks, and redundant separators cannot bypass
    /// the `is_inside_project` check downstream (T2.3).
    ///
    /// Falls back to the raw join when canonicalization fails (e.g. the
    /// target path does not exist yet AND its parent directory also does
    /// not exist). The subsequent `tokio::fs::create_dir_all` + `write`
    /// will still fail loudly with a clearer error in that edge case.
    fn resolve_path(file_path: &str, project_dir: &Path) -> PathBuf {
        match crate::path::absolutize(project_dir, file_path) {
            Ok(canonical) => canonical,
            Err(_) => {
                let path = PathBuf::from(file_path);
                if path.is_absolute() {
                    path
                } else {
                    project_dir.join(path)
                }
            }
        }
    }

    /// Canonical-root comparison via [`crate::path::is_contained`]. Falls
    /// back to `starts_with` if either side cannot be canonicalized.
    fn is_inside_project(path: &Path, project_dir: &Path) -> bool {
        crate::path::is_contained(path, project_dir)
            .unwrap_or_else(|_| path.starts_with(project_dir))
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

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "filePath".to_string(),
                    param_type: "string".to_string(),
                    description: "Absolute or relative path to the file to write".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "content".to_string(),
                    param_type: "string".to_string(),
                    description: "The complete content to write to the file".to_string(),
                    required: true,
                },
            ],
        input_examples: Vec::new(),
    }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FileOps
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

        // Ask for an ExternalDirectory permission BEFORE creating dirs if
        // the target is outside the workspace. Mirrors ReadTool's guard and
        // closes a hole where `write("../outside.txt", …)` would silently
        // create a file next to the project root (T2.3).
        if !Self::is_inside_project(&resolved, &ctx.project_dir) {
            let dir = resolved.parent().unwrap_or(&resolved);
            let pattern = format!("{}/*", dir.display()).replace('\\', "/");
            permissions.record(PermissionRequest {
                permission: PermissionType::ExternalDirectory,
                patterns: vec![pattern.clone()],
                always: vec![pattern],
                metadata: serde_json::json!({}),
            });
        }

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

        // Coach the model after creating a new file: builds usually need
        // the new module wired into its parent.
        let llm_suffix = if !exists {
            Some(
                "New file created. If it is a Rust source file, add `pub mod <name>;` in the \
                 parent `lib.rs` / `mod.rs` so it participates in the build."
                    .to_string(),
            )
        } else {
            None
        };

        Ok(ToolOutput {
            title,
            output: format!(
                "Wrote file successfully ({} bytes). \
                 The file is on disk. You can call `done` if the task is complete.",
                content.len()
            ),
            metadata: serde_json::json!({
                "filepath": resolved.display().to_string(),
                "exists": exists,
            }),
            attachments: None,
            llm_suffix,
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

    // --- Path traversal / external directory permission (T2.3) ---

    #[tokio::test]
    async fn rejects_silent_escape_via_parent_dir_traversal() {
        // `workspace/sub/../../outside.txt` resolves to a file next to
        // the workspace root. Before T2.3 this wrote without any
        // permission prompt; now it must record an ExternalDirectory
        // permission request.
        let tmp = TestDir::new();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let _ = write_tool()
            .execute(
                serde_json::json!({
                    "filePath": "sub/../../outside_via_traversal.txt",
                    "content": "payload",
                }),
                &ctx,
                &mut perms,
            )
            .await;

        let ext = find_permission(&perms, &PermissionType::ExternalDirectory);
        assert!(
            ext.is_some(),
            "write MUST request ExternalDirectory when resolved path escapes the workspace via ../",
        );
    }

    #[tokio::test]
    async fn does_not_record_external_permission_for_in_project_paths() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("inside.txt");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        write_tool()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "content": "hi",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(
            find_permission(&perms, &PermissionType::ExternalDirectory).is_none(),
            "in-project write MUST NOT record ExternalDirectory permission"
        );
    }

    #[tokio::test]
    async fn absolutize_makes_is_inside_project_honest_under_symlink_escape() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let tmp = TestDir::new();
            let outside = TestDir::new();
            let ctx = test_context(tmp.path());
            let mut perms = PermissionCollector::new();

            // Create a symlink inside the project that points outside.
            let link = tmp.path().join("back_door");
            symlink(outside.path(), &link).unwrap();

            let _ = write_tool()
                .execute(
                    serde_json::json!({
                        "filePath": "back_door/captured.txt",
                        "content": "secret",
                    }),
                    &ctx,
                    &mut perms,
                )
                .await;

            // Canonical comparison resolves the symlink and correctly
            // classifies the target as external — permission is required.
            assert!(
                find_permission(&perms, &PermissionType::ExternalDirectory).is_some(),
                "symlink-to-outside MUST require ExternalDirectory permission"
            );
        }
    }
}
