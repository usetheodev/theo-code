//! Bubblewrap (bwrap) sandbox executor — the primary sandbox backend.
//!
//! Uses the system bwrap binary (/usr/bin/bwrap) to create isolated namespaces:
//! - Mount namespace: read-only root, writable project dir + /tmp
//! - PID namespace: processes inside can't see/signal host processes
//! - Network namespace: no network access (default deny)
//! - Capabilities: all dropped
//! - Auto-cleanup: --die-with-parent ensures no orphan namespaces

use std::path::Path;
use std::process::Stdio;
use theo_domain::sandbox::{
    AuditEntry, AuditEventType, SandboxConfig, SandboxError, SandboxResult,
};

use super::command_validator::{self, ValidationResult, ValidatorConfig};
use super::env_sanitizer;
use super::executor::SandboxExecutor;
use super::probe::BWRAP_PATH;

/// Bubblewrap-based sandbox executor.
pub struct BwrapExecutor {
    bwrap_version: String,
}

impl BwrapExecutor {
    /// Create a new BwrapExecutor, verifying bwrap is available.
    pub fn new() -> Result<Self, SandboxError> {
        let output = std::process::Command::new(BWRAP_PATH)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| SandboxError::Unavailable(format!("bwrap not found at {BWRAP_PATH}: {e}")))?;

        if !output.status.success() {
            return Err(SandboxError::Unavailable(
                "bwrap --version failed".to_string(),
            ));
        }

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Self {
            bwrap_version: version,
        })
    }

    /// Build the bwrap command with all isolation flags.
    fn build_command(
        &self,
        command: &str,
        working_dir: &Path,
        config: &SandboxConfig,
    ) -> std::process::Command {
        let mut cmd = std::process::Command::new(BWRAP_PATH);

        // Read-only root filesystem
        for path in &["/usr", "/lib", "/lib64", "/lib32", "/bin", "/sbin", "/etc"] {
            if Path::new(path).exists() {
                cmd.arg("--ro-bind").arg(path).arg(path);
            }
        }

        // Symlinks for FHS compatibility (some distros use usr-merge)
        // These are no-ops if the targets already exist from ro-bind above
        if !Path::new("/bin").exists() && Path::new("/usr/bin").exists() {
            cmd.arg("--symlink").arg("usr/bin").arg("/bin");
        }
        if !Path::new("/sbin").exists() && Path::new("/usr/sbin").exists() {
            cmd.arg("--symlink").arg("usr/sbin").arg("/sbin");
        }

        // Proc and dev
        cmd.arg("--proc").arg("/proc");
        cmd.arg("--dev").arg("/dev");

        // Writable /tmp
        cmd.arg("--tmpfs").arg("/tmp");

        // Writable project directory
        cmd.arg("--bind")
            .arg(working_dir)
            .arg(working_dir);
        cmd.arg("--chdir").arg(working_dir);

        // Additional allowed read paths from config
        for path in &config.filesystem.allowed_read {
            if Path::new(path).exists() {
                cmd.arg("--ro-bind").arg(path).arg(path);
            }
        }

        // Additional allowed write paths from config
        for path in &config.filesystem.allowed_write {
            if Path::new(path).exists() && path.as_str() != working_dir.to_str().unwrap_or("") {
                cmd.arg("--bind").arg(path).arg(path);
            }
        }

        // PID namespace isolation
        cmd.arg("--unshare-pid");

        // Network isolation (unless explicitly allowed)
        if !config.network.allow_network {
            cmd.arg("--unshare-net");
        }

        // Drop ALL capabilities
        cmd.arg("--cap-drop").arg("ALL");

        // Auto-cleanup: child dies when parent dies
        cmd.arg("--die-with-parent");

        // Prevent terminal injection (TIOCSTI)
        cmd.arg("--new-session");

        // Apply env sanitization
        env_sanitizer::apply_to_command(&mut cmd, &config.process);

        // The actual command to run inside the sandbox
        cmd.arg("sh").arg("-c").arg(command);

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        cmd
    }
}

