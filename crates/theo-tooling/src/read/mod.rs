use async_trait::async_trait;
use std::path::{Path, PathBuf};
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    FileAttachment, PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam,
    ToolSchema, optional_u64, require_string,
};

/// Known binary file extensions that should not be read as text
const BINARY_EXTENSIONS: &[&str] = &[
    "wasm", "exe", "dll", "so", "dylib", "o", "a", "lib", "bin", "dat", "db", "sqlite", "sqlite3",
    "zip", "tar", "gz", "bz2", "xz", "7z", "rar", "jar", "war", "ear", "class", "pyc", "pyo",
];

/// Image extensions that should be returned as attachments
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "tiff"];

/// Maximum characters per line before truncation
const MAX_LINE_CHARS: usize = 2000;

/// Default line limit
const DEFAULT_LIMIT: usize = 2000;

/// Max bytes for text files before truncation
const MAX_FILE_BYTES: usize = 50 * 1024;

pub struct ReadTool;

impl ReadTool {
    pub fn new() -> Self {
        Self
    }

    fn is_binary_extension(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
            .unwrap_or(false)
    }

    fn is_image_extension(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
            .unwrap_or(false)
    }

    fn contains_null_bytes(content: &[u8]) -> bool {
        content.iter().any(|&b| b == 0)
    }

    fn is_env_file(filename: &str) -> bool {
        if filename == ".env" {
            return true;
        }
        if filename.starts_with(".env.") {
            let suffix = &filename[5..];
            // .env.example is NOT a sensitive env file
            if suffix == "example" || suffix == "sample" || suffix == "template" {
                return false;
            }
            return true;
        }
        false
    }

    fn resolve_path(file_path: &str, project_dir: &Path) -> PathBuf {
        let path = PathBuf::from(file_path);
        if path.is_absolute() {
            path
        } else {
            project_dir.join(path)
        }
    }

    fn is_inside_project(path: &Path, project_dir: &Path) -> bool {
        path.starts_with(project_dir)
    }

