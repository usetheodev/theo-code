//! Denied paths enforcement — checks if a path matches the denied list
//! from SandboxConfig's FilesystemPolicy.

use std::path::Path;
use theo_domain::sandbox::{
    FilesystemOp, FilesystemPolicy, SandboxViolation, ALWAYS_DENIED_READ, ALWAYS_DENIED_WRITE,
    SENSITIVE_FILE_PATTERNS,
};

/// Check if a path is denied for a specific operation.
///
/// Returns `Some(violation)` if the path is denied, `None` if allowed.
pub fn check_path_denied(
    path: &Path,
    operation: FilesystemOp,
    policy: &FilesystemPolicy,
) -> Option<SandboxViolation> {
    let path_str = path.display().to_string();
    let expanded = expand_home(&path_str);

    // Check hardcoded always-denied list first
    let always_denied = match operation {
        FilesystemOp::Read => ALWAYS_DENIED_READ,
        FilesystemOp::Write | FilesystemOp::Delete => ALWAYS_DENIED_WRITE,
        FilesystemOp::Execute => ALWAYS_DENIED_WRITE, // execute implies potential write
    };

    for denied in always_denied {
        let denied_expanded = expand_home(denied);
        if path_starts_with_or_equals(&expanded, &denied_expanded) {
            return Some(SandboxViolation::FilesystemAccess {
                path: path_str,
                operation,
                denied_by: format!("ALWAYS_DENIED: {denied}"),
            });
        }
    }

    // Check sensitive file patterns (match against filename only)
    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
        for pattern in SENSITIVE_FILE_PATTERNS {
            if filename == *pattern || filename.starts_with(pattern) {
                return Some(SandboxViolation::FilesystemAccess {
                    path: path_str,
                    operation,
                    denied_by: format!("SENSITIVE_FILE: {pattern}"),
                });
            }
        }
    }

    // Check policy-specific denied lists
    let policy_denied = match operation {
        FilesystemOp::Read => &policy.denied_read,
        FilesystemOp::Write | FilesystemOp::Delete | FilesystemOp::Execute => &policy.denied_write,
    };

    for denied in policy_denied {
        let denied_expanded = expand_home(denied);
        if path_starts_with_or_equals(&expanded, &denied_expanded) {
            return Some(SandboxViolation::FilesystemAccess {
                path: path_str,
                operation,
                denied_by: format!("policy: {denied}"),
            });
        }
    }

    None
}

/// Expand ~ to the user's home directory.
fn expand_home(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return path.replacen('~', &home, 1);
        }
    }
    path.to_string()
}

/// Check if path starts with or equals the prefix (after normalization).
fn path_starts_with_or_equals(path: &str, prefix: &str) -> bool {
    let p = Path::new(path);
    let pfx = Path::new(prefix);
    p.starts_with(pfx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn default_policy() -> FilesystemPolicy {
        FilesystemPolicy::default()
    }

    fn home_dir() -> String {
        std::env::var("HOME").unwrap_or_else(|_| "/home/testuser".to_string())
    }

    // ── Always denied paths ─────────────────────────────────────

    #[test]
    fn denies_ssh_read() {
        let home = home_dir();
        let path = PathBuf::from(format!("{home}/.ssh/id_rsa"));
        let result = check_path_denied(&path, FilesystemOp::Read, &default_policy());
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), SandboxViolation::FilesystemAccess { .. }));
    }

    #[test]
    fn denies_gnupg_read() {
        let home = home_dir();
        let path = PathBuf::from(format!("{home}/.gnupg/secring.gpg"));
        let result = check_path_denied(&path, FilesystemOp::Read, &default_policy());
        assert!(result.is_some());
    }

    #[test]
    fn denies_aws_credentials_read() {
        let home = home_dir();
        let path = PathBuf::from(format!("{home}/.aws/credentials"));
        let result = check_path_denied(&path, FilesystemOp::Read, &default_policy());
        assert!(result.is_some());
    }

    #[test]
    fn denies_etc_write() {
        let path = PathBuf::from("/etc/passwd");
        let result = check_path_denied(&path, FilesystemOp::Write, &default_policy());
        assert!(result.is_some());
    }

    #[test]
    fn denies_usr_write() {
        let path = PathBuf::from("/usr/bin/something");
        let result = check_path_denied(&path, FilesystemOp::Write, &default_policy());
        assert!(result.is_some());
    }

    // ── Sensitive file patterns ─────────────────────────────────

    #[test]
    fn denies_env_file() {
        let path = PathBuf::from("/project/.env");
        let result = check_path_denied(&path, FilesystemOp::Read, &default_policy());
        assert!(result.is_some());
    }

    #[test]
    fn denies_env_production() {
        let path = PathBuf::from("/project/.env.production");
        let result = check_path_denied(&path, FilesystemOp::Read, &default_policy());
        assert!(result.is_some());
    }

    #[test]
    fn denies_credentials_json() {
        let path = PathBuf::from("/project/credentials.json");
        let result = check_path_denied(&path, FilesystemOp::Read, &default_policy());
        assert!(result.is_some());
    }

    // ── Allowed paths ───────────────────────────────────────────

    #[test]
    fn allows_project_src_read() {
        let path = PathBuf::from("/home/user/project/src/main.rs");
        let result = check_path_denied(&path, FilesystemOp::Read, &default_policy());
        assert!(result.is_none());
    }

    #[test]
    fn allows_tmp_write() {
        let path = PathBuf::from("/tmp/output.txt");
        let result = check_path_denied(&path, FilesystemOp::Write, &default_policy());
        assert!(result.is_none());
    }

    #[test]
    fn allows_project_target_write() {
        let path = PathBuf::from("/home/user/project/target/debug/binary");
        let result = check_path_denied(&path, FilesystemOp::Write, &default_policy());
        assert!(result.is_none());
    }

    // ── Policy-specific denied ──────────────────────────────────

    #[test]
    fn policy_denied_write_blocks() {
        let mut policy = default_policy();
        policy.denied_write.push("/custom/blocked".to_string());
        let path = PathBuf::from("/custom/blocked/file.txt");
        let result = check_path_denied(&path, FilesystemOp::Write, &policy);
        assert!(result.is_some());
    }
}
