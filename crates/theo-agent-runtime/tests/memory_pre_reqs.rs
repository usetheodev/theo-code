//! RM-pre acceptance tests — plan: `outputs/agent-memory-plan.md` §2.
//!
//! Covers the acceptance criteria for RM-pre-1 (.gitignore), RM-pre-4 (ADR),
//! and RM-pre-5 (memory_enabled feature flag). RM-pre-2 (`MemoryError`) lives
//! as unit tests inside `theo-domain::memory::tests`. RM-pre-3 (fix
//! `unwrap()`) has no dedicated test but is covered by the existing retry
//! loop smoke tests.

use std::path::PathBuf;

use theo_agent_runtime::config::AgentConfig;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

// ── RM-pre-1 ──────────────────────────────────────────────────────────

#[test]
fn test_pre1_ac_1_gitignore_excludes_theo_memory() {
    let root = workspace_root();
    let gi = std::fs::read_to_string(root.join(".gitignore")).expect("read .gitignore");
    assert!(
        gi.contains(".theo/memory/"),
        ".gitignore must exclude personal memory; got:\n{gi}"
    );
}

#[test]
fn test_pre1_ac_2_gitignore_excludes_memory_wiki() {
    let gi = std::fs::read_to_string(workspace_root().join(".gitignore")).unwrap();
    assert!(
        gi.contains(".theo/wiki/memory/"),
        ".gitignore must exclude agent-owned wiki mount"
    );
}

#[test]
fn test_pre1_ac_3_gitignore_allows_code_wiki() {
    let gi = std::fs::read_to_string(workspace_root().join(".gitignore")).unwrap();
    // Either the directory is not ignored OR there is an explicit re-include.
    let has_reinclude = gi.contains("!.theo/wiki/code/");
    assert!(
        has_reinclude,
        "code wiki must remain tracked — expected `!.theo/wiki/code/` in .gitignore"
    );
}

#[test]
fn test_pre1_ac_4_gitignore_excludes_reflections_jsonl() {
    let gi = std::fs::read_to_string(workspace_root().join(".gitignore")).unwrap();
    assert!(
        gi.contains(".theo/reflections.jsonl"),
        ".gitignore must exclude reflections.jsonl"
    );
}

// ── RM-pre-4 ──────────────────────────────────────────────────────────

#[test]
fn test_pre4_ac_2_adr_008_exists_and_signed() {
    let adr = workspace_root().join("docs/adr/008-theo-infra-memory.md");
    let body = std::fs::read_to_string(&adr).expect("ADR-008 must be committed");
    assert!(body.contains("Status:"), "ADR must carry a status line");
    assert!(
        body.contains("Accepted") || body.contains("Proposed"),
        "ADR status must be declared"
    );
    assert!(
        body.contains("theo-infra-memory"),
        "ADR must name the crate"
    );
}

// ── RM-pre-5 ──────────────────────────────────────────────────────────

#[test]
fn test_pre5_ac_1_memory_enabled_default_false() {
    let cfg = AgentConfig::default();
    assert!(
        !cfg.memory.enabled,
        "memory_enabled must be off by default for backward-compat"
    );
}

#[test]
fn test_pre5_ac_2_memory_enabled_mutable_at_runtime() {
    let mut cfg = AgentConfig::default();
    assert!(!cfg.memory.enabled);
    cfg.memory.enabled = true;
    assert!(cfg.memory.enabled);
}
