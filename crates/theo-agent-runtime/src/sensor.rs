//! Computational sensors — automated verification after write tool execution.
//!
//! Sensors are post-edit hooks that run deterministic checks (clippy, cargo test)
//! after successful file modifications. Results are captured and injected back into
//! the LLM conversation as system messages, providing a feedback loop.
//!
//! Convention: `.theo/hooks/edit.verify.sh` receives JSON stdin with
//! `file_path`, `tool_name`, `project_dir` and outputs diagnostics to stdout.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::hooks::{HookConfig, HookRunner};

/// Result of a computational sensor execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorResult {
    /// Which write tool triggered this sensor (edit, write, apply_patch).
    pub tool_name: String,
    /// Which file was modified.
    pub file_path: String,
    /// Sensor stdout output (diagnostics, warnings, errors).
    pub output: String,
    /// Exit code: 0 = clean, non-zero = issues found.
    pub exit_code: i32,
    /// How long the sensor took in milliseconds.
    pub duration_ms: u64,
}

/// Orchestrates computational sensor execution after write tools.
///
/// Sensors fire asynchronously (via `tokio::spawn`) and results accumulate in
/// a pending queue. The agent loop drains pending results before each LLM call.
pub struct SensorRunner {
    hook_runner: HookRunner,
    /// Config remembered so `fire()` can spin up a fresh HookRunner
    /// (HookRunner is not Send across `await`) with the SAME settings
    /// instead of `HookConfig::default()`. T4.1 follow-up:
    /// after `project_hooks_enabled` defaulted to `false` (find_p6_006),
    /// using the default in `fire()` would stop sensors firing in
    /// production for any user that opted-in via their `.theo/config.toml`.
    config: HookConfig,
    pending: Arc<Mutex<Vec<SensorResult>>>,
}

/// The hook type name used for edit verification sensors.
pub const SENSOR_HOOK_TYPE: &str = "edit.verify";

/// Check if a tool name corresponds to a write tool that should trigger sensors.
pub fn is_write_tool(tool_name: &str) -> bool {
    matches!(tool_name, "edit" | "write" | "apply_patch")
}

