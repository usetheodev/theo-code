use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolContext, ToolOutput, optional_string, optional_u64,
    require_string,
};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

pub struct BashTool;

impl BashTool {
    pub fn new() -> Self {
        Self
    }

    /// Parse commands from a compound command string (e.g., "echo foo && echo bar")
    fn parse_commands(command: &str) -> Vec<String> {
        command
            .split("&&")
            .flat_map(|s| s.split("||"))
            .flat_map(|s| s.split(';'))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Check if a command is cd-only (just changes directory)
    fn is_cd_only(command: &str) -> bool {
        let trimmed = command.trim();
        trimmed == "cd" || trimmed.starts_with("cd ") || trimmed.starts_with("cd\t")
    }

    /// Detect external file paths referenced in a command
    fn detect_external_paths(command: &str, project_dir: &Path) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        // Simple heuristic: look for absolute paths in the command
        for token in command.split_whitespace() {
            let token = token.trim_matches(|c: char| c == '\'' || c == '"');
            if token.starts_with('/') || token.starts_with("~/") {
                let path = if token.starts_with("~/") {
                    if let Ok(home) = std::env::var("HOME") {
                        PathBuf::from(home).join(&token[2..])
                    } else {
                        PathBuf::from(token)
                    }
                } else {
                    PathBuf::from(token)
                };
                if !path.starts_with(project_dir) {
                    paths.push(path);
                }
            }
        }
        paths
    }

    /// Generate the always-allow pattern for a command
    fn always_pattern(command: &str) -> String {
        let first_word = command.split_whitespace().next().unwrap_or("");
        format!("{first_word} *")
    }
}

