//! Sandbox types for secure command execution.
//!
//! These are pure data types — no OS dependencies, no async, no execution logic.
//! The sandbox executor lives in theo-tooling; these types define the contracts.
//!
//! **Instability notice:** These types may change in Phases 2-4 as the actual
//! sandbox implementation with landlock/namespaces reveals constraints.

use serde::{Deserialize, Serialize};

// ── Configuration ───────────────────────────────────────────────────

/// Top-level sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Whether the sandbox is enabled.
    pub enabled: bool,

    /// If true, tool calls are REJECTED when sandbox is unavailable.
    /// If false, execution proceeds without sandbox (with warning).
    /// Default: true (fail-closed).
    pub fail_if_unavailable: bool,

    /// Filesystem access restrictions.
    pub filesystem: FilesystemPolicy,

    /// Network access restrictions.
    pub network: NetworkPolicy,

    /// Process and resource restrictions.
    pub process: ProcessPolicy,

    /// Audit logging configuration.
    pub audit: AuditPolicy,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            fail_if_unavailable: true,
            filesystem: FilesystemPolicy::default(),
            network: NetworkPolicy::default(),
            process: ProcessPolicy::default(),
            audit: AuditPolicy::default(),
        }
    }
}

// ── Filesystem Policy ───────────────────────────────────────────────

/// Filesystem access control policy.
///
/// Deny rules take precedence over allow rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    /// Path patterns allowed for reading (globs).
    pub allowed_read: Vec<String>,

    /// Path patterns allowed for writing (globs).
    pub allowed_write: Vec<String>,

    /// Path patterns always denied for reading.
    /// Takes precedence over allowed_read.
    pub denied_read: Vec<String>,

    /// Path patterns always denied for writing.
    /// Takes precedence over allowed_write.
    pub denied_write: Vec<String>,
}

/// Paths that are ALWAYS denied regardless of configuration.
/// These are hardcoded security boundaries, not configurable.
pub const ALWAYS_DENIED_READ: &[&str] = &[
    "~/.ssh",
    "~/.gnupg",
    "~/.gpg",
    "~/.config/gh/hosts.yml",
    "~/.aws/credentials",
    "~/.azure",
    "~/.kube/config",
    "~/.docker/config.json",
];

pub const ALWAYS_DENIED_WRITE: &[&str] = &[
    "/etc",
    "/usr",
    "/boot",
    "/sbin",
    "/bin",
    "/lib",
    "/lib64",
    "~/.ssh",
    "~/.gnupg",
    "~/.gpg",
    "~/.bashrc",
    "~/.zshrc",
    "~/.profile",
    "~/.bash_profile",
];

/// Filename patterns that indicate sensitive files (matched anywhere in path).
pub const SENSITIVE_FILE_PATTERNS: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.staging",
    "credentials.json",
    "secrets.yaml",
    "secrets.yml",
    "id_rsa",
    "id_ed25519",
];

impl Default for FilesystemPolicy {
    fn default() -> Self {
        Self {
            allowed_read: vec![],
            allowed_write: vec![],
            denied_read: ALWAYS_DENIED_READ.iter().map(|s| s.to_string()).collect(),
            denied_write: ALWAYS_DENIED_WRITE.iter().map(|s| s.to_string()).collect(),
        }
    }
}

// ── Network Policy ──────────────────────────────────────────────────

/// Network access control policy. Default: everything denied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicy {
    /// Whether network access is allowed at all. Default: false.
    pub allow_network: bool,

    /// Domains explicitly allowed (whitelist). Only checked if allow_network=true.
    pub allowed_domains: Vec<String>,

    /// Domains explicitly denied (blacklist). Takes precedence over allowed.
    pub denied_domains: Vec<String>,

    /// Whether DNS resolution is allowed. Default: false.
    /// Even with allow_network=true, DNS can be independently blocked
    /// to prevent DNS exfiltration.
    pub allow_dns: bool,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            allow_network: false,
            allowed_domains: vec![],
            denied_domains: vec![],
            allow_dns: false,
        }
    }
}

// ── Process Policy ──────────────────────────────────────────────────

/// Process isolation and resource limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessPolicy {
    /// Maximum number of child processes. 0 = use system default.
    pub max_processes: u32,

    /// Maximum memory in bytes. 0 = use system default.
    pub max_memory_bytes: u64,

    /// Maximum CPU time in seconds. 0 = use system default.
    pub max_cpu_seconds: u64,

    /// Maximum file size in bytes that can be created. 0 = use system default.
    pub max_file_size_bytes: u64,

    /// Environment variables allowed in the sandbox.
    /// Everything else is stripped before execution.
    pub allowed_env_vars: Vec<String>,
}

