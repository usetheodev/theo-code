//! macOS sandbox backend — stub implementation.
//!
//! macOS does not have landlock or user namespaces.
//! `sandbox-exec` is deprecated since macOS 10.15.
//!
//! Current status: returns SandboxError::Unavailable with clear message.
//! Future options:
//! 1. Container via Lima/Colima (higher overhead)
//! 2. App Sandbox via entitlements (limited for CLI)
//! 3. Accept reduced sandbox with explicit warning

use std::path::Path;
use theo_domain::sandbox::{SandboxConfig, SandboxError, SandboxResult};

use super::executor::SandboxExecutor;

/// macOS sandbox executor — currently a stub that warns about limited support.
pub struct MacOsSandboxExecutor;

impl MacOsSandboxExecutor {
    pub fn new() -> Result<Self, SandboxError> {
        // macOS sandbox is always "available" as a stub,
        // but warns about reduced functionality
        Ok(Self)
    }
}

impl SandboxExecutor for MacOsSandboxExecutor {
    fn execute_sandboxed(
        &self,
        command: &str,
        working_dir: &Path,
        config: &SandboxConfig,
    ) -> Result<SandboxResult, SandboxError> {
        if config.fail_if_unavailable {
            return Err(SandboxError::Unavailable(
                "full sandbox (landlock/namespaces) not available on macOS. \
                 Set fail_if_unavailable=false to run with reduced isolation."
                    .to_string(),
            ));
        }

        // Reduced sandbox: only command validation + env sanitization
        // (no kernel-level filesystem or network isolation)
        use super::command_validator::{self, ValidatorConfig, ValidationResult};
        use super::env_sanitizer;
        use std::process::Stdio;

        // Step 1: Validate command
        let validator_config = ValidatorConfig::default();
        if let ValidationResult::Blocked(violation) =
            command_validator::validate_command(command, &validator_config)
        {
            return Ok(SandboxResult::blocked(violation));
        }

        // Step 2: Execute with env sanitization only
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c")
            .arg(command)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        env_sanitizer::apply_to_command(&mut cmd, &config.process);

        let output = cmd
            .output()
            .map_err(|e| SandboxError::InitFailed(format!("failed to spawn: {e}")))?;

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(SandboxResult::success(exit_code, stdout, stderr, vec![]))
        } else {
            Ok(SandboxResult::failed(exit_code, stdout, stderr, vec![], vec![]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_executor_creates() {
        let executor = MacOsSandboxExecutor::new();
        assert!(executor.is_ok());
    }

    #[test]
    fn macos_executor_fail_if_unavailable_returns_error() {
        let executor = MacOsSandboxExecutor::new().unwrap();
        let config = SandboxConfig::default(); // fail_if_unavailable = true
        let result = executor.execute_sandboxed("echo test", Path::new("/tmp"), &config);
        assert!(result.is_err());
        if let Err(SandboxError::Unavailable(msg)) = result {
            assert!(msg.contains("macOS"));
        }
    }

    #[test]
    fn macos_executor_graceful_runs_command() {
        let executor = MacOsSandboxExecutor::new().unwrap();
        let config = SandboxConfig {
            fail_if_unavailable: false,
            ..SandboxConfig::default()
        };
        let result = executor
            .execute_sandboxed("echo macos_test", Path::new("/tmp"), &config)
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("macos_test"));
    }

    #[test]
    fn macos_executor_blocks_dangerous_command() {
        let executor = MacOsSandboxExecutor::new().unwrap();
        let config = SandboxConfig {
            fail_if_unavailable: false,
            ..SandboxConfig::default()
        };
        let result = executor
            .execute_sandboxed("rm -rf /", Path::new("/tmp"), &config)
            .unwrap();
        assert!(!result.success);
        assert!(!result.violations.is_empty());
    }
}
