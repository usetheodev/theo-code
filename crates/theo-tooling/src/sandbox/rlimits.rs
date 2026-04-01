//! Resource limits via setrlimit — applied in child process before exec.
//!
//! Uses libc::setrlimit directly (async-signal-safe syscall).
//! Value of 0 in ProcessPolicy means "use system default" (skip that limit).

use theo_domain::sandbox::ProcessPolicy;

/// Apply resource limits from ProcessPolicy.
///
/// Called inside pre_exec (after fork, before exec).
/// Only calls setrlimit for non-zero values.
///
/// SAFETY: libc::setrlimit is async-signal-safe.
#[cfg(target_os = "linux")]
pub fn apply_rlimits(policy: &ProcessPolicy) -> std::io::Result<()> {
    if policy.max_cpu_seconds > 0 {
        set_rlimit(libc::RLIMIT_CPU, policy.max_cpu_seconds)?;
    }
    if policy.max_memory_bytes > 0 {
        set_rlimit(libc::RLIMIT_AS, policy.max_memory_bytes)?;
    }
    if policy.max_file_size_bytes > 0 {
        set_rlimit(libc::RLIMIT_FSIZE, policy.max_file_size_bytes)?;
    }
    if policy.max_processes > 0 {
        set_rlimit(libc::RLIMIT_NPROC, policy.max_processes as u64)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_rlimit(resource: libc::__rlimit_resource_t, value: u64) -> std::io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    let ret = unsafe { libc::setrlimit(resource, &limit) };
    if ret != 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Read current rlimit value for a resource (for testing/verification).
#[cfg(target_os = "linux")]
pub fn get_rlimit(resource: libc::__rlimit_resource_t) -> std::io::Result<(u64, u64)> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let ret = unsafe { libc::getrlimit(resource, &mut limit) };
    if ret != 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok((limit.rlim_cur as u64, limit.rlim_max as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn zero_values_do_not_apply_limits() {
        let policy = ProcessPolicy {
            max_processes: 0,
            max_memory_bytes: 0,
            max_cpu_seconds: 0,
            max_file_size_bytes: 0,
            allowed_env_vars: vec![],
        };
        // Should succeed without changing any limits
        apply_rlimits(&policy).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn apply_fsize_limit() {
        // We can safely test RLIMIT_FSIZE in the test process
        // because it only affects new file writes, not existing reads
        let policy = ProcessPolicy {
            max_processes: 0,
            max_memory_bytes: 0,
            max_cpu_seconds: 0,
            max_file_size_bytes: 100 * 1024 * 1024, // 100MB
            allowed_env_vars: vec![],
        };

        // Read current limits first
        let (cur_before, _) = get_rlimit(libc::RLIMIT_FSIZE).unwrap();

        // Apply — this changes the test process's limits!
        // Only safe because we set a high value that won't affect normal test operation
        apply_rlimits(&policy).unwrap();

        let (cur_after, _) = get_rlimit(libc::RLIMIT_FSIZE).unwrap();
        assert_eq!(cur_after, 100 * 1024 * 1024);

        // Restore original limit to avoid affecting other tests
        let restore = libc::rlimit {
            rlim_cur: cur_before as libc::rlim_t,
            rlim_max: cur_before as libc::rlim_t,
        };
        unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &restore) };
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn get_rlimit_reads_current_value() {
        let (cur, max) = get_rlimit(libc::RLIMIT_NOFILE).unwrap();
        // RLIMIT_NOFILE should be > 0 on any system
        assert!(cur > 0);
        assert!(max > 0);
    }
}
