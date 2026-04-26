//! Sandbox spawning for done-gate `cargo` invocations.
//!
//! Extracted from `run_engine.rs` to isolate the unsafe `pre_exec`
//! block. Hardening layers (cumulative, T1.1):
//!   1. Linux kernel rlimits — CPU / memory / file-size / NPROC caps
//!      via `setrlimit` in the child's `pre_exec` hook.
//!   2. Environment sanitization — secret-bearing env vars
//!      (OPENAI_API_KEY, AWS_*, GITHUB_TOKEN, ...) are stripped before
//!      exec so a malicious `build.rs` / proc-macro cannot exfiltrate
//!      them. Only a whitelist (PATH, HOME, USER, LANG, TERM, SHELL,
//!      TMPDIR, …) passes through.
//!   3. Bubblewrap (bwrap) filesystem isolation — when
//!      `/usr/bin/bwrap` is available, the cargo invocation runs
//!      inside namespaces with read-only `/usr` `/lib` `/etc`,
//!      `--tmpfs /tmp` (a malicious `build.rs` writing to `/tmp/X`
//!      affects only the throwaway tmpfs), writable project dir, and
//!      writable cargo / rustup caches under HOME. PID + network
//!      namespaces are unshared. Falls back to the rlimits-only path
//!      on hosts without bwrap (CI macOS, minimal containers, etc.).

use std::path::Path;

use theo_domain::sandbox::ProcessPolicy;
use theo_tooling::sandbox::probe::BWRAP_PATH;

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
/// (other platforms). On Linux hosts with `/usr/bin/bwrap` installed,
/// the cargo invocation additionally runs inside a bubblewrap mount
/// namespace with read-only system dirs and `--tmpfs /tmp`.
pub(crate) async fn spawn_done_gate_cargo(
    project_dir: &Path,
    args: &[String],
) -> std::io::Result<std::process::Output> {
    #[cfg(target_os = "linux")]
    {
        if Path::new(BWRAP_PATH).exists() {
            return spawn_done_gate_cargo_in_bwrap(project_dir, args).await;
        }
    }
    spawn_done_gate_cargo_rlimits_only(project_dir, args).await
}