/// Default allowed environment variables (safe, no secrets).
pub const DEFAULT_ALLOWED_ENV_VARS: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TERM", "SHELL",
    "TMPDIR", "TMP", "TEMP", "XDG_RUNTIME_DIR",
    "LD_LIBRARY_PATH", "LD_PRELOAD",
];

/// Environment variable prefixes that are ALWAYS stripped.
pub const ALWAYS_STRIPPED_ENV_PREFIXES: &[&str] = &[
    "AWS_",
    "AZURE_",
    "GCP_",
    "GOOGLE_",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "GITLAB_",
    "NPM_TOKEN",
    "DOCKER_",
    "KUBECONFIG",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "CLAUDE_",
    "HF_TOKEN",
];

impl Default for ProcessPolicy {
    fn default() -> Self {
        Self {
            max_processes: 64,
            max_memory_bytes: 512 * 1024 * 1024, // 512 MB
            max_cpu_seconds: 120,
            max_file_size_bytes: 100 * 1024 * 1024, // 100 MB
            allowed_env_vars: DEFAULT_ALLOWED_ENV_VARS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

// ── Audit Policy ────────────────────────────────────────────────────

/// Configuration for sandbox audit logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPolicy {
    /// Log every command executed in the sandbox.
    pub log_commands: bool,

    /// Log every sandbox violation (blocked access).
    pub log_violations: bool,

    /// Log network activity (connections, DNS).
    pub log_network: bool,
}

impl Default for AuditPolicy {
    fn default() -> Self {
        Self {
            log_commands: true,
            log_violations: true,
            log_network: true,
        }
    }
}

// ── Execution Result ────────────────────────────────────────────────

/// Result of a sandboxed execution.
///
/// **Invariant:** If `success` is true, `violations` MUST be empty.
/// Use `SandboxResult::new()` to enforce this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    /// Whether the command completed successfully.
    pub success: bool,

    /// Process exit code.
    pub exit_code: i32,

    /// Captured stdout.
    pub stdout: String,

    /// Captured stderr.
    pub stderr: String,

    /// Sandbox violations that occurred during execution.
    /// Empty if success=true.
    pub violations: Vec<SandboxViolation>,

    /// Audit entries generated during execution.
    pub audit_entries: Vec<AuditEntry>,
}

impl SandboxResult {
    /// Create a successful result (no violations).
    pub fn success(exit_code: i32, stdout: String, stderr: String, audit_entries: Vec<AuditEntry>) -> Self {
        Self {
            success: true,
            exit_code,
            stdout,
            stderr,
            violations: vec![],
            audit_entries,
        }
    }

    /// Create a failed result with violations.
    pub fn failed(
        exit_code: i32,
        stdout: String,
        stderr: String,
        violations: Vec<SandboxViolation>,
        audit_entries: Vec<AuditEntry>,
    ) -> Self {
        Self {
            success: false,
            exit_code,
            stdout,
            stderr,
            violations,
            audit_entries,
        }
    }

    /// Create a result from a sandbox-blocked execution (command never ran).
    pub fn blocked(violation: SandboxViolation) -> Self {
        Self {
            success: false,
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("Sandbox blocked: {violation}"),
            violations: vec![violation],
            audit_entries: vec![],
        }
    }
}

// ── Violations ──────────────────────────────────────────────────────

/// A security violation detected by the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SandboxViolation {
    /// Attempted to access a denied filesystem path.
    FilesystemAccess {
        path: String,
        operation: FilesystemOp,
        denied_by: String,
    },

    /// Attempted to access the network when denied.
    NetworkAccess {
        address: String,
        port: u16,
        denied_by: String,
    },

    /// Exceeded a resource limit.
    ResourceExceeded {
        resource: String,
        limit: u64,
        attempted: u64,
    },

    /// Attempted to leak an environment variable.
    EnvironmentLeak {
        var_name: String,
    },

    /// Sandbox is unavailable on this platform.
    Unavailable {
        reason: String,
    },
}

impl std::fmt::Display for SandboxViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FilesystemAccess { path, operation, denied_by } => {
                write!(f, "filesystem {operation} denied on '{path}' by {denied_by}")
            }
            Self::NetworkAccess { address, port, denied_by } => {
                write!(f, "network access to {address}:{port} denied by {denied_by}")
            }
            Self::ResourceExceeded { resource, limit, attempted } => {
                write!(f, "resource '{resource}' exceeded: limit={limit}, attempted={attempted}")
            }
            Self::EnvironmentLeak { var_name } => {
                write!(f, "environment variable '{var_name}' leaked")
            }
            Self::Unavailable { reason } => {
                write!(f, "sandbox unavailable: {reason}")
            }
        }
    }
}

