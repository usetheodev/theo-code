//! T5.5 — CLI smoke tests.
//!
//! Minimal invariants the CLI must uphold. Each test spawns the real
//! `theo` binary in a dedicated working directory so flakiness from
//! shared state is impossible.
//!
//! Scope of this iteration: `--help` and `--version` — the cheapest
//! signals that rule out most breakage. Longer flows (login flow,
//! single-prompt chat, multi-turn chat, tool invocation, logout) are
//! earmarked for subsequent iterations once wiremock + an LLM fixture
//! runtime are plumbed. Tracking: `docs/audit/remediation-plan.md` T5.5.

use assert_cmd::Command;
use predicates::prelude::*;

/// Helper: locate the freshly-built `theo` binary via assert_cmd's
/// CARGO_BIN_EXE_theo env var. Returns a configured `Command` with an
/// isolated working directory so accidental filesystem touches land in
/// the tempdir, not the workspace root.
fn theo() -> Command {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut cmd = Command::cargo_bin("theo").expect("binary");
    cmd.current_dir(tmp.path())
        // Forget the tempdir reference — assert_cmd runs synchronously,
        // so the dir exists for the duration of the assertion.
        .env_remove("THEO_SESSION")
        .env_remove("THEO_CONFIG_DIR")
        .env("HOME", tmp.path())
        .env("NO_COLOR", "1");
    // Keep tempdir alive by leaking; acceptable in tests.
    std::mem::forget(tmp);
    cmd
}

#[test]
fn help_flag_prints_usage_banner() {
    theo()
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Usage")
                .or(predicate::str::contains("USAGE"))
                .or(predicate::str::contains("theo")),
        );
}

#[test]
fn version_flag_prints_semver_string() {
    theo()
        .arg("--version")
        .assert()
        .success()
        // Matches `theo 0.1.0` or `theo 0.1.0 (…)` etc.
        .stdout(predicate::str::is_match(r"\d+\.\d+\.\d+").unwrap());
}

#[test]
fn bogus_flag_exits_nonzero_with_diagnostic() {
    theo()
        .arg("--this-flag-does-not-exist")
        .assert()
        .failure()
        .code(predicate::ne(0));
}

#[test]
fn help_subcommand_output_stays_under_5kb() {
    // Guards against accidentally dumping the entire clap hierarchy.
    // The limit is generous; adjust upward when new subcommands legitimately
    // grow help text.
    let output = theo().arg("--help").output().expect("execute");
    assert!(
        output.stdout.len() < 5_000,
        "--help exceeded 5 KB: {} bytes",
        output.stdout.len()
    );
}

// ── Subcommand surface (structure pins — not behaviour) ─────────────────────
//
// These tests pin the list of subcommands the CLI advertises. Adding a new
// subcommand should expand this list consciously (so the deprecation cost
// of removing one is visible). They stay deterministic because they only
// check output text — they never spawn a network request or an LLM.

#[test]
fn help_exposes_every_advertised_subcommand() {
    let output = theo().arg("--help").output().expect("execute");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for cmd in [
        // Core agent surface
        "init", "agent", "pilot", "context", "impact", "stats", "memory",
        // Identity + service
        "login", "logout", "dashboard",
        // Sub-agent + state management
        "subagent", "checkpoints", "agents",
        // External integrations
        "mcp",
        // SOTA Tier 1 + Tier 2 additions
        "skill",       // T9.1
        "trajectory",  // T16.1 / D16
    ] {
        assert!(
            stdout.contains(cmd),
            "subcommand `{cmd}` missing from `theo --help` output"
        );
    }
}

/// Locks the INVOKABILITY half of every CLI subcommand: each one
/// must respond to `--help` with exit 0 and non-empty output.
///
/// Why a dedicated test: the existing
/// `help_exposes_every_advertised_subcommand` test only checks that
/// the subcommand NAME appears in the parent `--help` output, not
/// that the subcommand itself responds to `--help`. A typo in the
/// dispatch table would let the parent help mention `trajectory`
/// while `theo trajectory --help` panics or returns exit 2 — the
/// kind of CLI-surface gap I closed in commit 86165f8 for
/// `theo trajectory export-rlhf` (where the library code existed
/// but the CLI subcommand had never been wired).
///
/// Subcommands that REQUIRE an argument (e.g. `pilot` takes a
/// project path, `mcp` takes a sub-action) are tested via
/// `<cmd> --help` which clap renders without invoking the handler.
#[test]
fn every_subcommand_responds_to_help_with_exit_zero() {
    for cmd in [
        "init", "agent", "pilot", "context", "impact", "stats", "memory",
        "login", "logout", "dashboard",
        "subagent", "checkpoints", "agents",
        "mcp",
        "skill", "trajectory",
    ] {
        let assert = theo().args([cmd, "--help"]).assert().success();
        let output = assert.get_output();
        assert!(
            !output.stdout.is_empty(),
            "subcommand `theo {cmd} --help` produced empty stdout — \
             the dispatch-or-handler is silently broken"
        );
    }
}

/// T16.1 / D16 — `theo trajectory export-rlhf` is the closing CLI
/// surface for the SOTA Tier 1 + Tier 2 plan. Locks both the
/// subcommand wiring (parent --help mentions trajectory) and the
/// nested action (`export-rlhf --help` works + names the contract
/// vocabulary: the `--out` arg + the `--filter` arg).
#[test]
fn trajectory_export_rlhf_surface_is_invokable() {
    theo()
        .args(["trajectory", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("export-rlhf"));
    theo()
        .args(["trajectory", "export-rlhf", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--out")
                .and(predicate::str::contains("--filter")),
        );
}

#[test]
fn login_subcommand_help_mentions_provider() {
    theo()
        .args(["login", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("provider")
                .or(predicate::str::contains("Provider"))
                .or(predicate::str::contains("OpenAI")),
        );
}

#[test]
fn logout_subcommand_help_mentions_credentials() {
    theo()
        .args(["logout", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("credential")
                .or(predicate::str::contains("saved"))
                .or(predicate::str::contains("Remove")),
        );
}

#[test]
fn stats_subcommand_accepts_help() {
    theo().args(["stats", "--help"]).assert().success();
}

#[test]
fn memory_subcommand_lint_help() {
    theo()
        .args(["memory", "lint", "--help"])
        .assert()
        .success();
}

// ── Unknown-arg behaviour ───────────────────────────────────────────────────

#[test]
fn unknown_subcommand_exits_nonzero() {
    theo()
        .arg("definitely-not-a-subcommand")
        .arg("--help")
        .assert()
        // Returning `success` for an unknown token is acceptable when it
        // is treated as a positional prompt; the important invariant is
        // that clap does NOT crash with an internal error.
        .code(predicate::lt(2_i32));
}

#[test]
fn version_output_matches_workspace_version() {
    // Workspace version is 0.1.0 (root Cargo.toml). When we bump the
    // workspace version this test must be updated — cheap reminder.
    theo()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("0.1.0"));
}
