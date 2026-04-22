use async_trait::async_trait;
use std::path::{Path, PathBuf};
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    optional_bool, require_string,
};

pub struct EditTool;

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl EditTool {
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

    /// Detect the dominant line ending in content
    fn detect_line_ending(content: &str) -> &'static str {
        let crlf_count = content.matches("\r\n").count();
        let lf_count = content.matches('\n').count() - crlf_count;
        if crlf_count > lf_count { "\r\n" } else { "\n" }
    }

    /// Normalize line endings to \n for comparison, then restore original
    fn normalize_for_match(text: &str) -> String {
        text.replace("\r\n", "\n")
    }

    fn apply_replacement(
        content: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> Result<String, ToolError> {
        let original_ending = Self::detect_line_ending(content);
        let normalized_content = Self::normalize_for_match(content);
        let normalized_old = Self::normalize_for_match(old_string);
        let normalized_new = Self::normalize_for_match(new_string);

        if !normalized_content.contains(&normalized_old) {
            return Err(ToolError::Execution("The old_string was not found in the file. Make sure it matches exactly.".to_string()));
        }

        let result = if replace_all {
            normalized_content.replace(&normalized_old, &normalized_new)
        } else {
            normalized_content.replacen(&normalized_old, &normalized_new, 1)
        };

        // Restore original line endings
        if original_ending == "\r\n" {
            Ok(result.replace('\n', "\r\n"))
        } else {
            Ok(result)
        }
    }

    fn compute_diff(old: &str, new: &str) -> String {
        use similar::TextDiff;
        let diff = TextDiff::from_lines(old, new);
        diff.unified_diff()
            .context_radius(3)
            .header("before", "after")
            .to_string()
    }

    fn count_diff_stats(diff: &str) -> (usize, usize) {
        let mut additions = 0;
        let mut deletions = 0;
        for line in diff.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }
        (additions, deletions)
    }
}

