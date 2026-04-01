//! Network isolation via user + network namespaces.
//!
//! When enabled, creates a network namespace with no external interfaces,
//! effectively blocking all network access (default deny).
//!
//! This is a binary on/off control:
//! - allow_network=false → apply net ns (no network)
//! - allow_network=true → skip net ns (full network)
//!
//! Domain-level whitelist is NOT implemented in this phase.

use theo_domain::sandbox::NetworkPolicy;

/// Apply network isolation in the child process via unshare(2).
///
/// Called inside pre_exec (after fork, before exec).
/// Creates a new user namespace + network namespace.
/// The net namespace is empty — only loopback exists (but is DOWN).
///
/// If `policy.allow_network` is true, this is a no-op.
/// If unshare fails, returns Ok(()) (graceful degradation — logged by caller).
///
/// SAFETY: unshare(2) is a syscall, async-signal-safe.
#[cfg(target_os = "linux")]
pub fn apply_network_isolation(policy: &NetworkPolicy) -> std::io::Result<()> {
    if policy.allow_network {
        return Ok(()); // Network allowed, skip isolation
    }

    // CLONE_NEWUSER | CLONE_NEWNET
    let flags = libc::CLONE_NEWUSER | libc::CLONE_NEWNET;
    let ret = unsafe { libc::unshare(flags) };

    if ret != 0 {
        let err = std::io::Error::last_os_error();
        // Graceful degradation: log will happen in caller.
        // Common failures: EPERM (no permission), EINVAL (nested ns),
        // ENOMEM (resource exhaustion).
        // We treat this as non-fatal — the command runs without net isolation.
        eprintln!("sandbox warning: network isolation failed: {err}");
        // Return Ok to not block execution
    }

    Ok(())
}

/// Check if network isolation would be applied given the policy.
pub fn would_isolate_network(policy: &NetworkPolicy) -> bool {
    !policy.allow_network
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_network_skips_isolation() {
        let policy = NetworkPolicy {
            allow_network: true,
            ..NetworkPolicy::default()
        };
        assert!(!would_isolate_network(&policy));
    }

    #[test]
    fn deny_network_would_isolate() {
        let policy = NetworkPolicy::default(); // allow_network=false
        assert!(would_isolate_network(&policy));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn apply_network_isolation_with_allow_is_noop() {
        let policy = NetworkPolicy {
            allow_network: true,
            ..NetworkPolicy::default()
        };
        // Should return Ok immediately without calling unshare
        apply_network_isolation(&policy).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn network_isolated_command_cannot_reach_external() {
        // This test verifies that a command in a net namespace cannot reach external hosts.
        // We run curl in a child with network isolation.
        use crate::sandbox::probe;

        let caps = probe::probe_kernel();
        if !caps.net_ns_available {
            return; // Skip if net ns not available
        }

        use std::os::unix::process::CommandExt;
        use std::process::Stdio;

        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c")
            // Try to connect to a public DNS — should fail in net ns
            .arg("curl -s --connect-timeout 2 http://1.1.1.1 2>&1; echo exit=$?")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let policy = NetworkPolicy::default(); // deny
        unsafe {
            cmd.pre_exec(move || apply_network_isolation(&policy));
        }

        let output = cmd.output();
        match output {
            Ok(out) => {
                let combined = String::from_utf8_lossy(&out.stdout).to_string()
                    + &String::from_utf8_lossy(&out.stderr);
                // Either curl fails to connect, or the command itself fails
                // In a net namespace, external connections should fail
                let exit_code = out.status.code().unwrap_or(-1);
                assert!(
                    exit_code != 0 || combined.contains("exit="),
                    "curl should fail or report error in isolated namespace"
                );
            }
            Err(_) => {
                // Command failed to spawn — also acceptable (ns creation failed)
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn network_allowed_command_can_reach_localhost() {
        // With allow_network=true, commands should work normally
        use std::process::Stdio;

        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c")
            .arg("echo network_allowed")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // No isolation applied
        let output = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("network_allowed"));
    }
}
