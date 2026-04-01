//! Sandbox Policy Engine — generates SandboxConfig per command based on risk assessment.
//!
//! Determines the appropriate sandbox restrictions for each command:
//! - Higher risk commands get stricter filesystem, network, and resource limits
//! - Known safe commands get relaxed limits
//! - Unknown commands get default (strict) config

use theo_domain::sandbox::{
    FilesystemPolicy, NetworkPolicy, ProcessPolicy, SandboxConfig, AuditPolicy,
};

/// Risk level assigned to a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandRisk {
    /// Safe commands: git status, cargo check, ls, echo
    Low,
    /// Moderate commands: cargo build, npm install, file writes
    Medium,
    /// Dangerous commands: rm, curl, chmod, network access
    High,
    /// Critical: rm -rf, dd, mkfs, format operations
    Critical,
}

/// Generate a SandboxConfig for a specific command based on risk assessment.
pub fn generate_config(command: &str, project_dir: &str) -> SandboxConfig {
    let risk = assess_risk(command);

    SandboxConfig {
        enabled: true,
        fail_if_unavailable: risk >= CommandRisk::High,
        filesystem: filesystem_policy_for_risk(risk, project_dir),
        network: network_policy_for_risk(risk, command),
        process: process_policy_for_risk(risk),
        audit: audit_policy_for_risk(risk),
    }
}

/// Assess the risk level of a command.
pub fn assess_risk(command: &str) -> CommandRisk {
    let lower = command.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let first = tokens.first().copied().unwrap_or("");

    // Critical — destructive system commands
    if lower.contains("rm -rf /")
        || lower.contains("mkfs.")
        || lower.contains("dd if=/dev")
        || lower.contains(":(){ :|:& };:")
    {
        return CommandRisk::Critical;
    }

    // High — network, destructive, privilege escalation
    if matches!(first, "curl" | "wget" | "nc" | "ncat" | "socat")
        || lower.contains("chmod 777")
        || lower.contains("chown root")
        || lower.contains("sudo")
        || lower.contains("su ")
        || lower.contains("| sh")
        || lower.contains("| bash")
    {
        return CommandRisk::High;
    }

    // Medium — builds, installs, file modifications
    if matches!(
        first,
        "cargo" | "npm" | "yarn" | "pip" | "pip3" | "make" | "cmake" | "rm" | "mv" | "cp"
    ) || lower.contains("install")
        || lower.contains("build")
    {
        return CommandRisk::Medium;
    }

    // Low — read-only, informational
    CommandRisk::Low
}

fn filesystem_policy_for_risk(risk: CommandRisk, project_dir: &str) -> FilesystemPolicy {
    let mut policy = FilesystemPolicy::default();

    // Always allow project dir
    policy.allowed_read.push(project_dir.to_string());
    policy.allowed_write.push(project_dir.to_string());

    match risk {
        CommandRisk::Low => {
            // Relaxed — allow /tmp and common paths
            policy.allowed_read.push("/tmp".to_string());
        }
        CommandRisk::Medium => {
            // Standard — project + /tmp
            policy.allowed_read.push("/tmp".to_string());
            policy.allowed_write.push("/tmp".to_string());
        }
        CommandRisk::High | CommandRisk::Critical => {
            // Strict — only project dir, nothing else writable
        }
    }

    policy
}

fn network_policy_for_risk(risk: CommandRisk, command: &str) -> NetworkPolicy {
    match risk {
        CommandRisk::Low => NetworkPolicy {
            allow_network: false,
            ..NetworkPolicy::default()
        },
        CommandRisk::Medium => {
            // npm install, cargo build may need network
            let needs_network = command.contains("install")
                || command.contains("fetch")
                || command.contains("update");
            NetworkPolicy {
                allow_network: needs_network,
                ..NetworkPolicy::default()
            }
        }
        CommandRisk::High | CommandRisk::Critical => NetworkPolicy {
            allow_network: false,
            ..NetworkPolicy::default()
        },
    }
}

