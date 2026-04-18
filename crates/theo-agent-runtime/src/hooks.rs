//! Hooks system — external scripts that execute on agent events.
//!
//! Hooks are shell scripts in `.theo/hooks/` (project) and `~/.config/theo/hooks/` (global).
//! They receive event data as JSON via stdin and can block execution (pre-hooks)
//! or fire-and-forget (post-hooks).
//!
//! Hook naming convention:
//!   - `tool.before.sh` — runs before every tool call. Exit != 0 blocks execution.
//!   - `tool.after.sh` — runs after every tool call. Fire-and-forget.
//!   - `run.start.sh` — runs when agent starts.
//!   - `run.end.sh` — runs when agent ends.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// HookEvent — data passed to hook scripts via stdin
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct HookEvent {
    pub hook_type: String,
    pub tool_name: Option<String>,
    pub tool_args: Option<serde_json::Value>,
    pub project_dir: String,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// HookResult — what the hook returned
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HookResult {
    pub allowed: bool,
    pub output: String,
    pub exit_code: i32,
}

// ---------------------------------------------------------------------------
// HookConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct HookConfig {
    /// Timeout per hook execution in seconds. Default: 5, max: 30.
    #[serde(default = "default_hook_timeout")]
    pub timeout_secs: u64,
    /// Whether to load hooks from .theo/hooks/ (project-level).
    /// Set to false to disable project hooks (security).
    #[serde(default = "default_true")]
    pub project_hooks_enabled: bool,
}

fn default_hook_timeout() -> u64 {
    5
}
fn default_true() -> bool {
    true
}

impl Default for HookConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 5,
            project_hooks_enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// HookRunner — discovers and executes hooks
// ---------------------------------------------------------------------------

pub struct HookRunner {
    hooks_dirs: Vec<PathBuf>,
    config: HookConfig,
}

impl HookRunner {
    /// Create a HookRunner that discovers hooks from project and global dirs.
    pub fn new(project_dir: &Path, config: HookConfig) -> Self {
        let mut hooks_dirs = Vec::new();

        // Project hooks (if enabled)
        if config.project_hooks_enabled {
            let project_hooks = project_dir.join(".theo").join("hooks");
            if project_hooks.exists() {
                hooks_dirs.push(project_hooks);
            }
        }

        // Global hooks
        if let Ok(home) = std::env::var("HOME") {
            let global_hooks = PathBuf::from(home)
                .join(".config")
                .join("theo")
                .join("hooks");
            if global_hooks.exists() {
                hooks_dirs.push(global_hooks);
            }
        }

        Self { hooks_dirs, config }
    }

    /// Find hook scripts matching a hook type (e.g., "tool.before").
    fn find_hooks(&self, hook_type: &str) -> Vec<PathBuf> {
        let filename = format!("{hook_type}.sh");
        self.hooks_dirs
            .iter()
            .map(|dir| dir.join(&filename))
            .filter(|path| path.exists() && path.is_file())
            .collect()
    }

    /// Execute a pre-hook. Returns HookResult. If exit code != 0, the caller should block.
    pub async fn run_pre_hook(&self, hook_type: &str, event: &HookEvent) -> HookResult {
        let scripts = self.find_hooks(hook_type);
        if scripts.is_empty() {
            return HookResult {
                allowed: true,
                output: String::new(),
                exit_code: 0,
            };
        }

        let event_json = serde_json::to_string(event).unwrap_or_default();
        let timeout = Duration::from_secs(self.config.timeout_secs.min(30));

        for script in &scripts {
            match self.execute_script(script, &event_json, timeout).await {
                Ok(result) if !result.allowed => return result,
                Ok(_) => continue,
                Err(e) => {
                    return HookResult {
                        allowed: false,
                        output: format!("Hook error: {e}"),
                        exit_code: -1,
                    };
                }
            }
        }

        HookResult {
            allowed: true,
            output: String::new(),
            exit_code: 0,
        }
    }

    /// Execute a post-hook. Fire-and-forget — errors are logged but don't affect execution.
    pub async fn run_post_hook(&self, hook_type: &str, event: &HookEvent) {
        let scripts = self.find_hooks(hook_type);
        if scripts.is_empty() {
            return;
        }

        let event_json = serde_json::to_string(event).unwrap_or_default();
        let timeout = Duration::from_secs(self.config.timeout_secs.min(30));

        for script in &scripts {
            if let Err(e) = self.execute_script(script, &event_json, timeout).await {
                eprintln!("[theo] Hook error ({}): {e}", script.display());
            }
        }
    }

    /// Execute a single hook script with timeout.
    async fn execute_script(
        &self,
        script: &Path,
        stdin_data: &str,
        timeout: Duration,
    ) -> Result<HookResult, String> {
        use tokio::process::Command;

        let mut cmd = Command::new("sh");
        cmd.arg(script);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn hook: {e}"))?;

        // Write stdin
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(stdin_data.as_bytes()).await;
            drop(stdin);
        }