/// Filesystem operation type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemOp {
    Read,
    Write,
    Execute,
    Delete,
}

impl std::fmt::Display for FilesystemOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Write => write!(f, "write"),
            Self::Execute => write!(f, "execute"),
            Self::Delete => write!(f, "delete"),
        }
    }
}

// ── Audit Entries ───────────────────────────────────────────────────

/// A single audit entry from sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// ISO 8601 timestamp.
    pub timestamp: String,

    /// Type of event.
    pub event_type: AuditEventType,

    /// Human-readable detail.
    pub detail: String,

    /// Arbitrary structured metadata.
    pub metadata: serde_json::Value,
}

/// Types of sandbox audit events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    /// Sandbox initialized for a command.
    SandboxInit,
    /// Command started executing.
    CommandStart,
    /// Command finished.
    CommandEnd,
    /// A violation was detected and blocked.
    ViolationBlocked,
    /// A resource limit was applied.
    ResourceLimitApplied,
    /// An environment variable was stripped.
    EnvVarStripped,
    /// Sandbox cleanup completed.
    Cleanup,
}

// ── Errors ──────────────────────────────────────────────────────────

/// Sandbox-specific errors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SandboxError {
    /// Sandbox is not available on this platform/kernel.
    Unavailable(String),

    /// Sandbox initialization failed.
    InitFailed(String),

    /// Configuration is invalid.
    InvalidConfig(String),

    /// A policy violation prevented execution.
    PolicyViolation(SandboxViolation),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "sandbox unavailable: {reason}"),
            Self::InitFailed(reason) => write!(f, "sandbox init failed: {reason}"),
            Self::InvalidConfig(reason) => write!(f, "invalid sandbox config: {reason}"),
            Self::PolicyViolation(v) => write!(f, "policy violation: {v}"),
        }
    }
}