#[async_trait]
impl Tool for BashTool {
    fn id(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command"
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let command = require_string(&args, "command")?;
        let description = require_string(&args, "description")?;
        let _timeout_ms = optional_u64(&args, "timeout").unwrap_or(120_000);
        let workdir = optional_string(&args, "workdir");

        let commands = Self::parse_commands(&command);

        // Check for cd-only commands
        let all_cd = commands.iter().all(|c| Self::is_cd_only(c));
        if !all_cd {
            // Ask for bash permission
            let patterns: Vec<String> = commands.clone();
            let always: Vec<String> = commands.iter().map(|c| Self::always_pattern(c)).collect();

            permissions.record(PermissionRequest {
                permission: PermissionType::Bash,
                patterns,
                always,
                metadata: serde_json::json!({}),
            });
        }

        // Check for external directory access
        let effective_workdir = workdir.as_deref().unwrap_or("");
        let project_dir = &ctx.project_dir;

        if !effective_workdir.is_empty() {
            let wd_path = PathBuf::from(effective_workdir);
            if !wd_path.starts_with(project_dir) {
                let pattern = format!("{}/*", wd_path.display());
                permissions.record(PermissionRequest {
                    permission: PermissionType::ExternalDirectory,
                    patterns: vec![pattern.clone()],
                    always: vec![pattern],
                    metadata: serde_json::json!({}),
                });
            }
        }

        // Check for cd ../ in commands
        for cmd in &commands {
            if cmd.starts_with("cd ../") || cmd == "cd .." || cmd == "cd ../" {
                permissions.record(PermissionRequest {
                    permission: PermissionType::ExternalDirectory,
                    patterns: vec!["../*".to_string()],
                    always: vec!["../*".to_string()],
                    metadata: serde_json::json!({}),
                });
            }
        }

        // Check for external file paths
        let external_paths = Self::detect_external_paths(&command, project_dir);
        for path in &external_paths {
            let parent = path.parent().unwrap_or(path);
            let pattern = format!("{}/*", parent.display());
            permissions.record(PermissionRequest {
                permission: PermissionType::ExternalDirectory,
                patterns: vec![pattern.clone()],
                always: vec![pattern],
                metadata: serde_json::json!({}),
            });
        }

        // Execute the command
        let cwd = if let Some(ref wd) = workdir {
            PathBuf::from(wd)
        } else {
            project_dir.clone()
        };

        let output = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .current_dir(&cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to execute command: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        let combined = if stderr.is_empty() {
            stdout.clone()
        } else {
            format!("{stdout}{stderr}")
        };

        // Truncate output
        let truncated_result =
            theo_domain::truncate::truncate_output(&combined, None);

        Ok(ToolOutput {
            title: description,
            output: truncated_result.content,
            metadata: serde_json::json!({
                "output": combined,
                "exit": exit_code,
                "description": require_string(&args, "description").unwrap_or_default(),
                "truncated": truncated_result.truncated,
                "outputPath": truncated_result.output_path,
            }),
            attachments: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    fn bash() -> BashTool {
        BashTool::new()
    }

    #[tokio::test]
    async fn basic_execution() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = bash()
            .execute(
                serde_json::json!({
                    "command": "echo 'test'",
                    "description": "Echo test message",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(result.metadata["exit"], 0);
        assert!(result.metadata["output"].as_str().unwrap().contains("test"));
    }

    #[tokio::test]
    async fn asks_for_bash_permission_with_correct_pattern() {
        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        bash()
            .execute(
                serde_json::json!({
                    "command": "echo hello",
                    "description": "Echo hello",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let bash_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Bash);
        assert!(bash_req.is_some());
        assert!(bash_req.unwrap().patterns.contains(&"echo hello".to_string()));
    }

    #[tokio::test]
    async fn asks_for_bash_permission_with_multiple_commands() {
        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        bash()
            .execute(
                serde_json::json!({
                    "command": "echo foo && echo bar",
                    "description": "Echo twice",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let bash_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Bash)
            .unwrap();
        assert!(bash_req.patterns.contains(&"echo foo".to_string()));
        assert!(bash_req.patterns.contains(&"echo bar".to_string()));
    }

    #[tokio::test]
    async fn asks_for_external_directory_permission_when_cd_to_parent() {
        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        bash()
            .execute(
                serde_json::json!({
                    "command": "cd ../",
                    "description": "Change to parent directory",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let ext_req = find_permission(&perms, &PermissionType::ExternalDirectory);
        assert!(ext_req.is_some());
    }

    #[tokio::test]
    async fn asks_for_external_directory_when_workdir_outside_project() {
        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();
        let tmp_dir = std::env::temp_dir();

        bash()
            .execute(
                serde_json::json!({
                    "command": "ls",
                    "workdir": tmp_dir.to_string_lossy(),
                    "description": "List temp dir",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let ext_req = find_permission(&perms, &PermissionType::ExternalDirectory);
        assert!(ext_req.is_some());
        let pattern = format!("{}/*", tmp_dir.display());
        assert!(ext_req.unwrap().patterns.contains(&pattern));
    }

    #[tokio::test]
    async fn asks_for_external_directory_when_file_arg_outside_project() {
        let outer = TestDir::new();
        outer.write_file("outside.txt", "x");

        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let filepath = outer.path().join("outside.txt");
        bash()
            .execute(
                serde_json::json!({
                    "command": format!("cat {}", filepath.display()),
                    "description": "Read external file",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let ext_req = find_permission(&perms, &PermissionType::ExternalDirectory);
        assert!(ext_req.is_some());
        let expected = format!("{}/*", outer.path().display());
        assert!(ext_req.unwrap().patterns.contains(&expected));
        assert!(ext_req.unwrap().always.contains(&expected));
    }

    #[tokio::test]
    async fn does_not_ask_external_directory_for_rm_inside_project() {
        let tmp = TestDir::with_git();
        tmp.write_file("tmpfile", "x");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let nested_path = tmp.path().join("nested");
        bash()
            .execute(
                serde_json::json!({
                    "command": format!("rm -rf {}", nested_path.display()),
                    "description": "remove nested dir",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let ext_req = find_permission(&perms, &PermissionType::ExternalDirectory);
        assert!(ext_req.is_none());
    }

    #[tokio::test]
    async fn includes_always_patterns_for_auto_approval() {
        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        bash()
            .execute(
                serde_json::json!({
                    "command": "git log --oneline -5",
                    "description": "Git log",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let bash_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Bash)
            .unwrap();
        assert!(!bash_req.always.is_empty());
        assert!(bash_req.always.iter().any(|p| p.ends_with('*')));
    }

    #[tokio::test]
    async fn does_not_ask_bash_permission_for_cd_only() {
        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        bash()
            .execute(
                serde_json::json!({
                    "command": "cd .",
                    "description": "Stay in current directory",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let bash_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Bash);
        assert!(bash_req.is_none());
    }

    #[tokio::test]
    async fn matches_redirects_in_permission_pattern() {
        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        bash()
            .execute(
                serde_json::json!({
                    "command": "cat > /tmp/output.txt",
                    "description": "Redirect ls output",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let bash_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Bash)
            .unwrap();
        assert!(bash_req.patterns.contains(&"cat > /tmp/output.txt".to_string()));
    }

    #[tokio::test]
    async fn always_pattern_has_space_before_wildcard() {
        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        bash()
            .execute(
                serde_json::json!({
                    "command": "ls -la",
                    "description": "List",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let bash_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Bash)
            .unwrap();
        assert_eq!(bash_req.always[0], "ls *");
    }

    #[tokio::test]
    async fn truncates_output_exceeding_line_limit() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let line_count = theo_domain::truncate::MAX_LINES + 500;
        let result = bash()
            .execute(
                serde_json::json!({
                    "command": format!("seq 1 {line_count}"),
                    "description": "Generate lines exceeding limit",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(result.metadata["truncated"], true);
        assert!(result.output.contains("truncated"));
        assert!(result.output.contains("The tool call succeeded but the output was truncated"));
    }

    #[tokio::test]
    async fn truncates_output_exceeding_byte_limit() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let byte_count = theo_domain::truncate::MAX_BYTES + 10000;
        let result = bash()
            .execute(
                serde_json::json!({
                    "command": format!("head -c {byte_count} /dev/zero | tr '\\0' 'a'"),
                    "description": "Generate bytes exceeding limit",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(result.metadata["truncated"], true);
        assert!(result.output.contains("truncated"));
    }

    #[tokio::test]
    async fn does_not_truncate_small_output() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = bash()
            .execute(
                serde_json::json!({
                    "command": "echo hello",
                    "description": "Echo hello",
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(result.metadata["truncated"], false);
        assert!(result.output.contains("hello"));
    }
}
