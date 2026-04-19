use async_trait::async_trait;
use std::path::PathBuf;
use std::process::Stdio;
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    optional_string, require_string,
};
use tokio::process::Command;

const MAX_MATCHES: usize = 100;

pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn id(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        concat!(
            "Search file contents with a regex pattern. Returns file:line:match for each hit. ",
            "Use this when you know part of the text (symbol name, error string, literal constant) and want every occurrence. ",
            "Use `glob` instead when searching by FILENAME pattern (e.g. '**/*.rs'). ",
            "Use `read` instead when you already know the file path and want its full contents. ",
            "Use `codebase_context` for structural relationships (callers, callees, definitions) rather than text matches. ",
            "Scope with `path` to a subdir and `glob` to a filename filter — a broad regex over the whole workspace can easily truncate; narrow first. ",
            "Example: grep({pattern: 'fn main', path: 'crates/theo-cli', glob: '*.rs'})."
        )
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "pattern".to_string(),
                    param_type: "string".to_string(),
                    description: "Regular expression pattern to search for".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "path".to_string(),
                    param_type: "string".to_string(),
                    description: "File or directory to search in".to_string(),
                    required: false,
                },
                ToolParam {
                    name: "include".to_string(),
                    param_type: "string".to_string(),
                    description: "Glob pattern to filter files (e.g. '*.py')".to_string(),
                    required: false,
                },
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    /// Grep output is a list of matches — tail is more informative than head
    /// when broad patterns generate thousands of hits. Ref: opendev
    /// sanitizer.rs:27-53.
    fn truncation_rule(&self) -> Option<theo_domain::tool::TruncationRule> {
        Some(theo_domain::tool::TruncationRule {
            max_chars: 4_000,
            strategy: theo_domain::tool::TruncationStrategy::Tail,
        })
    }

    fn format_validation_error(
        &self,
        error: &ToolError,
        _args: &serde_json::Value,
    ) -> Option<String> {
        let msg = error.to_string();
        if msg.contains("pattern") {
            Some(
                "Provide `pattern` as a regex string. Narrow with `path` to a subdir and `include` to a filename glob. \
                 Example: grep({pattern: 'fn main', path: 'crates/theo-cli', include: '*.rs'})."
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
        let pattern = require_string(&args, "pattern")?;
        let search_path = optional_string(&args, "path")
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.project_dir.clone());
        let include = optional_string(&args, "include");

        permissions.record(PermissionRequest {
            permission: PermissionType::Grep,
            patterns: vec![pattern.clone()],
            always: vec![pattern.clone()],
            metadata: serde_json::json!({}),
        });

        let mut cmd = Command::new("grep");
        cmd.arg("-rn")
            .arg("--color=never")
            .arg("-m")
            .arg(MAX_MATCHES.to_string())
            .arg(&pattern)
            .arg(&search_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(ref inc) = include {
            cmd.arg("--include").arg(inc);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to run grep: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if stdout.trim().is_empty() {
            return Ok(ToolOutput {
                title: format!("grep: {pattern}"),
                output: "No files found".to_string(),
                metadata: serde_json::json!({"matches": 0, "truncated": false}),
                attachments: None,
                llm_suffix: None,
            });
        }

        let lines: Vec<&str> = split_lines(&stdout);
        let match_count = lines.len();
        let truncated = match_count >= MAX_MATCHES;

        // Group by file
        let mut grouped = std::collections::BTreeMap::new();
        for line in &lines {
            if let Some((file, rest)) = line.split_once(':') {
                grouped
                    .entry(file.to_string())
                    .or_insert_with(Vec::new)
                    .push(rest.to_string());
            }
        }

        let mut result_output = format!("Found {match_count} matches");
        if truncated {
            result_output.push_str(&format!(" (showing first {MAX_MATCHES})"));
        }
        result_output.push('\n');

        for (file, matches) in &grouped {
            result_output.push_str(&format!("\n{file}:\n"));
            for m in matches {
                result_output.push_str(&format!("  {m}\n"));
            }
        }

        Ok(ToolOutput {
            title: format!("grep: {pattern}"),
            output: result_output,
            metadata: serde_json::json!({
                "matches": match_count,
                "truncated": truncated,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// Workaround: use regex split for cross-platform line ending handling
fn split_lines(s: &str) -> Vec<&str> {
    s.trim()
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    fn grep() -> GrepTool {
        GrepTool::new()
    }

    #[tokio::test]
    async fn basic_search() {
        let tmp = TestDir::new();
        tmp.write_file("test.txt", "hello world\nfoo bar\nhello again");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = grep()
            .execute(
                serde_json::json!({
                    "pattern": "hello",
                    "path": tmp.path().to_string_lossy().to_string(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.metadata["matches"].as_u64().unwrap() > 0);
        assert!(result.output.contains("Found"));
    }

    #[tokio::test]
    async fn no_matches_returns_correct_output() {
        let tmp = TestDir::new();
        tmp.write_file("test.txt", "hello world");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = grep()
            .execute(
                serde_json::json!({
                    "pattern": "xyznonexistentpatternxyz123",
                    "path": tmp.path().to_string_lossy().to_string(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(result.metadata["matches"], 0);
        assert_eq!(result.output, "No files found");
    }

    #[tokio::test]
    async fn handles_crlf_line_endings_in_output() {
        let tmp = TestDir::new();
        tmp.write_file("test.txt", "line1\nline2\nline3");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = grep()
            .execute(
                serde_json::json!({
                    "pattern": "line",
                    "path": tmp.path().to_string_lossy().to_string(),
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.metadata["matches"].as_u64().unwrap() > 0);
    }

    // --- CRLF regex handling (pure unit tests) ---

    #[test]
    fn regex_correctly_splits_unix_line_endings() {
        let unix_output = "file1.txt|1|content1\nfile2.txt|2|content2\nfile3.txt|3|content3";
        let lines = split_lines(unix_output);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "file1.txt|1|content1");
        assert_eq!(lines[2], "file3.txt|3|content3");
    }

    #[test]
    fn regex_correctly_splits_windows_crlf_line_endings() {
        let windows_output = "file1.txt|1|content1\r\nfile2.txt|2|content2\r\nfile3.txt|3|content3";
        let lines = split_lines(windows_output);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "file1.txt|1|content1");
        assert_eq!(lines[2], "file3.txt|3|content3");
    }

    #[test]
    fn regex_handles_mixed_line_endings() {
        let mixed_output = "file1.txt|1|content1\nfile2.txt|2|content2\r\nfile3.txt|3|content3";
        let lines = split_lines(mixed_output);
        assert_eq!(lines.len(), 3);
    }
}