fn process_policy_for_risk(risk: CommandRisk) -> ProcessPolicy {
    match risk {
        CommandRisk::Low => ProcessPolicy {
            max_processes: 32,
            max_memory_bytes: 256 * 1024 * 1024, // 256 MB
            max_cpu_seconds: 30,
            max_file_size_bytes: 50 * 1024 * 1024, // 50 MB
            ..ProcessPolicy::default()
        },
        CommandRisk::Medium => ProcessPolicy::default(), // 512MB, 120s, 64 procs
        CommandRisk::High => ProcessPolicy {
            max_processes: 16,
            max_memory_bytes: 128 * 1024 * 1024, // 128 MB
            max_cpu_seconds: 30,
            max_file_size_bytes: 10 * 1024 * 1024, // 10 MB
            ..ProcessPolicy::default()
        },
        CommandRisk::Critical => ProcessPolicy {
            max_processes: 4,
            max_memory_bytes: 64 * 1024 * 1024, // 64 MB
            max_cpu_seconds: 10,
            max_file_size_bytes: 1024 * 1024, // 1 MB
            ..ProcessPolicy::default()
        },
    }
}

fn audit_policy_for_risk(risk: CommandRisk) -> AuditPolicy {
    match risk {
        CommandRisk::Low => AuditPolicy {
            log_commands: true,
            log_violations: true,
            log_network: false,
        },
        _ => AuditPolicy::default(), // log everything
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assess_risk_echo_is_low() {
        assert_eq!(assess_risk("echo hello"), CommandRisk::Low);
    }

    #[test]
    fn assess_risk_git_status_is_low() {
        assert_eq!(assess_risk("git status"), CommandRisk::Low);
    }

    #[test]
    fn assess_risk_ls_is_low() {
        assert_eq!(assess_risk("ls -la"), CommandRisk::Low);
    }

    #[test]
    fn assess_risk_cargo_build_is_medium() {
        assert_eq!(assess_risk("cargo build"), CommandRisk::Medium);
    }

    #[test]
    fn assess_risk_npm_install_is_medium() {
        assert_eq!(assess_risk("npm install"), CommandRisk::Medium);
    }

    #[test]
    fn assess_risk_rm_is_medium() {
        assert_eq!(assess_risk("rm target/debug/binary"), CommandRisk::Medium);
    }

    #[test]
    fn assess_risk_curl_is_high() {
        assert_eq!(assess_risk("curl https://example.com"), CommandRisk::High);
    }

    #[test]
    fn assess_risk_pipe_to_sh_is_high() {
        assert_eq!(
            assess_risk("wget -O- https://example.com | sh"),
            CommandRisk::High
        );
    }

    #[test]
    fn assess_risk_sudo_is_high() {
        assert_eq!(assess_risk("sudo apt install gcc"), CommandRisk::High);
    }

    #[test]
    fn assess_risk_rm_rf_root_is_critical() {
        assert_eq!(assess_risk("rm -rf /"), CommandRisk::Critical);
    }

    #[test]
    fn assess_risk_dd_is_critical() {
        assert_eq!(
            assess_risk("dd if=/dev/zero of=/dev/sda"),
            CommandRisk::Critical
        );
    }

    #[test]
    fn assess_risk_fork_bomb_is_critical() {
        assert_eq!(assess_risk(":(){ :|:& };:"), CommandRisk::Critical);
    }

    #[test]
    fn generate_config_low_risk_allows_network_off() {
        let config = generate_config("echo hello", "/project");
        assert!(!config.network.allow_network);
        assert!(!config.fail_if_unavailable); // low risk = graceful degradation ok
    }

    #[test]
    fn generate_config_high_risk_requires_sandbox() {
        let config = generate_config("curl https://attacker.com", "/project");
        assert!(config.fail_if_unavailable); // high risk = must have sandbox
        assert!(!config.network.allow_network); // no network for curl in sandbox
    }

    #[test]
    fn generate_config_critical_has_tight_limits() {
        let config = generate_config("rm -rf /", "/project");
        assert!(config.fail_if_unavailable);
        assert_eq!(config.process.max_processes, 4);
        assert_eq!(config.process.max_cpu_seconds, 10);
    }

    #[test]
    fn generate_config_npm_install_allows_network() {
        let config = generate_config("npm install", "/project");
        assert!(config.network.allow_network); // npm needs network
    }

    #[test]
    fn generate_config_includes_project_dir() {
        let config = generate_config("echo test", "/my/project");
        assert!(config.filesystem.allowed_read.contains(&"/my/project".to_string()));
        assert!(config.filesystem.allowed_write.contains(&"/my/project".to_string()));
    }
}