        // Wait with timeout
        let timeout_dur = timeout;
        match tokio::time::timeout(timeout_dur, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let exit_code = output.status.code().unwrap_or(-1);
                Ok(HookResult {
                    allowed: exit_code == 0,
                    output: stdout,
                    exit_code,
                })
            }
            Ok(Err(e)) => Err(format!("Hook execution failed: {e}")),
            Err(_) => {
                // child was consumed by wait_with_output future — process will be killed on drop
                Err(format!("Hook timed out after {}s", timeout_dur.as_secs()))
            }
        }
    }

    /// Find hook scripts matching a hook type. Public for sensor integration.
    pub fn find_hooks_for_type(&self, hook_type: &str) -> Vec<PathBuf> {
        self.find_hooks(hook_type)
    }

    /// Execute a sensor hook and return the result (captures output unlike post-hooks).
    ///
    /// Similar to pre-hooks but does not block on failure — always returns a result.
    pub async fn run_sensor_hook(&self, hook_type: &str, event: &HookEvent) -> HookResult {
        let scripts = self.find_hooks(hook_type);
        if scripts.is_empty() {
            return HookResult {
                allowed: true,
                output: String::new(),
                exit_code: 0,
            };
        }

        let event_json = serde_json::to_string(event).unwrap_or_default();
        let timeout = Duration::from_secs(self.config.timeout_secs.min(30));

        let mut combined_output = String::new();
        let mut last_exit_code = 0;

        for script in &scripts {
            match self.execute_script(script, &event_json, timeout).await {
                Ok(result) => {
                    if !result.output.is_empty() {
                        combined_output.push_str(&result.output);
                    }
                    last_exit_code = result.exit_code;
                }
                Err(e) => {
                    combined_output.push_str(&format!("Sensor error: {e}\n"));
                    last_exit_code = -1;
                }
            }
        }

        HookResult {
            allowed: last_exit_code == 0,
            output: combined_output,
            exit_code: last_exit_code,
        }
    }

    /// Check if any hooks are registered.
    pub fn has_hooks(&self) -> bool {
        !self.hooks_dirs.is_empty()
    }

    /// List all discovered hook files.
    pub fn list_hooks(&self) -> Vec<(String, PathBuf)> {
        let mut hooks = Vec::new();
        for dir in &self.hooks_dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("sh") {
                        let name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        hooks.push((name, path));
                    }
                }
            }
        }
        hooks
    }
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Create a HookEvent for a tool execution.
pub fn tool_hook_event(
    hook_type: &str,
    tool_name: &str,
    tool_args: &serde_json::Value,
    project_dir: &Path,
) -> HookEvent {
    HookEvent {
        hook_type: hook_type.to_string(),
        tool_name: Some(tool_name.to_string()),
        tool_args: Some(tool_args.clone()),
        project_dir: project_dir.display().to_string(),
        timestamp: now_millis(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_config_defaults() {
        let config = HookConfig::default();
        assert_eq!(config.timeout_secs, 5);
        assert!(config.project_hooks_enabled);
    }

    #[test]
    fn hook_runner_no_hooks_dir_is_empty() {
        let runner = HookRunner::new(Path::new("/nonexistent"), HookConfig::default());
        assert!(!runner.has_hooks());
        assert!(runner.list_hooks().is_empty());
    }

    #[tokio::test]
    async fn pre_hook_no_scripts_allows() {
        let runner = HookRunner::new(Path::new("/nonexistent"), HookConfig::default());
        let event = HookEvent {
            hook_type: "tool.before".into(),
            tool_name: Some("read".into()),
            tool_args: None,
            project_dir: "/tmp".into(),
            timestamp: 0,
        };
        let result = runner.run_pre_hook("tool.before", &event).await;
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn pre_hook_blocking_script() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".theo").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("tool.before.sh"), "#!/bin/sh\nexit 1\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                hooks_dir.join("tool.before.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }

        let runner = HookRunner::new(dir.path(), HookConfig::default());
        let event = HookEvent {
            hook_type: "tool.before".into(),
            tool_name: Some("bash".into()),
            tool_args: None,
            project_dir: dir.path().display().to_string(),
            timestamp: 0,
        };
        let result = runner.run_pre_hook("tool.before", &event).await;
        assert!(!result.allowed);
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn pre_hook_allowing_script() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".theo").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(
            hooks_dir.join("tool.before.sh"),
            "#!/bin/sh\necho ok\nexit 0\n",
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                hooks_dir.join("tool.before.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }

        let runner = HookRunner::new(dir.path(), HookConfig::default());
        let event = HookEvent {
            hook_type: "tool.before".into(),
            tool_name: Some("read".into()),
            tool_args: None,
            project_dir: dir.path().display().to_string(),
            timestamp: 0,
        };
        let result = runner.run_pre_hook("tool.before", &event).await;
        assert!(result.allowed);
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn hook_timeout_kills_process() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".theo").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("tool.before.sh"), "#!/bin/sh\nsleep 60\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                hooks_dir.join("tool.before.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }

        let mut config = HookConfig::default();
        config.timeout_secs = 1; // 1 second timeout

        let runner = HookRunner::new(dir.path(), config);
        let event = HookEvent {
            hook_type: "tool.before".into(),
            tool_name: None,
            tool_args: None,
            project_dir: "/tmp".into(),
            timestamp: 0,
        };
        let result = runner.run_pre_hook("tool.before", &event).await;
        assert!(!result.allowed); // timeout = blocked
    }

    #[test]
    fn list_hooks_finds_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".theo").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("tool.before.sh"), "#!/bin/sh\n").unwrap();
        std::fs::write(hooks_dir.join("run.start.sh"), "#!/bin/sh\n").unwrap();
        std::fs::write(hooks_dir.join("not-a-hook.txt"), "ignore\n").unwrap();

        let runner = HookRunner::new(dir.path(), HookConfig::default());
        let hooks = runner.list_hooks();
        assert_eq!(hooks.len(), 2);
    }

    #[test]
    fn tool_hook_event_creates_correct_json() {
        let event = tool_hook_event(
            "tool.before",
            "bash",
            &serde_json::json!({"command": "ls"}),
            Path::new("/tmp/project"),
        );
        assert_eq!(event.hook_type, "tool.before");
        assert_eq!(event.tool_name.as_deref(), Some("bash"));
        assert!(event.timestamp > 0);
    }
}