    fn format_lines_with_numbers(content: &str, offset: usize) -> String {
        content
            .lines()
            .enumerate()
            .map(|(i, line)| {
                let line_num = offset + i;
                let truncated_line = if line.len() > MAX_LINE_CHARS {
                    format!(
                        "{} (line truncated to {MAX_LINE_CHARS} chars)",
                        &line[..MAX_LINE_CHARS]
                    )
                } else {
                    line.to_string()
                };
                format!("{line_num}: {truncated_line}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn id(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        concat!(
            "Read a file (with line numbers) or list a directory. ",
            "Use this when you need the exact contents of a known file: source, config, lock file, docs. ",
            "Supports partial reads via `offset` (1-based line number) and `limit`. ",
            "Images (PNG/JPG) return as inline attachments. ",
            "Use `glob` instead to find files by NAME pattern. ",
            "Use `grep` instead to SEARCH file contents; do NOT read every matching file to scan it yourself. ",
            "For long files, pass offset/limit to avoid large token spend; the tool will tell you how to resume. ",
            "Example: read({filePath: 'Cargo.toml'}). ",
            "Example: read({filePath: 'src/lib.rs', offset: 200, limit: 100})."
        )
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "filePath".to_string(),
                    param_type: "string".to_string(),
                    description: "Absolute or relative path to the file to read".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "offset".to_string(),
                    param_type: "integer".to_string(),
                    description: "Line number to start reading from (0-based)".to_string(),
                    required: false,
                },
                ToolParam {
                    name: "limit".to_string(),
                    param_type: "integer".to_string(),
                    description: "Maximum number of lines to read".to_string(),
                    required: false,
                },
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FileOps
    }

    fn format_validation_error(
        &self,
        error: &ToolError,
        _args: &serde_json::Value,
    ) -> Option<String> {
        let msg = error.to_string();
        if msg.contains("filePath") {
            Some(
                "Provide `filePath` as a string. Example: read({filePath: 'Cargo.toml'}) \
                 or read({filePath: 'src/lib.rs', offset: 200, limit: 100})."
                    .to_string(),
            )
        } else if msg.contains("out of range") {
            Some(
                "`offset` starts at line 1 and cannot exceed the file's total line count. \
                 Omit offset to start from the beginning, or call read once without offset to learn the total."
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
        let offset = optional_u64(&args, "offset").map(|v| v as usize);
        let limit = optional_u64(&args, "limit").map(|v| v as usize);

        let resolved = Self::resolve_path(&file_path_str, &ctx.project_dir);

        // Check external directory permission
        if !Self::is_inside_project(&resolved, &ctx.project_dir) {
            let dir = if resolved.is_dir() {
                &resolved
            } else {
                resolved.parent().unwrap_or(&resolved)
            };
            let pattern = format!("{}/*", dir.display()).replace('\\', "/");
            permissions.record(PermissionRequest {
                permission: PermissionType::ExternalDirectory,
                patterns: vec![pattern.clone()],
                always: vec![pattern],
                metadata: serde_json::json!({}),
            });
        }

        // Check if path is a directory
        if resolved.is_dir() {
            return self.read_directory(&resolved, offset, limit).await;
        }

        // Check binary extensions
        if Self::is_binary_extension(&resolved) {
            return Err(ToolError::Execution(format!(
                "Cannot read binary file: {}",
                resolved.display()
            )));
        }

        // Check image files
        if Self::is_image_extension(&resolved) {
            let bytes = tokio::fs::read(&resolved)
                .await
                .map_err(|e| ToolError::Execution(format!("Failed to read file: {e}")))?;
            let b64 = base64_encode(&bytes);
            let ext = resolved
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png");
            let mime = format!("image/{ext}");

            return Ok(ToolOutput {
                title: file_path_str,
                output: "Image file read successfully".to_string(),
                metadata: serde_json::json!({"truncated": false}),
                attachments: Some(vec![FileAttachment {
                    file_type: "file".to_string(),
                    mime: Some(mime.clone()),
                    url: format!("data:{mime};base64,{b64}"),
                }]),
                llm_suffix: None,
            });
        }

        // Check env file permissions
        if let Some(filename) = resolved.file_name().and_then(|f| f.to_str()) {
            if Self::is_env_file(filename) {
                permissions.record(PermissionRequest {
                    permission: PermissionType::Read,
                    patterns: vec![file_path_str.clone()],
                    always: vec![],
                    metadata: serde_json::json!({}),
                });
            }
        }

        // Read text file
        let bytes = tokio::fs::read(&resolved)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read file: {e}")))?;

        // Check for null bytes in text files
        if Self::contains_null_bytes(&bytes) {
            return Err(ToolError::Execution(format!(
                "Cannot read binary file: {}",
                resolved.display()
            )));
        }

        let content = String::from_utf8(bytes)
            .map_err(|_| ToolError::Execution("File is not valid UTF-8".to_string()))?;

        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len();

        // Handle empty file
        if content.is_empty() {
            if let Some(off) = offset {
                if off > 1 {
                    return Err(ToolError::Execution(format!(
                        "Offset {off} is out of range for this file (0 lines)"
                    )));
                }
            }
            return Ok(ToolOutput {
                title: file_path_str,
                output: "\nEnd of file - total 0 lines".to_string(),
                metadata: serde_json::json!({"truncated": false}),
                attachments: None,
                llm_suffix: None,
            });
        }

        let start = offset.unwrap_or(1);
        let line_limit = limit.unwrap_or(DEFAULT_LIMIT);

        // Validate offset
        if start > total_lines {
            return Err(ToolError::Execution(format!(
                "Offset {start} is out of range for this file ({total_lines} lines)"
            )));
        }

        let start_idx = start.saturating_sub(1);
        let end_idx = (start_idx + line_limit).min(total_lines);
        let selected: Vec<&str> = all_lines[start_idx..end_idx].to_vec();
        let shown = selected.len();
        let truncated = end_idx < total_lines || content.len() > MAX_FILE_BYTES;

        let formatted = Self::format_lines_with_numbers(&selected.join("\n"), start);

        let mut output = formatted;
        if truncated && limit.is_some() {
            output.push_str(&format!(
                "\n\nShowing lines {start}-{} of {total_lines}. Use offset={} to see more.",
                start + shown - 1,
                start + shown,
            ));
        } else if truncated {
            output.push_str(&format!(
                "\n\nOutput capped at {MAX_FILE_BYTES} bytes. Use offset= to read more."
            ));
        } else {
            output.push_str(&format!("\nEnd of file - total {total_lines} lines"));
        }

        // When a read is truncated, coach the model on how to resume with
        // a precise `offset`. Anthropic principle 10 (truncate with guidance).
        let llm_suffix = if truncated {
            Some(format!(
                "[read truncated] File has more content. Continue with `read(filePath, offset={}, limit=...)` to read the next window.",
                start + shown
            ))
        } else {
            None
        };

        Ok(ToolOutput {
            title: file_path_str,
            output,
            metadata: serde_json::json!({"truncated": truncated}),
            attachments: None,
            llm_suffix,
        })
    }
}

impl ReadTool {
    async fn read_directory(
        &self,
        path: &Path,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<ToolOutput, ToolError> {
        let mut entries: Vec<String> = Vec::new();
        let mut dir = tokio::fs::read_dir(path)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read directory: {e}")))?;

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read directory entry: {e}")))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            let metadata = entry.metadata().await.ok();
            let suffix = if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
                "/"
            } else {
                ""
            };
            entries.push(format!("{name}{suffix}"));
        }

        entries.sort();
        let total = entries.len();
        let start = offset.unwrap_or(1).saturating_sub(1);
        let count = limit.unwrap_or(total);
        let end = (start + count).min(total);
        let selected = &entries[start..end];
        let truncated = end < total;

        let output = selected.join("\n");
        let title = path.display().to_string();

        Ok(ToolOutput {
            title,
            output,
            metadata: serde_json::json!({"truncated": truncated}),
            attachments: None,
            llm_suffix: None,
        })
    }
}

fn base64_encode(data: &[u8]) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    let mut encoder = Base64Encoder::new(&mut buf);
    encoder.write_all(data).unwrap();
    encoder.finish();
    String::from_utf8(buf).unwrap()
}

struct Base64Encoder<'a> {
    buf: &'a mut Vec<u8>,
    pending: [u8; 3],
    pending_len: usize,
}

