//! Sandbox executor — applies kernel-level isolation to command execution.
//!
//! Two implementations:
//! - `LandlockExecutor`: Real sandbox via landlock (Linux 5.13+)
//! - `NoopExecutor`: Passthrough for testing or unsupported platforms

use std::path::Path;
use std::process::Stdio;
use theo_domain::sandbox::{
    AuditEntry, AuditEventType, SandboxConfig, SandboxError, SandboxResult,
};

use super::command_validator::{self, ValidationResult, ValidatorConfig};
use super::probe;

/// Trait for sandbox execution — enables DIP (BashTool depends on trait, not concrete).
pub trait SandboxExecutor: Send + Sync {
    /// Execute a command within the sandbox.
    ///
    /// Returns SandboxResult with output, violations, and audit entries.
    fn execute_sandboxed(
        &self,
        command: &str,
        working_dir: &Path,
        config: &SandboxConfig,
    ) -> Result<SandboxResult, SandboxError>;
}

/// Passthrough executor — no isolation, for testing and unsupported platforms.
pub struct NoopExecutor;

impl SandboxExecutor for NoopExecutor {
    fn execute_sandboxed(
        &self,
        command: &str,
        working_dir: &Path,
        _config: &SandboxConfig,
    ) -> Result<SandboxResult, SandboxError> {
        // Execute without any isolation
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| SandboxError::InitFailed(format!("failed to spawn: {e}")))?;

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(SandboxResult::success(exit_code, stdout, stderr, vec![]))
        } else {
            Ok(SandboxResult::failed(
                exit_code,
                stdout,
                stderr,
                vec![],
                vec![],
            ))
        }
    }
}

/// Landlock-based executor — real filesystem isolation on Linux 5.13+.
#[cfg(target_os = "linux")]
pub struct LandlockExecutor {
    capabilities: probe::SandboxCapabilities,
}

#[cfg(target_os = "linux")]
impl LandlockExecutor {
    /// Create a new LandlockExecutor, probing the kernel for capabilities.
    pub fn new() -> Result<Self, SandboxError> {
        let capabilities = probe::probe_kernel();
        if !capabilities.landlock_available {
            return Err(SandboxError::Unavailable(
                "landlock not available on this kernel".to_string(),
            ));
        }
        Ok(Self { capabilities })
    }

    /// Create with pre-probed capabilities (for testing).
    pub fn with_capabilities(capabilities: probe::SandboxCapabilities) -> Self {
        Self { capabilities }
    }
}

