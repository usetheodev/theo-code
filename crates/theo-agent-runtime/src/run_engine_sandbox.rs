//! Sandbox spawning for done-gate `cargo` invocations.
//!
//! Extracted from `run_engine.rs` to isolate the unsafe `pre_exec`
//! block. Current hardening layers (cumulative, T1.1):
//!   1. Linux kernel rlimits — CPU / memory / file-size / NPROC caps
//!      via `setrlimit` in the child's `pre_exec` hook.
//!   2. Environment sanitization — secret-bearing env vars
//!      (OPENAI_API_KEY, AWS_*, GITHUB_TOKEN, ...) are stripped before
//!      exec so a malicious `build.rs` / proc-macro cannot exfiltrate
//!      them. Only a whitelist (PATH, HOME, USER, LANG, TERM, SHELL,
//!      TMPDIR, …) passes through.
//!
//! Full filesystem isolation via bwrap/landlock is follow-up work —
//! it requires tuning the allowed-write set so `cargo test` can still
//! write to `target/` and the shared cargo cache.

use std::path::Path;

use theo_domain::sandbox::ProcessPolicy;

/// Build the `ProcessPolicy` that governs the done-gate's spawn. The
/// env whitelist is the crate-default (PATH/HOME/USER/LANG/…) minus
/// the ALWAYS_STRIPPED prefixes (handled inside `sanitized_env`). The
/// rlimit fields come from `crate::constants` so the numbers are a
/// single source of truth shared with the rest of the runtime.
fn done_gate_policy() -> ProcessPolicy {
    ProcessPolicy {
        max_cpu_seconds: crate::constants::DONE_GATE_CPU_SECONDS,
        max_memory_bytes: crate::constants::DONE_GATE_MEM_BYTES,
        max_file_size_bytes: crate::constants::DONE_GATE_FSIZE_BYTES,
        max_processes: crate::constants::DONE_GATE_NPROC,
        // Use the crate-default allowlist so cargo can resolve PATH,
        // HOME, USER, etc. The ALWAYS_STRIPPED_ENV_PREFIXES filter
        // runs on top of this list inside `sanitized_env` — secret
        // tokens never pass through even if something else adds them.
        allowed_env_vars: ProcessPolicy::default().allowed_env_vars,
    }
}

/// Spawn `cargo <args>` under the done-gate sandbox. Applies kernel
/// rlimits + env-var sanitization (Linux) or env-var sanitization only
/// (other platforms).
pub(crate) async fn spawn_done_gate_cargo(
    project_dir: &Path,
    args: &[String],
) -> std::io::Result<std::process::Output> {
    let policy = done_gate_policy();

    let mut cmd = tokio::process::Command::new("cargo");
    cmd.args(args).current_dir(project_dir);

    // Strip the current environment down to the allowlist + remove
    // ALWAYS_STRIPPED entries. Runs on every platform.
    let allowed = theo_tooling::sandbox::env_sanitizer::sanitized_env(&policy);
    cmd.env_clear();
    for (key, value) in allowed {
        cmd.env(key, value);
    }

    #[cfg(target_os = "linux")]
    {
        // SAFETY: `apply_rlimits` only calls `setrlimit` which is
        // async-signal-safe. Runs in the child process after fork and
        // before exec, as required.
        let policy_for_exec = policy.clone();
        unsafe {
            cmd.pre_exec(move || {
                theo_tooling::sandbox::rlimits::apply_rlimits(&policy_for_exec)
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

    /// T1.1 AC regression — the done-gate policy MUST whitelist basic
    /// execution env (PATH/HOME/USER) AND strip secret-bearing vars.
    #[test]
    fn done_gate_policy_whitelists_exec_env() {
        let policy = done_gate_policy();
        for essential in ["PATH", "HOME", "USER"] {
            assert!(
                policy
                    .allowed_env_vars
                    .iter()
                    .any(|v| v == essential),
                "done_gate policy must allow {essential} — cargo cannot run without it"
            );
        }
    }

    /// T1.1 AC regression — secret-bearing env vars (OPENAI_API_KEY,
    /// AWS_*, GITHUB_TOKEN, …) MUST NOT pass through the sanitizer
    /// even if the allowlist accidentally includes them. Tests the
    /// `ALWAYS_STRIPPED_ENV_PREFIXES` override layer.
    #[test]
    fn secrets_are_stripped_even_if_allowlisted() {
        use theo_domain::sandbox::ProcessPolicy;
        // Craft a malicious policy that includes a bunch of secret
        // vars in its allowlist. The sanitizer must still strip them.
        let policy = ProcessPolicy {
            max_cpu_seconds: 10,
            max_memory_bytes: 1024,
            max_file_size_bytes: 1024,
            max_processes: 1,
            allowed_env_vars: vec![
                "PATH".to_string(),
                "OPENAI_API_KEY".to_string(),
                "AWS_SECRET_ACCESS_KEY".to_string(),
                "GITHUB_TOKEN".to_string(),
                "ANTHROPIC_API_KEY".to_string(),
                "CLAUDE_API_KEY".to_string(),
            ],
        };
        // Inject fake secret values into the process env for the
        // duration of the test.
        let saved: Vec<(String, Option<std::ffi::OsString>)> = [
            "OPENAI_API_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "GITHUB_TOKEN",
            "ANTHROPIC_API_KEY",
            "CLAUDE_API_KEY",
        ]
        .iter()
        .map(|k| (k.to_string(), std::env::var_os(k)))
        .collect();
        unsafe {
            for (k, _) in &saved {
                std::env::set_var(k, "SECRET-VALUE-DO-NOT-LEAK");
            }
        }

        let sanitized = theo_tooling::sandbox::env_sanitizer::sanitized_env(&policy);
        for secret in [
            "OPENAI_API_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "GITHUB_TOKEN",
            "ANTHROPIC_API_KEY",
            "CLAUDE_API_KEY",
        ] {
            assert!(
                !sanitized.iter().any(|(k, _)| k == secret),
                "secret var {secret} leaked through sanitizer despite being in allowlist"
            );
        }

        // Restore original env.
        unsafe {
            for (k, v) in saved {
                match v {
                    Some(val) => std::env::set_var(&k, val),
                    None => std::env::remove_var(&k),
                }
            }
        }
    }
}
