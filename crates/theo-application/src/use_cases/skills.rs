//! T9.1 — Skill catalog use cases.
//!
//! Public façade over `theo_agent_runtime::skill_catalog`. Apps (theo-cli)
//! consume this module to enumerate user-installed skills and view their
//! contents. Install/remove/edit are explicit follow-ups (install requires
//! either a network registry or local file copy with checksum
//! verification — both out of scope for this use case which is pure read).
//!
//! Why a use case instead of direct re-export: it gives us a stable API
//! surface for the CLI even if the underlying `skill_catalog` module is
//! refactored, and centralises the resolution of `$THEO_HOME` so callers
//! don't reach for env vars themselves.
//!
//! See `docs/plans/sota-tier1-tier2-plan.md` §T9.1 + ADR D10.

use std::path::{Path, PathBuf};

use theo_agent_runtime::skill_catalog::{self, SkillMetadata, SkillView};

/// Resolve the theo home directory used to discover skills.
/// Order: `$THEO_HOME` env var → `~/.theo` (user_paths::theo_config_dir).
/// Returns `None` when neither is set / resolvable.
pub fn theo_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("THEO_HOME") {
        return Some(PathBuf::from(home));
    }
    theo_domain::user_paths::theo_config_dir()
}

/// List metadata for every skill under `home/skills/`.
pub fn list(home: &Path) -> Vec<SkillMetadata> {
    skill_catalog::list_skills(home)
}

/// View one skill's full body + linked files. Returns `None` when the
/// skill is missing.
pub fn view(home: &Path, name: &str) -> Option<SkillView> {
    skill_catalog::view_skill(home, name)
}

/// One-shot helper for the CLI: list skills using the resolved theo home.
/// Returns an empty `Vec` when no theo home is configured (instead of
/// erroring — listing skills should never fail loudly).
pub fn list_default() -> Vec<SkillMetadata> {
    match theo_home() {
        Some(h) => list(&h),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_skill(home: &Path, name: &str, frontmatter: &str, body: &str) {
        let dir = home.join("skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        let md = dir.join("SKILL.md");
        fs::write(md, format!("---\n{frontmatter}\n---\n{body}")).unwrap();
    }

    #[test]
    fn t91_list_returns_skills_under_home() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "fmt-rust", "description: Format Rust\ncategory: rust", "");
        let skills = list(tmp.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "fmt-rust");
    }

    #[test]
    fn t91_list_empty_when_no_skills_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(list(tmp.path()).is_empty());
    }

    #[test]
    fn t91_view_returns_skill_when_present() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "x", "description: d\ncategory: c", "body content");
        let v = view(tmp.path(), "x").expect("should be present");
        assert_eq!(v.metadata.name, "x");
        assert!(v.body.contains("body content"));
    }

    #[test]
    fn t91_view_returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(view(tmp.path(), "ghost").is_none());
    }

    #[test]
    fn t91_theo_home_respects_env_var() {
        // Save / restore the env var so this test is independent.
        let prev = std::env::var_os("THEO_HOME");
        let tmp = TempDir::new().unwrap();
        // SAFETY: skill catalog test sets a process-scoped env var that only this test reads; the test runner serializes by file, so no concurrent access exists.
        unsafe {
            std::env::set_var("THEO_HOME", tmp.path());
        }
        let home = theo_home().unwrap();
        assert_eq!(home, tmp.path());

        // Cleanup.
        // SAFETY: skill catalog test sets a process-scoped env var that only this test reads; the test runner serializes by file, so no concurrent access exists.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("THEO_HOME", v),
                None => std::env::remove_var("THEO_HOME"),
            }
        }
    }

    #[test]
    fn t91_list_default_is_empty_for_unconfigured_home() {
        // Snapshot env state to keep test deterministic.
        let theo_home_prev = std::env::var_os("THEO_HOME");
        let home_prev = std::env::var_os("HOME");
        // SAFETY: skill catalog test sets a process-scoped env var that only this test reads; the test runner serializes by file, so no concurrent access exists.
        unsafe {
            std::env::remove_var("THEO_HOME");
            // Point HOME to a temp dir without skills so list_default
            // resolves home but finds no skills.
            let tmp = TempDir::new().unwrap();
            std::env::set_var("HOME", tmp.path());
        }
        let skills = list_default();
        assert!(skills.is_empty());

        // Restore.
        // SAFETY: skill catalog test sets a process-scoped env var that only this test reads; the test runner serializes by file, so no concurrent access exists.
        unsafe {
            match theo_home_prev {
                Some(v) => std::env::set_var("THEO_HOME", v),
                None => std::env::remove_var("THEO_HOME"),
            }
            match home_prev {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }
}