impl SandboxExecutor for BwrapExecutor {
    fn execute_sandboxed(
        &self,
        command: &str,
        working_dir: &Path,
        config: &SandboxConfig,
    ) -> Result<SandboxResult, SandboxError> {
        let mut audit_entries = Vec::new();

        // Step 1: Validate command lexically (before any fork)
        let validator_config = ValidatorConfig::default();
        match command_validator::validate_command(command, &validator_config) {
            ValidationResult::Blocked(violation) => {
                audit_entries.push(AuditEntry {
                    timestamp: now_iso8601(),
                    event_type: AuditEventType::ViolationBlocked,
                    detail: format!("command blocked by validator: {command}"),
                    metadata: serde_json::json!({"command": command}),
                });
                return Ok(SandboxResult::blocked(violation));
            }
            ValidationResult::Warning(msg) => {
                audit_entries.push(AuditEntry {
                    timestamp: now_iso8601(),
                    event_type: AuditEventType::ViolationBlocked,
                    detail: format!("command warning: {msg}"),
                    metadata: serde_json::json!({"command": command, "warning": msg}),
                });
            }
            ValidationResult::Allowed => {}
        }

        audit_entries.push(AuditEntry {
            timestamp: now_iso8601(),
            event_type: AuditEventType::SandboxInit,
            detail: format!("bwrap sandbox ({})", self.bwrap_version),
            metadata: serde_json::json!({
                "backend": "bwrap",
                "version": self.bwrap_version,
                "network_isolated": !config.network.allow_network,
            }),
        });

        // Step 2: Build and execute bwrap command
        let mut cmd = self.build_command(command, working_dir, config);

        audit_entries.push(AuditEntry {
            timestamp: now_iso8601(),
            event_type: AuditEventType::CommandStart,
            detail: format!("executing: {command}"),
            metadata: serde_json::json!({
                "command": command,
                "workdir": working_dir.display().to_string(),
            }),
        });

        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                audit_entries.push(AuditEntry {
                    timestamp: now_iso8601(),
                    event_type: AuditEventType::CommandEnd,
                    detail: format!("bwrap spawn failed: {e}"),
                    metadata: serde_json::json!({"error": e.to_string()}),
                });
                return Ok(SandboxResult::failed(
                    -1,
                    String::new(),
                    format!("bwrap spawn failed: {e}"),
                    vec![],
                    audit_entries,
                ));
            }
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        audit_entries.push(AuditEntry {
            timestamp: now_iso8601(),
            event_type: AuditEventType::CommandEnd,
            detail: format!("exit code {exit_code}"),
            metadata: serde_json::json!({"exit_code": exit_code}),
        });

        audit_entries.push(AuditEntry {
            timestamp: now_iso8601(),
            event_type: AuditEventType::Cleanup,
            detail: "bwrap namespace auto-cleaned (--die-with-parent)".to_string(),
            metadata: serde_json::json!({}),
        });

        if output.status.success() {
            Ok(SandboxResult::success(exit_code, stdout, stderr, audit_entries))
        } else {
            Ok(SandboxResult::failed(exit_code, stdout, stderr, vec![], audit_entries))
        }
    }
}

