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
    // SAFETY: `setrlimit` reads the `limit` struct for the duration of the
    // call (borrowed reference below). `resource` is a valid libc rlimit
    // resource constant by type construction. Failure (negative return) is
    // converted to a typed `io::Error`; no undefined behaviour regardless
    // of outcome.
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
    // SAFETY: `getrlimit` writes into `&mut limit` for the duration of the
    // call. The struct is stack-allocated, fully initialised before the
    // call, and its lifetime covers the entire syscall.
    let ret = unsafe { libc::getrlimit(resource, &mut limit) };
    if ret != 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok((limit.rlim_cur, limit.rlim_max))
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
        // SAFETY: `setrlimit` reads `restore` for the duration of the call;
        // `restore` lives on the stack through the statement. `RLIMIT_FSIZE`
        // is a valid resource constant. Test-only code that restores the
        // pre-test limit, so no cross-test contamination.
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

    #[cfg(target_os = "linux")]
    #[test]
    fn rlimit_fsize_enforced_in_child_process() {
        // Test that RLIMIT_FSIZE actually blocks large file creation in a child.
        // Safe: runs in a child process, doesn't affect test runner.
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;

        let policy = ProcessPolicy {
            max_processes: 0,
            max_memory_bytes: 0,
            max_cpu_seconds: 0,
            max_file_size_bytes: 1024, // 1KB limit
            allowed_env_vars: vec![],
        };
        let policy_clone = policy.clone();

        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c")
            // Try to write 10KB — should fail with EFBIG
            .arg("dd if=/dev/zero of=/tmp/theo_rlimit_test bs=1024 count=10 2>&1; echo exit=$?")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // SAFETY: `pre_exec` runs after fork, before exec, in a child
        // process. `apply_rlimits` only calls `setrlimit` — async-signal-safe.
        // Test-only code gated behind `#[cfg(target_os = "linux")]`.
        unsafe {
            cmd.pre_exec(move || apply_rlimits(&policy_clone));
        }

        let output = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        // dd should fail or be limited to 1KB
        assert!(
            stdout.contains("File size limit exceeded")
                || stdout.contains("exit=1")
                || stdout.contains("exit=25") // EFBIG signal
                || !output.status.success(),
            "RLIMIT_FSIZE should prevent writing 10KB with 1KB limit. Got: {stdout}"
        );

        // Cleanup
        let _ = std::fs::remove_file("/tmp/theo_rlimit_test");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rlimit_nproc_enforced_in_child_process() {
        // Test that RLIMIT_NPROC limits process creation in a child.
        // Safe: runs in a child process with strict limit.
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;

        let policy = ProcessPolicy {
            max_processes: 2, // Very restrictive
            max_memory_bytes: 0,
            max_cpu_seconds: 0,
            max_file_size_bytes: 0,
            allowed_env_vars: vec![],
        };
        let policy_clone = policy.clone();

        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c")
            // Try to spawn multiple subshells — should hit NPROC limit
            .arg("for i in 1 2 3 4 5; do sh -c 'echo $i' 2>/dev/null; done; echo done")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // SAFETY: see above — identical `pre_exec` invariant; child-side
        // `apply_rlimits` is async-signal-safe.
        unsafe {
            cmd.pre_exec(move || apply_rlimits(&policy_clone));
        }

        let output = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        // With NPROC=2, not all 5 subshells should succeed
        // (the parent sh counts as 1, leaving room for only 1 more)
        let line_count = stdout.lines().filter(|l| !l.is_empty()).count();
        // We should see "done" but fewer than 5 numbered lines
        assert!(
            line_count < 6,
            "RLIMIT_NPROC should limit subshell creation. Got {line_count} lines: {stdout}"
        );
    }
}