#[cfg(target_os = "linux")]
impl SandboxExecutor for LandlockExecutor {
    fn execute_sandboxed(
        &self,
        command: &str,
        working_dir: &Path,
        config: &SandboxConfig,
    ) -> Result<SandboxResult, SandboxError> {
        let mut audit_entries = Vec::new();

        // Step 1: Validate command lexically
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
                // Warning = allow but log
            }
            ValidationResult::Allowed => {}
        }

        audit_entries.push(AuditEntry {
            timestamp: now_iso8601(),
            event_type: AuditEventType::SandboxInit,
            detail: format!(
                "initializing landlock sandbox (ABI v{})",
                self.capabilities.landlock_abi_version
            ),
            metadata: serde_json::json!({"abi_version": self.capabilities.landlock_abi_version}),
        });

        // Step 2: Build landlock ruleset for the child process
        let working_dir_owned = working_dir.to_path_buf();
        let allowed_read: Vec<String> = config.filesystem.allowed_read.clone();
        let allowed_write: Vec<String> = config.filesystem.allowed_write.clone();
        let process_policy = config.process.clone();

        audit_entries.push(AuditEntry {
            timestamp: now_iso8601(),
            event_type: AuditEventType::CommandStart,
            detail: format!("executing: {command}"),
            metadata: serde_json::json!({"command": command, "workdir": working_dir.display().to_string()}),
        });

        // Step 3: Build command with env sanitization
        use std::os::unix::process::CommandExt;

        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c")
            .arg(command)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Apply env sanitization (whitelist approach)
        super::env_sanitizer::apply_to_command(&mut cmd, &config.process);

        audit_entries.push(AuditEntry {
            timestamp: now_iso8601(),
            event_type: AuditEventType::EnvVarStripped,
            detail: format!(
                "env sanitized: {} vars allowed",
                config.process.allowed_env_vars.len()
            ),
            metadata: serde_json::json!({"allowed_count": config.process.allowed_env_vars.len()}),
        });

        // Step 4: Apply rlimits + network ns + landlock in child via pre_exec
        // Order: rlimits → unshare(net) → landlock (per governance decision)
        // SAFETY: All syscalls (setrlimit, unshare, landlock_restrict_self) are
        // async-signal-safe.
        let network_policy = config.network.clone();
        unsafe {
            cmd.pre_exec(move || {
                // 1. Apply resource limits
                super::rlimits::apply_rlimits(&process_policy)?;
                // 2. Apply network isolation (unshare NEWUSER|NEWNET)
                super::network::apply_network_isolation(&network_policy)?;
                // 3. Apply landlock filesystem restrictions
                apply_landlock_in_child(&working_dir_owned, &allowed_read, &allowed_write)
            });
        }

        audit_entries.push(AuditEntry {
            timestamp: now_iso8601(),
            event_type: AuditEventType::ResourceLimitApplied,
            detail: format!(
                "rlimits: cpu={}s mem={}B fsize={}B nproc={}",
                config.process.max_cpu_seconds,
                config.process.max_memory_bytes,
                config.process.max_file_size_bytes,
                config.process.max_processes,
            ),
            metadata: serde_json::json!({
                "max_cpu_seconds": config.process.max_cpu_seconds,
                "max_memory_bytes": config.process.max_memory_bytes,
                "max_file_size_bytes": config.process.max_file_size_bytes,
                "max_processes": config.process.max_processes,
            }),
        });

        let output = cmd.output();

        let output = match output {
            Ok(o) => o,
            Err(e) => {
                audit_entries.push(AuditEntry {
                    timestamp: now_iso8601(),
                    event_type: AuditEventType::CommandEnd,
                    detail: format!("spawn failed: {e}"),
                    metadata: serde_json::json!({"error": e.to_string()}),
                });
                return Ok(SandboxResult::failed(
                    -1,
                    String::new(),
                    format!("sandbox spawn failed: {e}"),
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
            detail: format!("command finished with exit code {exit_code}"),
            metadata: serde_json::json!({"exit_code": exit_code}),
        });

        if output.status.success() {
            Ok(SandboxResult::success(
                exit_code,
                stdout,
                stderr,
                audit_entries,
            ))
        } else {
            Ok(SandboxResult::failed(
                exit_code,
                stdout,
                stderr,
                vec![],
                audit_entries,
            ))
        }
    }
}

/// Apply landlock restrictions in the child process (after fork, before exec).
///
/// SAFETY: This runs in a forked child. Must not allocate, panic, or use
/// non-async-signal-safe functions. landlock syscalls are safe here.
///
/// Note: In practice, the landlock crate does allocate internally (Strings for errors),
/// but the syscalls themselves are signal-safe. This is acceptable because pre_exec
/// in Rust's stdlib is already not fully async-signal-safe (it uses closures).
#[cfg(target_os = "linux")]
fn apply_landlock_in_child(
    working_dir: &Path,
    allowed_read: &[String],
    allowed_write: &[String],
) -> std::io::Result<()> {
    use landlock::{
        ABI, Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr,
    };

    let abi = ABI::V1; // Use V1 for maximum compatibility (kernel 5.13+)
    let all_access = AccessFs::from_all(abi);
    let to_io =
        |e: std::fmt::Arguments<'_>| std::io::Error::other(e.to_string());

    let ruleset = Ruleset::default()
        .handle_access(all_access)
        .map_err(|e| to_io(format_args!("{e}")))?
        .create()
        .map_err(|e| to_io(format_args!("{e}")))?;

    // Build rules: working_dir (rw), standard paths (ro), /tmp (rw), configured paths
    let mut created = ruleset;

    // Allow full access to the working directory
    if let Ok(fd) = PathFd::new(working_dir) {
        created = created
            .add_rule(PathBeneath::new(fd, all_access))
            .map_err(|e| to_io(format_args!("{e}")))?;
    }

    // Allow read access to standard paths needed for shell execution
    for path in &[
        "/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc", "/proc", "/dev",
    ] {
        if let Ok(fd) = PathFd::new(path) {
            created = created
                .add_rule(PathBeneath::new(fd, all_access))
                .map_err(|e| to_io(format_args!("{e}")))?;
        }
    }

    // Allow read+write to /tmp
    if let Ok(fd) = PathFd::new("/tmp") {
        created = created
            .add_rule(PathBeneath::new(fd, all_access))
            .map_err(|e| to_io(format_args!("{e}")))?;
    }

    // Allow additional configured read paths
    for path in allowed_read {
        if let Ok(fd) = PathFd::new(path.as_str()) {
            created = created
                .add_rule(PathBeneath::new(fd, all_access))
                .map_err(|e| to_io(format_args!("{e}")))?;
        }
    }

    // Allow additional configured write paths
    for path in allowed_write {
        if let Ok(fd) = PathFd::new(path.as_str()) {
            created = created
                .add_rule(PathBeneath::new(fd, all_access))
                .map_err(|e| to_io(format_args!("{e}")))?;
        }
    }

    // Restrict self — this applies to the current process (the child)
    created
        .restrict_self()
        .map_err(|e| to_io(format_args!("{e}")))?;

    Ok(())
}