fn now_iso8601() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}s", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::probe;

    fn bwrap_available() -> bool {
        probe::probe_kernel().bwrap_available
    }

    fn default_config() -> SandboxConfig {
        SandboxConfig::default()
    }

    #[test]
    fn bwrap_executor_creates_when_available() {
        if !bwrap_available() {
            return;
        }
        let executor = BwrapExecutor::new();
        assert!(executor.is_ok());
        assert!(!executor.unwrap().bwrap_version.is_empty());
    }

    #[test]
    fn bwrap_echo_works_in_sandbox() {
        if !bwrap_available() {
            return;
        }
        let executor = BwrapExecutor::new().unwrap();
        let result = executor
            .execute_sandboxed("echo bwrap_sandboxed", Path::new("/tmp"), &default_config())
            .unwrap();
        assert!(result.success, "echo should succeed in sandbox. stderr: {}", result.stderr);
        assert!(
            result.stdout.contains("bwrap_sandboxed"),
            "stdout should contain output. Got: {}",
            result.stdout
        );
    }

    #[test]
    fn bwrap_root_is_readonly() {
        if !bwrap_available() {
            return;
        }
        let executor = BwrapExecutor::new().unwrap();
        let result = executor
            .execute_sandboxed(
                "touch /usr/bwrap_test_file 2>&1; echo exit=$?",
                Path::new("/tmp"),
                &default_config(),
            )
            .unwrap();
        let output = format!("{}{}", result.stdout, result.stderr);
        assert!(
            output.contains("Read-only") || output.contains("Permission denied") || output.contains("exit=1"),
            "Writing to /usr should fail in sandbox. Got: {output}"
        );
    }

    #[test]
    fn bwrap_tmp_is_writable() {
        if !bwrap_available() {
            return;
        }
        let executor = BwrapExecutor::new().unwrap();
        let result = executor
            .execute_sandboxed(
                "echo test > /tmp/bwrap_write_test && cat /tmp/bwrap_write_test",
                Path::new("/tmp"),
                &default_config(),
            )
            .unwrap();
        assert!(result.success, "Writing to /tmp should succeed. stderr: {}", result.stderr);
        assert!(result.stdout.contains("test"));
    }

    #[test]
    fn bwrap_network_blocked() {
        if !bwrap_available() {
            return;
        }
        let executor = BwrapExecutor::new().unwrap();
        let config = SandboxConfig {
            network: theo_domain::sandbox::NetworkPolicy {
                allow_network: false,
                ..Default::default()
            },
            ..default_config()
        };
        let result = executor
            .execute_sandboxed(
                "curl --connect-timeout 2 http://1.1.1.1 2>&1; echo exit=$?",
                Path::new("/tmp"),
                &config,
            )
            .unwrap();
        let output = format!("{}{}", result.stdout, result.stderr);
        // curl should fail — no network in sandbox
        assert!(
            output.contains("exit=7") || output.contains("exit=28")
                || output.contains("Couldn't connect") || output.contains("Connection refused")
                || output.contains("Network is unreachable") || !result.success,
            "Network should be blocked. Got: {output}"
        );
    }

    #[test]
    fn bwrap_pid_isolated() {
        if !bwrap_available() {
            return;
        }
        let executor = BwrapExecutor::new().unwrap();
        let result = executor
            .execute_sandboxed("ps aux 2>/dev/null | wc -l", Path::new("/tmp"), &default_config())
            .unwrap();
        if result.success {
            let line_count: usize = result.stdout.trim().parse().unwrap_or(999);
            // In a PID namespace, ps should show very few processes (2-5)
            assert!(
                line_count <= 10,
                "PID namespace should show few processes, got {line_count} lines"
            );
        }
    }

    #[test]
    fn bwrap_command_validator_blocks_before_fork() {
        if !bwrap_available() {
            return;
        }
        let executor = BwrapExecutor::new().unwrap();
        let result = executor
            .execute_sandboxed("rm -rf /", Path::new("/tmp"), &default_config())
            .unwrap();
        assert!(!result.success);
        assert!(
            !result.violations.is_empty(),
            "Command validator should block rm -rf / before bwrap"
        );
    }

    #[test]
    fn bwrap_audit_entries_present() {
        if !bwrap_available() {
            return;
        }
        let executor = BwrapExecutor::new().unwrap();
        let result = executor
            .execute_sandboxed("echo audit_test", Path::new("/tmp"), &default_config())
            .unwrap();
        assert!(!result.audit_entries.is_empty(), "Should have audit entries");
        let has_init = result
            .audit_entries
            .iter()
            .any(|e| e.event_type == AuditEventType::SandboxInit);
        assert!(has_init, "Should have SandboxInit audit entry");
    }

    #[test]
    fn bwrap_network_allowed_when_configured() {
        if !bwrap_available() {
            return;
        }
        let executor = BwrapExecutor::new().unwrap();
        let config = SandboxConfig {
            network: theo_domain::sandbox::NetworkPolicy {
                allow_network: true,
                ..Default::default()
            },
            ..default_config()
        };
        // With network allowed, echo should still work
        let result = executor
            .execute_sandboxed("echo net_allowed", Path::new("/tmp"), &config)
            .unwrap();
        assert!(result.success);
    }
}