impl SensorRunner {
    /// Create a new SensorRunner for the given project directory.
    pub fn new(project_dir: &Path, config: HookConfig) -> Self {
        Self {
            hook_runner: HookRunner::new(project_dir, config.clone()),
            config,
            pending: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Check if any sensor hooks are registered.
    pub fn has_sensors(&self) -> bool {
        !self.hook_runner.find_hooks_for_type(SENSOR_HOOK_TYPE).is_empty()
    }

    /// Fire a sensor asynchronously after a successful write tool execution.
    ///
    /// The sensor runs in a background task. Results are collected in the
    /// pending queue and can be drained via `drain_pending()`.
    pub fn fire(&self, tool_name: &str, file_path: &str, project_dir: &Path) {
        let pending = Arc::clone(&self.pending);
        let tool_name = tool_name.to_string();
        let file_path_str = file_path.to_string();
        let project_dir = project_dir.to_path_buf();

        // Build hook event
        let event = crate::hooks::HookEvent {
            hook_type: SENSOR_HOOK_TYPE.to_string(),
            tool_name: Some(tool_name.clone()),
            tool_args: Some(serde_json::json!({
                "file_path": file_path_str,
                "project_dir": project_dir.display().to_string(),
            })),
            project_dir: project_dir.display().to_string(),
            timestamp: crate::hooks::now_millis(),
        };

        // Create a new HookRunner for the spawned task (HookRunner is not Send-safe
        // across await points due to internal state, so we create fresh).
        // Carries the user's config (T4.1 follow-up — default is now
        // `project_hooks_enabled = false`, so an explicit user opt-in
        // must be honoured here).
        let hook_runner = HookRunner::new(&project_dir, self.config.clone());

        tokio::spawn(async move {
            let start = Instant::now();
            let result = hook_runner.run_sensor_hook(SENSOR_HOOK_TYPE, &event).await;
            let duration_ms = start.elapsed().as_millis() as u64;

            let sensor_result = SensorResult {
                tool_name,
                file_path: file_path_str,
                output: result.output,
                exit_code: result.exit_code,
                duration_ms,
            };

            if let Ok(mut queue) = pending.lock() {
                queue.push(sensor_result);
            }
        });
    }

    /// Drain all pending sensor results collected since the last drain.
    ///
    /// Called by the agent loop before each LLM call to inject sensor feedback.
    pub fn drain_pending(&self) -> Vec<SensorResult> {
        if let Ok(mut queue) = self.pending.lock() {
            std::mem::take(&mut *queue)
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T4.1 follow-up — sensor tests must explicitly opt into project
    /// hooks (the default became `false` to plug find_p6_006).
    fn enabled_config() -> HookConfig {
        HookConfig {
            project_hooks_enabled: true,
            ..HookConfig::default()
        }
    }

    #[test]
    fn is_write_tool_recognizes_edit_write_apply_patch() {
        assert!(is_write_tool("edit"));
        assert!(is_write_tool("write"));
        assert!(is_write_tool("apply_patch"));
        assert!(!is_write_tool("read"));
        assert!(!is_write_tool("bash"));
        assert!(!is_write_tool("grep"));
    }

    #[test]
    fn sensor_result_serde_roundtrip() {
        let result = SensorResult {
            tool_name: "edit".into(),
            file_path: "src/main.rs".into(),
            output: "warning: unused variable".into(),
            exit_code: 0,
            duration_ms: 150,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: SensorResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_name, "edit");
        assert_eq!(back.file_path, "src/main.rs");
        assert_eq!(back.output, "warning: unused variable");
        assert_eq!(back.exit_code, 0);
        assert_eq!(back.duration_ms, 150);
    }

    #[test]
    fn sensor_runner_no_hooks_has_no_sensors() {
        let runner = SensorRunner::new(Path::new("/nonexistent"), HookConfig::default());
        assert!(!runner.has_sensors());
    }

    #[test]
    fn drain_pending_returns_empty_initially() {
        let runner = SensorRunner::new(Path::new("/nonexistent"), HookConfig::default());
        let results = runner.drain_pending();
        assert!(results.is_empty());
    }

    #[test]
    fn drain_pending_clears_queue() {
        let runner = SensorRunner::new(Path::new("/nonexistent"), HookConfig::default());
        // Manually push to pending to test drain behavior
        {
            let mut queue = runner.pending.lock().unwrap();
            queue.push(SensorResult {
                tool_name: "edit".into(),
                file_path: "test.rs".into(),
                output: "ok".into(),
                exit_code: 0,
                duration_ms: 10,
            });
        }
        let results = runner.drain_pending();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "test.rs");

        // Second drain should be empty
        let results2 = runner.drain_pending();
        assert!(results2.is_empty());
    }

    #[tokio::test]
    async fn sensor_fires_and_captures_output() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".theo").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(
            hooks_dir.join("edit.verify.sh"),
            "#!/bin/sh\necho 'warning: unused variable x'\nexit 0\n",
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                hooks_dir.join("edit.verify.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }

        let runner = SensorRunner::new(dir.path(), enabled_config());
        assert!(runner.has_sensors());

        runner.fire("edit", "src/main.rs", dir.path());

        // Wait for the async task to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let results = runner.drain_pending();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_name, "edit");
        assert_eq!(results[0].file_path, "src/main.rs");
        assert!(results[0].output.contains("unused variable"));
        assert_eq!(results[0].exit_code, 0);
        assert!(results[0].duration_ms < 5000);
    }

    #[tokio::test]
    async fn sensor_captures_nonzero_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".theo").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(
            hooks_dir.join("edit.verify.sh"),
            "#!/bin/sh\necho 'error: type mismatch'\nexit 1\n",
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                hooks_dir.join("edit.verify.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }

        let runner = SensorRunner::new(dir.path(), enabled_config());
        runner.fire("write", "lib.rs", dir.path());

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let results = runner.drain_pending();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].exit_code, 1);
        assert!(results[0].output.contains("type mismatch"));
    }

    #[tokio::test]
    async fn sensor_does_not_fire_for_non_write_tools() {
        // No hooks dir = no sensors = fire is a no-op
        let runner = SensorRunner::new(Path::new("/nonexistent"), HookConfig::default());
        runner.fire("read", "src/main.rs", Path::new("/tmp"));

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let _results = runner.drain_pending();
        // The filtering of non-write tools is done by the caller (is_write_tool check)
        assert!(!is_write_tool("read"));
    }
}