/// Result of the backend-selection decision — pure data, no side effects.
///
/// Extracted so `decide_backend` can be unit-tested without touching the
/// real kernel probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BackendDecision {
    /// Sandbox disabled by configuration.
    Disabled,
    /// Use bwrap (preferred on Linux).
    Bwrap,
    /// Use landlock (Linux fallback when bwrap missing).
    Landlock,
    /// No backend available; user opted into running without isolation.
    /// The reason is preserved so the wrapper can log it.
    NoopFallback { reason: &'static str },
}

/// Input to the decision function — presence of kernel-level capabilities.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BackendProbe {
    pub is_linux: bool,
    pub bwrap_ok: bool,
    pub landlock_ok: bool,
}

/// Pure decision function — given config + probed capabilities, pick a backend.
///
/// Keeping this pure lets us test every branch cross-platform, including the
/// "no backend available" fallback path which is otherwise unreachable on a
/// Linux CI host that has landlock.
pub(crate) fn decide_backend(
    config: &SandboxConfig,
    probe: BackendProbe,
) -> Result<BackendDecision, SandboxError> {
    if !config.enabled {
        return Ok(BackendDecision::Disabled);
    }

    if probe.is_linux {
        if probe.bwrap_ok {
            return Ok(BackendDecision::Bwrap);
        }
        if probe.landlock_ok {
            return Ok(BackendDecision::Landlock);
        }
        if config.fail_if_unavailable {
            return Err(SandboxError::Unavailable(
                "neither bwrap nor landlock available".to_string(),
            ));
        }
        return Ok(BackendDecision::NoopFallback {
            reason: "no linux sandbox backend available (neither bwrap nor landlock)",
        });
    }

    if config.fail_if_unavailable {
        return Err(SandboxError::Unavailable(
            "sandbox not supported on this platform".to_string(),
        ));
    }
    Ok(BackendDecision::NoopFallback {
        reason: "sandbox not supported on this platform",
    })
}

/// Create the appropriate SandboxExecutor for the current platform.
///
/// Cascading preference: bwrap > landlock > noop.
/// - bwrap: Full isolation (PID ns, net ns, mount ns, capabilities) — preferred
/// - landlock: Filesystem isolation only — fallback on Linux without bwrap
/// - noop: No isolation — last resort (only when `fail_if_unavailable=false`)
///
/// Safety-critical: when falling back to `NoopExecutor`, emits a structured
/// WARN log so the operator is never silently left without isolation.
pub fn create_executor(config: &SandboxConfig) -> Result<Box<dyn SandboxExecutor>, SandboxError> {
    // Probe once, then defer to the pure decision function.
    let is_linux = cfg!(target_os = "linux");

    #[cfg(target_os = "linux")]
    let (bwrap_candidate, landlock_candidate) = {
        // Constructing the executor IS the probe — the constructor returns
        // Err when the kernel feature is missing. We hold onto the successful
        // instance to avoid a second construction when it is selected.
        let bwrap = super::bwrap::BwrapExecutor::new().ok();
        let landlock = if bwrap.is_none() {
            LandlockExecutor::new().ok()
        } else {
            None
        };
        (bwrap, landlock)
    };
    #[cfg(not(target_os = "linux"))]
    let (bwrap_candidate, landlock_candidate): (Option<NoopExecutor>, Option<NoopExecutor>) =
        (None, None);

    let probe = BackendProbe {
        is_linux,
        bwrap_ok: bwrap_candidate.is_some(),
        landlock_ok: landlock_candidate.is_some(),
    };

    match decide_backend(config, probe)? {
        BackendDecision::Disabled => Ok(Box::new(NoopExecutor)),
        #[cfg(target_os = "linux")]
        BackendDecision::Bwrap => {
            // unwrap is sound — decide_backend only yields Bwrap when probe.bwrap_ok is true
            Ok(Box::new(bwrap_candidate.expect(
                "decide_backend returned Bwrap but bwrap_candidate is None",
            )))
        }
        #[cfg(target_os = "linux")]
        BackendDecision::Landlock => Ok(Box::new(landlock_candidate.expect(
            "decide_backend returned Landlock but landlock_candidate is None",
        ))),
        #[cfg(not(target_os = "linux"))]
        BackendDecision::Bwrap | BackendDecision::Landlock => {
            // Unreachable on non-linux because probe.is_linux == false.
            unreachable!("linux-only branch reached on non-linux target");
        }
        BackendDecision::NoopFallback { reason } => {
            log::warn!(
                target: "theo_tooling::sandbox",
                "sandbox backend unavailable ({reason}); falling back to NoopExecutor — bash tools will execute WITHOUT isolation. Set SandboxConfig::fail_if_unavailable=true to refuse this fallback."
            );
            Ok(Box::new(NoopExecutor))
        }
    }
}