impl std::error::Error for SandboxError {}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default values ──────────────────────────────────────────

    #[test]
    fn sandbox_config_default_is_secure() {
        let config = SandboxConfig::default();
        assert!(config.enabled);
        assert!(config.fail_if_unavailable);
        assert!(!config.network.allow_network);
        assert!(!config.network.allow_dns);
        assert!(config.audit.log_commands);
        assert!(config.audit.log_violations);
    }

    #[test]
    fn filesystem_policy_default_has_denied_paths() {
        let policy = FilesystemPolicy::default();
        assert!(policy.denied_read.iter().any(|p| p.contains(".ssh")));
        assert!(policy.denied_write.iter().any(|p| p.contains("/etc")));
        assert!(policy.allowed_read.is_empty());
        assert!(policy.allowed_write.is_empty());
    }

    #[test]
    fn network_policy_default_denies_everything() {
        let policy = NetworkPolicy::default();
        assert!(!policy.allow_network);
        assert!(!policy.allow_dns);
        assert!(policy.allowed_domains.is_empty());
    }

    #[test]
    fn process_policy_default_has_reasonable_limits() {
        let policy = ProcessPolicy::default();
        assert_eq!(policy.max_processes, 64);
        assert_eq!(policy.max_memory_bytes, 512 * 1024 * 1024);
        assert_eq!(policy.max_cpu_seconds, 120);
        assert_eq!(policy.max_file_size_bytes, 100 * 1024 * 1024);
        assert!(!policy.allowed_env_vars.is_empty());
        assert!(policy.allowed_env_vars.contains(&"PATH".to_string()));
    }

    // ── Serde round-trip ────────────────────────────────────────

    #[test]
    fn sandbox_config_serde_roundtrip() {
        let config = SandboxConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: SandboxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.enabled, deserialized.enabled);
        assert_eq!(config.fail_if_unavailable, deserialized.fail_if_unavailable);
        assert_eq!(config.network.allow_network, deserialized.network.allow_network);
    }

    #[test]
    fn sandbox_violation_serde_roundtrip() {
        let violation = SandboxViolation::FilesystemAccess {
            path: "/home/user/.ssh/id_rsa".to_string(),
            operation: FilesystemOp::Read,
            denied_by: "ALWAYS_DENIED_READ".to_string(),
        };
        let json = serde_json::to_string(&violation).unwrap();
        let deserialized: SandboxViolation = serde_json::from_str(&json).unwrap();
        assert_eq!(violation, deserialized);
    }

    #[test]
    fn sandbox_error_serde_roundtrip() {
        let error = SandboxError::Unavailable("kernel < 5.13".to_string());
        let json = serde_json::to_string(&error).unwrap();
        let deserialized: SandboxError = serde_json::from_str(&json).unwrap();
        assert_eq!(error, deserialized);
    }

    #[test]
    fn audit_event_type_serde_roundtrip() {
        let event = AuditEventType::ViolationBlocked;
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, "\"violation_blocked\"");
        let deserialized: AuditEventType = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    // ── SandboxResult invariants ────────────────────────────────

    #[test]
    fn sandbox_result_success_has_no_violations() {
        let result = SandboxResult::success(0, "output".to_string(), String::new(), vec![]);
        assert!(result.success);
        assert!(result.violations.is_empty());
    }

    #[test]
    fn sandbox_result_failed_has_violations() {
        let result = SandboxResult::failed(
            1,
            String::new(),
            "error".to_string(),
            vec![SandboxViolation::Unavailable {
                reason: "test".to_string(),
            }],
            vec![],
        );
        assert!(!result.success);
        assert!(!result.violations.is_empty());
    }

    #[test]
    fn sandbox_result_blocked_has_violation_and_negative_exit() {
        let result = SandboxResult::blocked(SandboxViolation::FilesystemAccess {
            path: "~/.ssh".to_string(),
            operation: FilesystemOp::Read,
            denied_by: "policy".to_string(),
        });
        assert!(!result.success);
        assert_eq!(result.exit_code, -1);
        assert_eq!(result.violations.len(), 1);
        assert!(result.stderr.contains("Sandbox blocked"));
    }

    // ── Display / formatting ────────────────────────────────────

    #[test]
    fn sandbox_violation_display() {
        let v = SandboxViolation::FilesystemAccess {
            path: "/etc/passwd".to_string(),
            operation: FilesystemOp::Read,
            denied_by: "policy".to_string(),
        };
        let display = format!("{v}");
        assert!(display.contains("filesystem"));
        assert!(display.contains("/etc/passwd"));
        assert!(display.contains("read"));
    }

    #[test]
    fn sandbox_error_display() {
        let e = SandboxError::Unavailable("no landlock".to_string());
        let display = format!("{e}");
        assert!(display.contains("unavailable"));
        assert!(display.contains("no landlock"));
    }

    // ── Edge cases ──────────────────────────────────────────────

    #[test]
    fn process_policy_zero_values_are_valid() {
        // Zero means "use system default" — not an error
        let policy = ProcessPolicy {
            max_processes: 0,
            max_memory_bytes: 0,
            max_cpu_seconds: 0,
            max_file_size_bytes: 0,
            allowed_env_vars: vec![],
        };
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: ProcessPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_processes, 0);
    }

    #[test]
    fn always_denied_paths_cover_critical_secrets() {
        assert!(ALWAYS_DENIED_READ.contains(&"~/.ssh"));
        assert!(ALWAYS_DENIED_READ.contains(&"~/.gnupg"));
        assert!(ALWAYS_DENIED_READ.contains(&"~/.aws/credentials"));
        assert!(ALWAYS_DENIED_WRITE.contains(&"/etc"));
        assert!(ALWAYS_DENIED_WRITE.contains(&"~/.ssh"));
    }

    #[test]
    fn always_stripped_env_covers_cloud_tokens() {
        assert!(ALWAYS_STRIPPED_ENV_PREFIXES.contains(&"AWS_"));
        assert!(ALWAYS_STRIPPED_ENV_PREFIXES.contains(&"GITHUB_TOKEN"));
        assert!(ALWAYS_STRIPPED_ENV_PREFIXES.contains(&"OPENAI_API_KEY"));
        assert!(ALWAYS_STRIPPED_ENV_PREFIXES.contains(&"ANTHROPIC_API_KEY"));
    }

    #[test]
    fn sensitive_file_patterns_cover_env_files() {
        assert!(SENSITIVE_FILE_PATTERNS.contains(&".env"));
        assert!(SENSITIVE_FILE_PATTERNS.contains(&".env.production"));
        assert!(SENSITIVE_FILE_PATTERNS.contains(&"credentials.json"));
    }

    #[test]
    fn filesystem_op_all_variants_display() {
        assert_eq!(format!("{}", FilesystemOp::Read), "read");
        assert_eq!(format!("{}", FilesystemOp::Write), "write");
        assert_eq!(format!("{}", FilesystemOp::Execute), "execute");
        assert_eq!(format!("{}", FilesystemOp::Delete), "delete");
    }
}