impl<'a> Base64Encoder<'a> {
    fn new(buf: &'a mut Vec<u8>) -> Self {
        Self {
            buf,
            pending: [0; 3],
            pending_len: 0,
        }
    }

    fn finish(mut self) {
        if self.pending_len > 0 {
            self.encode_block();
        }
    }

    fn encode_block(&mut self) {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let b = self.pending;
        match self.pending_len {
            3 => {
                self.buf.push(CHARS[(b[0] >> 2) as usize]);
                self.buf.push(CHARS[((b[0] & 3) << 4 | b[1] >> 4) as usize]);
                self.buf
                    .push(CHARS[((b[1] & 0xf) << 2 | b[2] >> 6) as usize]);
                self.buf.push(CHARS[(b[2] & 0x3f) as usize]);
            }
            2 => {
                self.buf.push(CHARS[(b[0] >> 2) as usize]);
                self.buf.push(CHARS[((b[0] & 3) << 4 | b[1] >> 4) as usize]);
                self.buf.push(CHARS[((b[1] & 0xf) << 2) as usize]);
                self.buf.push(b'=');
            }
            1 => {
                self.buf.push(CHARS[(b[0] >> 2) as usize]);
                self.buf.push(CHARS[((b[0] & 3) << 4) as usize]);
                self.buf.push(b'=');
                self.buf.push(b'=');
            }
            _ => {}
        }
        self.pending_len = 0;
    }
}

