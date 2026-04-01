//! Kernel feature detection for sandbox capabilities.

/// Result of probing the kernel for sandbox support.
#[derive(Debug, Clone)]
pub struct SandboxCapabilities {
    /// Whether landlock is available (Linux 5.13+).
    pub landlock_available: bool,
    /// Landlock ABI version (0 = unavailable).
    pub landlock_abi_version: i32,
    /// Whether user + network namespaces are available (for network isolation).
    pub net_ns_available: bool,
}

/// Probe the running kernel for sandbox capabilities.
///
/// This function never panics — it returns a capabilities struct
/// with all fields set to false/0 if detection fails or the
/// platform is unsupported.
pub fn probe_kernel() -> SandboxCapabilities {
    #[cfg(target_os = "linux")]
    {
        probe_linux()
    }
    #[cfg(not(target_os = "linux"))]
    {
        SandboxCapabilities {
            landlock_available: false,
            landlock_abi_version: 0,
            net_ns_available: false,
        }
    }
}

#[cfg(target_os = "linux")]
fn probe_linux() -> SandboxCapabilities {
    // Try to detect landlock ABI by attempting to create a ruleset.
    // The landlock crate handles this internally, but we can check
    // by probing the ABI version through the syscall.
    let abi_version = detect_landlock_abi();
    let net_ns_available = detect_user_net_ns();

    SandboxCapabilities {
        landlock_available: abi_version > 0,
        landlock_abi_version: abi_version,
        net_ns_available,
    }
}

#[cfg(target_os = "linux")]
fn detect_landlock_abi() -> i32 {
    // Use the landlock syscall directly to check ABI version.
    // landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)
    // returns the highest supported ABI version, or -1 with errno on failure.
    //
    // We use libc directly here because the landlock crate doesn't expose
    // a simple "what ABI is available?" function.
    unsafe {
        let ret = libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<libc::c_void>(),
            0usize,
            1u32, // LANDLOCK_CREATE_RULESET_VERSION
        );
        if ret < 0 { 0 } else { ret as i32 }
    }
}

/// Detect if user + network namespaces are available.
/// Tests by attempting unshare in a forked child.
#[cfg(target_os = "linux")]
fn detect_user_net_ns() -> bool {
    // Check if unprivileged user namespaces are enabled
    let userns_enabled = std::fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone")
        .map(|s| s.trim() == "1")
        .unwrap_or(false);

    if !userns_enabled {
        return false;
    }

    // Try a quick unshare to verify it actually works
    let result = std::process::Command::new("unshare")
        .args(["--user", "--net", "--", "true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    matches!(result, Ok(status) if status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_kernel_does_not_panic() {
        let caps = probe_kernel();
        // On any platform, this should return without panicking
        let _ = caps.landlock_available;
        let _ = caps.landlock_abi_version;
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn probe_linux_detects_landlock() {
        let caps = probe_kernel();
        // On our dev kernel (6.8.0), landlock should be available
        // This test may be #[ignore]d in CI without kernel 5.13+
        if caps.landlock_available {
            assert!(caps.landlock_abi_version >= 1);
        }
    }
}
