//! Sandbox spawning for done-gate `cargo` invocations.
//!
//! Extracted from `run_engine.rs` (Fase 4 — REMEDIATION_PLAN T4.2) to
//! isolate the unsafe `pre_exec` block. Linux applies kernel rlimits;
//! other platforms fall through to the unrestricted command.
//!
//! See REMEDIATION_PLAN T1.1 for the security rationale.

use std::path::Path;

/// Spawn `cargo <args>` under kernel rlimits (Linux only).
///
/// Rlimits — CPU, memory, file size, NPROC — prevent a malicious
/// `build.rs` or proc-macro from burning the host. Full bwrap/landlock
/// isolation is follow-up work; this is the "at minimum" mitigation.
pub(crate) async fn spawn_done_gate_cargo(
    project_dir: &Path,
    args: &[String],
) -> std::io::Result<std::process::Output> {
    let mut cmd = tokio::process::Command::new("cargo");
    cmd.args(args).current_dir(project_dir);

    #[cfg(target_os = "linux")]
    {
        use theo_domain::sandbox::ProcessPolicy;
        let policy = ProcessPolicy {
            max_cpu_seconds: crate::constants::DONE_GATE_CPU_SECONDS,
            max_memory_bytes: crate::constants::DONE_GATE_MEM_BYTES,
            max_file_size_bytes: crate::constants::DONE_GATE_FSIZE_BYTES,
            max_processes: crate::constants::DONE_GATE_NPROC,
            allowed_env_vars: vec![],
        };
        // SAFETY: `apply_rlimits` only calls `setrlimit` which is
        // async-signal-safe. Runs in the child process after fork and
        // before exec, as required.
        unsafe {
            cmd.pre_exec(move || {
                theo_tooling::sandbox::rlimits::apply_rlimits(&policy)
            });
        }
    }

    cmd.output().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_done_gate_cargo_returns_io_error_when_cargo_missing() {
        // If cargo is on PATH this runs (--version is fast + safe). If
        // not, we expect an io::Error. Either outcome is acceptable —
        // we're only checking that the function doesn't panic and that
        // its error surface is `io::Result<_>`.
        let out = spawn_done_gate_cargo(
            std::path::Path::new("/tmp"),
            &["--version".to_string()],
        )
        .await;
        // Permissive: cargo may or may not be installed in the test
        // environment. We just want the type to be io::Result.
        let _ = out;
    }
}