#[async_trait]
impl Tool for EditTool {
    fn id(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        concat!(
            "Replace `oldString` with `newString` in `filePath`. ",
            "The match must be exact (whitespace and punctuation included) and unique unless `replaceAll: true`. ",
            "Use this for small, surgical changes where you know the exact current text. ",
            "Use `write` instead when creating a new file from scratch or fully overwriting one. ",
            "Use `apply_patch` instead when a change spans many files or a large diff. ",
            "Pass an empty `oldString` to CREATE a new file with `newString` as its contents. ",
            "If the match is not unique, add more surrounding context to `oldString` OR set `replaceAll: true` deliberately. ",
            "Example: edit({filePath: 'src/lib.rs', oldString: 'pub mod old;', newString: 'pub mod new;'})."
        )
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "filePath".to_string(),
                    param_type: "string".to_string(),
                    description: "Path to the file to edit".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "oldString".to_string(),
                    param_type: "string".to_string(),
                    description: "Exact text to find and replace (must be unique in the file)"
                        .to_string(),
                    required: true,
                },
                ToolParam {
                    name: "newString".to_string(),
                    param_type: "string".to_string(),
                    description: "Replacement text".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "replaceAll".to_string(),
                    param_type: "boolean".to_string(),
                    description: "Replace all occurrences (default false)".to_string(),
                    required: false,
                },
            ],
            input_examples: vec![
                serde_json::json!({
                    "filePath": "src/lib.rs",
                    "oldString": "pub mod old;",
                    "newString": "pub mod new;"
                }),
                serde_json::json!({
                    "filePath": "src/new.rs",
                    "oldString": "",
                    "newString": "pub fn hello() {}\n"
                }),
                serde_json::json!({
                    "filePath": "README.md",
                    "oldString": "TODO",
                    "newString": "DONE",
                    "replaceAll": true
                }),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FileOps
    }

    /// Coach the agent when a common edit-call mistake shows up.
    /// Anthropic principle 8; ref: opendev `BaseTool::format_validation_error`.
    fn format_validation_error(
        &self,
        error: &ToolError,
        _args: &serde_json::Value,
    ) -> Option<String> {
        let msg = error.to_string();
        if msg.contains("filePath") {
            Some(
                "Provide `filePath` as a string (absolute or project-relative). \
                 Example: edit({filePath: 'src/lib.rs', oldString: 'pub mod old;', newString: 'pub mod new;'})."
                    .to_string(),
            )
        } else if msg.contains("oldString") || msg.contains("newString") {
            Some(
                "`oldString` and `newString` are both required strings and must differ. \
                 To CREATE a new file, pass oldString: '' and put the file content in newString. \
                 Example: edit({filePath: 'src/new.rs', oldString: '', newString: 'pub fn hello() {}'})."
                    .to_string(),
            )
        } else if msg.contains("old_string and new_string are identical") {
            Some(
                "`oldString` equals `newString` — no change would happen. \
                 If you intended to insert without removing, include the surrounding context in both so the diff is non-empty."
                    .to_string(),
            )
        } else if msg.contains("not found in the file") {
            Some(
                "The exact `oldString` is not present. Re-read the file to copy the text verbatim (whitespace, tabs, quotes matter). \
                 If the pattern appears multiple times and you want them all, set replaceAll: true."
                    .to_string(),
            )
        } else {
            None
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let file_path_str = require_string(&args, "filePath")?;
        let old_string = require_string(&args, "oldString")?;
        let new_string = require_string(&args, "newString")?;
        let replace_all = optional_bool(&args, "replaceAll").unwrap_or(false);

        // Validate old != new
        if old_string == new_string {
            return Err(ToolError::Validation(
                "old_string and new_string are identical. No changes needed.".to_string(),
            ));
        }

        let resolved = Self::resolve_path(&file_path_str, &ctx.project_dir);

        // Creating new file when old_string is empty
        if old_string.is_empty() {
            if let Some(parent) = resolved.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError::Execution(format!("Failed to create directories: {e}"))
                })?;
            }

            tokio::fs::write(&resolved, &new_string)
                .await
                .map_err(|e| ToolError::Execution(format!("Failed to write file: {e}")))?;

            let diff = Self::compute_diff("", &new_string);

            return Ok(ToolOutput {
                title: file_path_str,
                output: "Edit applied successfully (new file created)".to_string(),
                metadata: serde_json::json!({
                    "diff": diff,
                    "filediff": {
                        "file": resolved.display().to_string(),
                        "additions": new_string.lines().count(),
                        "deletions": 0,
                    },
                }),
                attachments: None,
                // Coach the model: new files often need to be wired into the
                // build (module decl, import, Cargo.toml). Follow-up reminder.
                llm_suffix: Some(
                    "New file created. If this is Rust source, add the `pub mod` line \
                     in the parent `lib.rs` / `mod.rs` so it participates in the build."
                        .to_string(),
                ),
            });
        }

        // Check file exists
        if !resolved.exists() {
            return Err(ToolError::Execution(format!(
                "File not found: {}",
                resolved.display()
            )));
        }

        // Check it's not a directory
        if resolved.is_dir() {
            return Err(ToolError::Execution(format!(
                "Cannot edit a directory: {}",
                resolved.display()
            )));
        }

        // Ask for edit permission
        permissions.record(PermissionRequest {
            permission: PermissionType::Edit,
            patterns: vec![file_path_str.clone()],
            always: vec![],
            metadata: serde_json::json!({}),
        });

        // Read current content
        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read file: {e}")))?;

        // Apply replacement
        let new_content = Self::apply_replacement(&content, &old_string, &new_string, replace_all)?;

        // Compute diff
        let diff = Self::compute_diff(&content, &new_content);
        let (additions, deletions) = Self::count_diff_stats(&diff);

        // Write file
        tokio::fs::write(&resolved, &new_content)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to write file: {e}")))?;

        let output_msg = format!(
            "Edit applied successfully (+{additions}/-{deletions} lines). \
             The file has been written to disk. You can call `done` if the task is complete."
        );

        Ok(ToolOutput {
            title: file_path_str,
            output: output_msg,
            metadata: serde_json::json!({
                "diff": diff,
                "filediff": {
                    "file": resolved.display().to_string(),
                    "additions": additions,
                    "deletions": deletions,
                },
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

    fn edit() -> EditTool {
        EditTool::new()
    }

    // --- Creating new files ---

    #[tokio::test]
    async fn creates_new_file_when_old_string_is_empty() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("newfile.txt");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "",
                    "newString": "new content",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(
            result.metadata["diff"]
                .as_str()
                .unwrap()
                .contains("new content")
        );
        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn creates_new_file_with_nested_directories() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("nested/dir/file.txt");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "",
                    "newString": "nested file",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "nested file");
    }

    // --- Editing existing files ---

    #[tokio::test]
    async fn replaces_text_in_existing_file() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("existing.txt");
        std::fs::write(&filepath, "old content here").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "old content",
                    "newString": "new content",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("Edit applied successfully"));
        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "new content here");
    }

    #[tokio::test]
    async fn throws_error_when_file_does_not_exist() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("nonexistent.txt");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "old",
                    "newString": "new",
                }),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn throws_error_when_old_equals_new() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("file.txt");
        std::fs::write(&filepath, "content").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "same",
                    "newString": "same",
                }),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("identical"));
    }

    #[tokio::test]
    async fn throws_error_when_old_string_not_found() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("file.txt");
        std::fs::write(&filepath, "actual content").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "not in file",
                    "newString": "replacement",
                }),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn replaces_all_occurrences_with_replace_all() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("file.txt");
        std::fs::write(&filepath, "foo bar foo baz foo").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "foo",
                    "newString": "qux",
                    "replaceAll": true,
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "qux bar qux baz qux");
    }

    #[tokio::test]
    async fn throws_error_when_path_is_directory() {
        let tmp = TestDir::new();
        let dirpath = tmp.path().join("adir");
        std::fs::create_dir(&dirpath).unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = edit()
            .execute(
                serde_json::json!({
                    "filePath": dirpath.to_string_lossy().to_string(),
                    "oldString": "old",
                    "newString": "new",
                }),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("directory"));
    }

    // --- Edge cases ---

    #[tokio::test]
    async fn handles_multiline_replacements() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("file.txt");
        std::fs::write(&filepath, "line1\nline2\nline3").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "line2",
                    "newString": "new line 2\nextra line",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "line1\nnew line 2\nextra line\nline3");
    }

    #[tokio::test]
    async fn handles_crlf_line_endings() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("file.txt");
        std::fs::write(&filepath, "line1\r\nold\r\nline3").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "old",
                    "newString": "new",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "line1\r\nnew\r\nline3");
    }

    #[tokio::test]
    async fn tracks_file_diff_statistics() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("file.txt");
        std::fs::write(&filepath, "line1\nline2\nline3").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "line2",
                    "newString": "new line a\nnew line b",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.metadata.get("filediff").is_some());
        assert_eq!(
            result.metadata["filediff"]["file"].as_str().unwrap(),
            filepath.to_string_lossy().as_ref()
        );
        assert!(result.metadata["filediff"]["additions"].as_u64().unwrap() > 0);
    }

    // --- Line ending preservation ---

    #[tokio::test]
    async fn preserves_lf_with_lf_multiline_strings() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("test.txt");
        std::fs::write(&filepath, "alpha\nbeta\ngamma\n").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "alpha\nbeta\ngamma",
                    "newString": "alpha\nbeta-updated\ngamma",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let output = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(output, "alpha\nbeta-updated\ngamma\n");
        assert!(!output.contains("\r\n"));
    }

    #[tokio::test]
    async fn preserves_crlf_with_crlf_multiline_strings() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("test.txt");
        std::fs::write(&filepath, "alpha\r\nbeta\r\ngamma\r\n").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "alpha\r\nbeta\r\ngamma",
                    "newString": "alpha\r\nbeta-updated\r\ngamma",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let output = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(output, "alpha\r\nbeta-updated\r\ngamma\r\n");
    }

    #[tokio::test]
    async fn preserves_lf_when_old_new_use_crlf() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("test.txt");
        std::fs::write(&filepath, "alpha\nbeta\ngamma\n").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "alpha\r\nbeta\r\ngamma",
                    "newString": "alpha\r\nbeta-updated\r\ngamma",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let output = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(output, "alpha\nbeta-updated\ngamma\n");
        assert!(!output.contains("\r\n"));
    }

    #[tokio::test]
    async fn preserves_crlf_when_old_new_use_lf() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("test.txt");
        std::fs::write(&filepath, "alpha\r\nbeta\r\ngamma\r\n").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "alpha\nbeta\ngamma",
                    "newString": "alpha\nbeta-updated\ngamma",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let output = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(output, "alpha\r\nbeta-updated\r\ngamma\r\n");
    }

    #[tokio::test]
    async fn throws_error_when_both_old_and_new_empty() {
        let tmp = TestDir::new();
        let filepath = tmp.path().join("file.txt");
        std::fs::write(&filepath, "content").unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = edit()
            .execute(
                serde_json::json!({
                    "filePath": filepath.to_string_lossy().to_string(),
                    "oldString": "",
                    "newString": "",
                }),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("identical"));
    }
}