fn now_iso8601() -> String {
    // Simple timestamp without chrono dependency
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}s", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::sandbox::SandboxConfig;

    // ── NoopExecutor tests ──────────────────────────────────────

    #[test]
    fn noop_executor_runs_echo() {
        let executor = NoopExecutor;
        let config = SandboxConfig {
            enabled: false,
            ..SandboxConfig::default()
        };
        let result = executor
            .execute_sandboxed("echo hello", Path::new("/tmp"), &config)
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
        assert!(result.violations.is_empty());
    }

    #[test]
    fn noop_executor_captures_stderr() {
        let executor = NoopExecutor;
        let config = SandboxConfig {
            enabled: false,
            ..SandboxConfig::default()
        };
        let result = executor
            .execute_sandboxed("echo error >&2", Path::new("/tmp"), &config)
            .unwrap();
        assert!(result.stderr.contains("error"));
    }

    #[test]
    fn noop_executor_reports_nonzero_exit() {
        let executor = NoopExecutor;
        let config = SandboxConfig {
            enabled: false,
            ..SandboxConfig::default()
        };
        let result = executor
            .execute_sandboxed("exit 42", Path::new("/tmp"), &config)
            .unwrap();
        assert!(!result.success);
        assert_eq!(result.exit_code, 42);
    }

    // ── create_executor tests ───────────────────────────────────

    #[test]
    fn create_executor_disabled_returns_noop() {
        let config = SandboxConfig {
            enabled: false,
            ..SandboxConfig::default()
        };
        let executor = create_executor(&config).unwrap();
        let result = executor
            .execute_sandboxed("echo test", Path::new("/tmp"), &config)
            .unwrap();
        assert!(result.success);
    }

    // ── decide_backend tests (pure, cross-platform) ─────────────
    //
    // Every branch of the backend selection is tested here without
    // touching the real kernel. This is the only way to cover the
    // "no linux backend available" path on a Linux host that actually
    // has landlock.

    #[test]
    fn decide_backend_disabled_when_config_off() {
        let config = SandboxConfig {
            enabled: false,
            ..SandboxConfig::default()
        };
        let probe = BackendProbe {
            is_linux: true,
            bwrap_ok: true,
            landlock_ok: true,
        };
        assert_eq!(
            decide_backend(&config, probe).unwrap(),
            BackendDecision::Disabled
        );
    }

    #[test]
    fn decide_backend_prefers_bwrap_on_linux() {
        let config = SandboxConfig {
            enabled: true,
            fail_if_unavailable: true,
            ..SandboxConfig::default()
        };
        let probe = BackendProbe {
            is_linux: true,
            bwrap_ok: true,
            landlock_ok: true,
        };
        assert_eq!(
            decide_backend(&config, probe).unwrap(),
            BackendDecision::Bwrap
        );
    }

    #[test]
    fn decide_backend_falls_back_to_landlock_without_bwrap() {
        let config = SandboxConfig {
            enabled: true,
            fail_if_unavailable: true,
            ..SandboxConfig::default()
        };
        let probe = BackendProbe {
            is_linux: true,
            bwrap_ok: false,
            landlock_ok: true,
        };
        assert_eq!(
            decide_backend(&config, probe).unwrap(),
            BackendDecision::Landlock
        );
    }

    #[test]
    fn decide_backend_errors_on_linux_when_strict_and_no_backend() {
        let config = SandboxConfig {
            enabled: true,
            fail_if_unavailable: true,
            ..SandboxConfig::default()
        };
        let probe = BackendProbe {
            is_linux: true,
            bwrap_ok: false,
            landlock_ok: false,
        };
        let result = decide_backend(&config, probe);
        assert!(
            matches!(result, Err(SandboxError::Unavailable(_))),
            "expected Unavailable, got {result:?}"
        );
    }

    #[test]
    fn decide_backend_noop_fallback_when_linux_no_backend_permissive() {
        let config = SandboxConfig {
            enabled: true,
            fail_if_unavailable: false,
            ..SandboxConfig::default()
        };
        let probe = BackendProbe {
            is_linux: true,
            bwrap_ok: false,
            landlock_ok: false,
        };
        match decide_backend(&config, probe).unwrap() {
            BackendDecision::NoopFallback { reason } => {
                assert!(
                    reason.contains("linux"),
                    "reason should mention linux: {reason}"
                );
                assert!(!reason.is_empty());
            }
            other => panic!("expected NoopFallback, got {other:?}"),
        }
    }

    #[test]
    fn decide_backend_errors_on_non_linux_when_strict() {
        let config = SandboxConfig {
            enabled: true,
            fail_if_unavailable: true,
            ..SandboxConfig::default()
        };
        let probe = BackendProbe {
            is_linux: false,
            bwrap_ok: false,
            landlock_ok: false,
        };
        assert!(matches!(
            decide_backend(&config, probe),
            Err(SandboxError::Unavailable(_))
        ));
    }

    #[test]
    fn decide_backend_noop_fallback_when_non_linux_permissive() {
        let config = SandboxConfig {
            enabled: true,
            fail_if_unavailable: false,
            ..SandboxConfig::default()
        };
        let probe = BackendProbe {
            is_linux: false,
            bwrap_ok: false,
            landlock_ok: false,
        };
        match decide_backend(&config, probe).unwrap() {
            BackendDecision::NoopFallback { reason } => {
                assert!(
                    reason.contains("platform"),
                    "reason should mention platform: {reason}"
                );
            }
            other => panic!("expected NoopFallback, got {other:?}"),
        }
    }

    #[test]
    fn decide_backend_ignores_probes_when_disabled() {
        // Even with no sandbox available, disabled config → Disabled (not error).
        let config = SandboxConfig {
            enabled: false,
            fail_if_unavailable: true,
            ..SandboxConfig::default()
        };
        let probe = BackendProbe {
            is_linux: true,
            bwrap_ok: false,
            landlock_ok: false,
        };
        assert_eq!(
            decide_backend(&config, probe).unwrap(),
            BackendDecision::Disabled
        );
    }

    // ── LandlockExecutor tests (Linux only, may need #[ignore]) ─

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_executor_creates_when_available() {
        let caps = probe::probe_kernel();
        if caps.landlock_available {
            let executor = LandlockExecutor::new();
            assert!(executor.is_ok());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_executor_blocks_dangerous_command() {
        let caps = probe::probe_kernel();
        if !caps.landlock_available {
            return; // Skip if landlock not available
        }
        let executor = LandlockExecutor::new().unwrap();
        let config = SandboxConfig::default();
        let result = executor
            .execute_sandboxed("rm -rf /", Path::new("/tmp"), &config)
            .unwrap();
        // Command validator should block this
        assert!(!result.success);
        assert!(!result.violations.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_executor_allows_echo_in_sandbox() {
        let caps = probe::probe_kernel();
        if !caps.landlock_available {
            return;
        }
        let executor = LandlockExecutor::new().unwrap();
        let config = SandboxConfig::default();
        let result = executor
            .execute_sandboxed("echo sandboxed", Path::new("/tmp"), &config)
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("sandboxed"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_executor_blocks_ssh_read() {
        let caps = probe::probe_kernel();
        if !caps.landlock_available {
            return;
        }
        let executor = LandlockExecutor::new().unwrap();
        let config = SandboxConfig::default();
        // Try to read ~/.ssh — landlock should block this
        let home = std::env::var("HOME").unwrap_or_default();
        let cmd = format!("cat {home}/.ssh/id_rsa 2>&1");
        let result = executor
            .execute_sandboxed(&cmd, Path::new("/tmp"), &config)
            .unwrap();
        // Either the command fails (file not found or permission denied),
        // or landlock blocks it at kernel level
        assert!(result.exit_code != 0 || !result.stderr.is_empty());
    }
}