impl<'a> std::io::Write for Base64Encoder<'a> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let mut i = 0;
        while i < data.len() {
            self.pending[self.pending_len] = data[i];
            self.pending_len += 1;
            i += 1;
            if self.pending_len == 3 {
                self.encode_block();
            }
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    fn read_tool() -> ReadTool {
        ReadTool::new()
    }

    // --- External directory permission tests ---

    #[tokio::test]
    async fn allows_reading_absolute_path_inside_project() {
        let tmp = TestDir::new();
        tmp.write_file("test.txt", "hello world");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("test.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("hello world"));
    }

    #[tokio::test]
    async fn allows_reading_file_in_subdirectory() {
        let tmp = TestDir::new();
        tmp.write_file("subdir/test.txt", "nested content");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("subdir/test.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("nested content"));
    }

    #[tokio::test]
    async fn asks_external_directory_permission_for_path_outside_project() {
        let outer = TestDir::new();
        outer.write_file("secret.txt", "secret data");

        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let _ = read_tool()
            .execute(
                serde_json::json!({"filePath": outer.path().join("secret.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await;

        let ext_req = find_permission(&perms, &PermissionType::ExternalDirectory);
        assert!(ext_req.is_some());
    }

    #[tokio::test]
    async fn does_not_ask_external_directory_for_path_inside_project() {
        let tmp = TestDir::new();
        tmp.write_file("internal.txt", "internal content");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("internal.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let ext_req = find_permission(&perms, &PermissionType::ExternalDirectory);
        assert!(ext_req.is_none());
    }

    // --- Env file permission tests ---

    #[tokio::test]
    async fn env_file_asks_for_read_permission() {
        let tmp = TestDir::new();
        tmp.write_file(".env", "SECRET=value");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join(".env").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let read_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Read);
        assert!(read_req.is_some());
    }

    #[tokio::test]
    async fn env_local_asks_for_read_permission() {
        let tmp = TestDir::new();
        tmp.write_file(".env.local", "SECRET=value");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join(".env.local").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let read_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Read);
        assert!(read_req.is_some());
    }

    #[tokio::test]
    async fn env_example_does_not_ask_for_read_permission() {
        let tmp = TestDir::new();
        tmp.write_file(".env.example", "EXAMPLE=value");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join(".env.example").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let read_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Read);
        assert!(read_req.is_none());
    }

    // --- Truncation tests ---

    #[tokio::test]
    async fn truncates_by_line_count_when_limit_specified() {
        let tmp = TestDir::new();
        let lines: String = (0..100)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        tmp.write_file("many-lines.txt", &lines);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({
                    "filePath": tmp.path().join("many-lines.txt").to_string_lossy().to_string(),
                    "limit": 10
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.metadata["truncated"].as_bool().unwrap());
        assert!(result.output.contains("Showing lines 1-10 of 100"));
        assert!(result.output.contains("Use offset=11"));
        assert!(result.output.contains("line0"));
        assert!(result.output.contains("line9"));
        assert!(!result.output.contains("10: line10"));
    }

    #[tokio::test]
    async fn does_not_truncate_small_file() {
        let tmp = TestDir::new();
        tmp.write_file("small.txt", "hello world");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("small.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(!result.metadata["truncated"].as_bool().unwrap());
        assert!(result.output.contains("End of file"));
    }

    #[tokio::test]
    async fn respects_offset_parameter() {
        let tmp = TestDir::new();
        let lines: String = (1..=20)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        tmp.write_file("offset.txt", &lines);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({
                    "filePath": tmp.path().join("offset.txt").to_string_lossy().to_string(),
                    "offset": 10,
                    "limit": 5
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("line10"));
        assert!(result.output.contains("line14"));
        assert!(!result.output.contains("line15"));
    }

    #[tokio::test]
    async fn throws_when_offset_beyond_end_of_file() {
        let tmp = TestDir::new();
        let lines: String = (1..=3)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        tmp.write_file("short.txt", &lines);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({
                    "filePath": tmp.path().join("short.txt").to_string_lossy().to_string(),
                    "offset": 4,
                    "limit": 5
                }),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Offset 4 is out of range for this file (3 lines)"));
    }

    #[tokio::test]
    async fn allows_reading_empty_file() {
        let tmp = TestDir::new();
        tmp.write_file("empty.txt", "");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("empty.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(!result.metadata["truncated"].as_bool().unwrap());
        assert!(result.output.contains("End of file - total 0 lines"));
    }

    #[tokio::test]
    async fn throws_when_offset_gt_1_for_empty_file() {
        let tmp = TestDir::new();
        tmp.write_file("empty.txt", "");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({
                    "filePath": tmp.path().join("empty.txt").to_string_lossy().to_string(),
                    "offset": 2
                }),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Offset 2 is out of range")
        );
    }

    #[tokio::test]
    async fn truncates_long_lines() {
        let tmp = TestDir::new();
        let long_line = "x".repeat(3000);
        tmp.write_file("long-line.txt", &long_line);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("long-line.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("(line truncated to 2000 chars)"));
        assert!(result.output.len() < 3000);
    }

    // --- Image file tests ---

    #[tokio::test]
    async fn image_files_set_truncated_to_false() {
        let tmp = TestDir::new();
        // 1x1 red PNG
        let png_bytes: &[u8] = &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207,
            192, 0, 0, 0, 3, 0, 1, 24, 216, 141, 164, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];
        let png_path = tmp.path().join("image.png");
        std::fs::write(&png_path, png_bytes).unwrap();

        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": png_path.to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(!result.metadata["truncated"].as_bool().unwrap());
        assert!(result.attachments.is_some());
        assert_eq!(result.attachments.as_ref().unwrap().len(), 1);
        assert_eq!(result.attachments.as_ref().unwrap()[0].file_type, "file");
    }

    // --- Binary detection tests ---

    #[tokio::test]
    async fn rejects_text_extension_files_with_null_bytes() {
        let tmp = TestDir::new();
        let bytes: &[u8] = &[
            0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x00, 0x77, 0x6f, 0x72, 0x6c, 0x64,
        ];
        let path = tmp.path().join("null-byte.txt");
        std::fs::write(&path, bytes).unwrap();

        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": path.to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Cannot read binary file")
        );
    }

    #[tokio::test]
    async fn rejects_known_binary_extensions() {
        let tmp = TestDir::new();
        tmp.write_file("module.wasm", "not really wasm");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("module.wasm").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Cannot read binary file")
        );
    }

    #[tokio::test]
    async fn fbs_files_read_as_text_not_images() {
        let tmp = TestDir::new();
        let fbs_content = "namespace MyGame;\n\ntable Monster {\n  pos:Vec3;\n  name:string;\n}\n\nroot_type Monster;";
        tmp.write_file("schema.fbs", fbs_content);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("schema.fbs").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.attachments.is_none());
        assert!(result.output.contains("namespace MyGame"));
        assert!(result.output.contains("table Monster"));
    }
}