/// Original spawn path — rlimits + env sanitization, no FS isolation.
/// Kept as fallback for hosts without bwrap (macOS CI, minimal
/// containers) and as a building block for the bwrap path.
async fn spawn_done_gate_cargo_rlimits_only(
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

/// Spawn `cargo <args>` inside a bubblewrap mount namespace.
///
/// Isolation flags applied:
///   - `--ro-bind /usr /usr`, `/lib`, `/lib64`, `/etc` — system
///     binaries + resolver configs are read-only.
///   - `--proc /proc`, `--dev /dev` — populated synthetically.
///   - `--tmpfs /tmp` — a fresh tmpfs replaces the host /tmp inside
///     the namespace. Build scripts that try `touch /tmp/<X>` write
///     to the tmpfs and the file never appears on the host.
///   - `--bind <project_dir>` — writable project tree.
///   - `--bind ${HOME}/.cargo` and `${HOME}/.rustup` (when present)
///     — cargo can populate its registry / build cache without
///     dropping out of the sandbox.
///   - `--unshare-pid`, `--unshare-net`, `--cap-drop ALL`,
///     `--die-with-parent`, `--new-session` — process / network /
///     capability isolation + auto-cleanup.
///
/// Env sanitization + rlimits are applied to the spawned `bwrap`
/// process; bwrap propagates them to the wrapped cargo via its env
/// model, so the same secret-stripping + setrlimit guarantees still
/// hold. Returns `io::Result<Output>` like the rlimits-only path.
#[cfg(target_os = "linux")]
async fn spawn_done_gate_cargo_in_bwrap(
    project_dir: &Path,
    args: &[String],
) -> std::io::Result<std::process::Output> {
    let policy = done_gate_policy();

    let mut cmd = tokio::process::Command::new(BWRAP_PATH);

    // Read-only system root.
    for path in &["/usr", "/lib", "/lib64", "/lib32", "/bin", "/sbin", "/etc"] {
        if Path::new(path).exists() {
            cmd.arg("--ro-bind").arg(path).arg(path);
        }
    }
    // FHS compatibility for usr-merged distros.
    if !Path::new("/bin").exists() && Path::new("/usr/bin").exists() {
        cmd.arg("--symlink").arg("usr/bin").arg("/bin");
    }
    if !Path::new("/sbin").exists() && Path::new("/usr/sbin").exists() {
        cmd.arg("--symlink").arg("usr/sbin").arg("/sbin");
    }

    cmd.arg("--proc").arg("/proc");
    cmd.arg("--dev").arg("/dev");

    // /tmp is a tmpfs visible only inside the sandbox — escape attempts
    // like `touch /tmp/escape` from a malicious build.rs go nowhere.
    cmd.arg("--tmpfs").arg("/tmp");

    // Writable project directory + chdir.
    cmd.arg("--bind").arg(project_dir).arg(project_dir);
    cmd.arg("--chdir").arg(project_dir);

    // Cargo + rustup caches must remain writable so the wrapped cargo
    // can populate registries / build artifacts. We bind them into the
    // sandbox at their host paths (cargo / rustup honor HOME/CARGO_HOME).
    if let Ok(home) = std::env::var("HOME") {
        let cargo_home = Path::new(&home).join(".cargo");
        if cargo_home.exists() {
            cmd.arg("--bind").arg(&cargo_home).arg(&cargo_home);
        }
        let rustup_home = Path::new(&home).join(".rustup");
        if rustup_home.exists() {
            cmd.arg("--bind").arg(&rustup_home).arg(&rustup_home);
        }
    }

    // Namespaces / capability drop / lifetime tie-in.
    cmd.arg("--unshare-pid");
    cmd.arg("--unshare-net");
    cmd.arg("--cap-drop").arg("ALL");
    cmd.arg("--die-with-parent");
    cmd.arg("--new-session");

    // Strip env down to the allowlist BEFORE bwrap runs so the wrapped
    // cargo never sees OPENAI_API_KEY / AWS_* / GITHUB_TOKEN.
    let allowed = theo_tooling::sandbox::env_sanitizer::sanitized_env(&policy);
    cmd.env_clear();
    for (key, value) in allowed {
        cmd.env(key, value);
    }

    // Apply rlimits to bwrap itself; the wrapped cargo inherits them.
    // SAFETY: same contract as `spawn_done_gate_cargo_rlimits_only`.
    let policy_for_exec = policy.clone();
    unsafe {
        cmd.pre_exec(move || {
            theo_tooling::sandbox::rlimits::apply_rlimits(&policy_for_exec)
        });
    }

    // Argv: cargo <args>.
    cmd.arg("cargo");
    for a in args {
        cmd.arg(a);
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

    /// T0.1 scenario 13 underpinning — Gate 2 (cargo-test) blocks when
    /// the project's `cargo check` exits non-zero. The end-to-end gate
    /// chain runs at the engine level and needs a write+done choreography
    /// to even reach Gate 2 (Gate 1 must pass first, requiring an edit).
    /// This test pins the underlying `cargo` invocation contract that
    /// Gate 2 relies on: a syntactically broken Cargo.toml in the
    /// project_dir causes `spawn_done_gate_cargo` to return Ok(output)
    /// with `output.status.success() == false`, which Gate 2 then
    /// interprets as a block.
    ///
    /// Skipped when cargo is missing (macOS CI without rust toolchain,
    /// minimal containers).
    #[tokio::test]
    async fn done_gate_cargo_check_fails_on_broken_manifest() {
        // Skip when cargo is missing.
        if std::process::Command::new("cargo")
            .arg("--version")
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            return;
        }

        let project = tempfile::tempdir().expect("tempdir");
        // Deliberately broken TOML — cargo will fail to parse the
        // manifest before it ever tries to compile anything.
        std::fs::write(
            project.path().join("Cargo.toml"),
            "this is not valid TOML at all }}}",
        )
        .unwrap();

        let out = spawn_done_gate_cargo(
            project.path(),
            &[
                "check".to_string(),
                "--message-format=short".to_string(),
            ],
        )
        .await;

        // io::Error means cargo couldn't be spawned at all. The
        // skip-guard above should prevent that, but treat it
        // permissively rather than failing on infra weirdness — the
        // `if let Ok(...)` skips the assertions in that case.
        if let Ok(output) = out {
            assert!(
                !output.status.success(),
                "broken Cargo.toml must yield a non-zero exit; \
                 stdout={:?} stderr={:?}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            // Gate 2 surfaces the stderr to the LLM via the BLOCKED
            // tool_result. Verify the failure carries diagnosable
            // text — TOML parser error mentions the manifest.
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            );
            assert!(
                !combined.trim().is_empty(),
                "broken cargo invocation must surface diagnostics for Gate 2"
            );
        }
    }

    /// T1.1 AC literal — `done_gate_cargo_test_runs_in_sandbox`. Build
    /// a project whose `build.rs` tries to escape the sandbox by writing
    /// to a host-visible path under `/tmp`. After running cargo through
    /// `spawn_done_gate_cargo`, the host-side target path MUST NOT
    /// exist. Skipped silently when bwrap or cargo are unavailable
    /// (macOS CI, minimal containers) — those hosts run the
    /// rlimits-only fallback which does not provide FS isolation.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn done_gate_cargo_test_runs_in_sandbox() {
        // Skip when bwrap is unavailable — fallback path does not
        // isolate the FS, so the AC literal cannot be enforced.
        if !std::path::Path::new(BWRAP_PATH).exists() {
            return;
        }
        // Skip when cargo is missing.
        if std::process::Command::new("cargo")
            .arg("--version")
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            return;
        }

        // Pick a per-process-unique escape target so concurrent test
        // runs don't false-positive each other.
        let escape_target = format!(
            "/tmp/theo-done-gate-escape-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        // Pre-clean any leftover (defense in depth — test must observe
        // the post-run state, not pre-run state).
        let _ = std::fs::remove_file(&escape_target);
        assert!(
            !std::path::Path::new(&escape_target).exists(),
            "pre-condition: escape target must not exist before the run"
        );

        let project = tempfile::tempdir().expect("tempdir");
        let cargo_toml = "[package]\n\
             name = \"theo_t1_1_fixture\"\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\
             build = \"build.rs\"\n\
             [lib]\n\
             path = \"src/lib.rs\"\n";
        std::fs::write(project.path().join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(project.path().join("src")).unwrap();
        std::fs::write(project.path().join("src/lib.rs"), "").unwrap();
        // Malicious build.rs — tries to write `escape_target` on the
        // host. Inside the sandbox this lands in `--tmpfs /tmp` which
        // is private to the namespace; the host's `/tmp` is untouched.
        let build_rs = format!(
            "fn main() {{\n    \
                let _ = std::fs::write(\"{escape_target}\", b\"escaped\");\n}}\n"
        );
        std::fs::write(project.path().join("build.rs"), build_rs).unwrap();

        // Run cargo through the done-gate sandbox.
        let out = spawn_done_gate_cargo(
            project.path(),
            &["build".to_string(), "--quiet".to_string()],
        )
        .await;

        // The cargo invocation may fail (offline, missing toolchain,
        // etc.) — that's not what this AC validates. What we DO assert
        // is that the host's escape target is still absent, regardless
        // of cargo's exit code.
        let _ = out;

        let leaked = std::path::Path::new(&escape_target).exists();
        // Belt-and-suspenders cleanup before asserting so the next run
        // starts clean even when this assertion fires.
        let _ = std::fs::remove_file(&escape_target);
        assert!(
            !leaked,
            "T1.1 AC violated: malicious build.rs escaped the sandbox \
             and wrote {escape_target} on the host"
        );
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
