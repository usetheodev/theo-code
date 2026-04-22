//! Progressive-disclosure skill catalog (two-tier).
//!
//! Separate from the in-context `skill::` module — this catalog implements
//! the hermes pattern where skills are external `SKILL.md` files under
//! `$THEO_HOME/skills/<name>/`. The agent first calls `list_skills` (tier 1
//! = metadata only, ~30 tokens per skill) and then `view_skill(name)`
//! (tier 2 = full body + `linked_files`) only for the skills relevant to
//! the current turn.
//!
//! Reference: `referencias/hermes-agent/tools/skills_tool.py:647-1000`
//!
//! Frontmatter is a minimal `key: value` parser — avoiding a new dep
//! (serde_yaml) per the workspace convention.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Metadata returned by `list_skills` — intentionally minimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub category: String,
}

/// Full content returned by `view_skill`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillView {
    pub metadata: SkillMetadata,
    pub body: String,
    /// Relative paths of files under `references/`, `templates/`, `assets/`,
    /// `scripts/` subdirectories of the skill folder. Content is NOT inlined —
    /// the agent loads each file lazily with a follow-up tool call.
    pub linked_files: Vec<PathBuf>,
}

/// Subdirectories scanned for `linked_files`.
const LINKED_SUBDIRS: &[&str] = &["references", "templates", "assets", "scripts"];

/// List all skills available under `home/skills/`, sorted by name.
///
/// Each skill is a directory containing a `SKILL.md` file. Directories without
/// that file are silently skipped. I/O errors on individual skills are skipped
/// (the catalog is best-effort). Returns an empty `Vec` if `home` doesn't exist.
pub fn list_skills(home: &Path) -> Vec<SkillMetadata> {
    let skills_root = home.join("skills");
    let Ok(entries) = fs::read_dir(&skills_root) else {
        return Vec::new();
    };
    let mut out: Vec<SkillMetadata> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&skill_md) else {
            continue;
        };
        let fm = parse_frontmatter(&raw);
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let description = fm.get("description").cloned().unwrap_or_default();
        let category = fm.get("category").cloned().unwrap_or_else(|| "general".into());
        out.push(SkillMetadata {
            name,
            description,
            category,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// View a single skill by name — returns full body + linked_files paths.
/// Returns `None` when the skill doesn't exist or cannot be read.
pub fn view_skill(home: &Path, name: &str) -> Option<SkillView> {
    let skill_dir = home.join("skills").join(name);
    let skill_md = skill_dir.join("SKILL.md");
    let raw = fs::read_to_string(&skill_md).ok()?;
    let fm = parse_frontmatter(&raw);
    let body = strip_frontmatter(&raw).to_string();
    let description = fm.get("description").cloned().unwrap_or_default();
    let category = fm.get("category").cloned().unwrap_or_else(|| "general".into());
    let linked_files = collect_linked_files(&skill_dir);
    Some(SkillView {
        metadata: SkillMetadata {
            name: name.to_string(),
            description,
            category,
        },
        body,
        linked_files,
    })
}

/// Minimal YAML-ish frontmatter parser. Reads `key: value` pairs between
/// the first two `---` lines. Unknown keys are preserved; blank lines and
/// comments (`#`) are ignored.
fn parse_frontmatter(raw: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return out;
    }
    let without_open = &trimmed[3..];
    let Some(end) = without_open.find("\n---") else {
        return out;
    };
    let block = &without_open[..end];
    for line in block.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

/// Return the slice of `raw` after the frontmatter block (if any).
fn strip_frontmatter(raw: &str) -> &str {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return raw;
    }
    let without_open = &trimmed[3..];
    let Some(end) = without_open.find("\n---") else {
        return raw;
    };
    let body_start = end + 4;
    &without_open[body_start..]
}

/// Collect file paths (relative to skill_dir) from the known subdirectories.
fn collect_linked_files(skill_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for sub in LINKED_SUBDIRS {
        let sub_dir = skill_dir.join(sub);
        let Ok(entries) = fs::read_dir(&sub_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Ok(rel) = path.strip_prefix(skill_dir) {
                    out.push(rel.to_path_buf());
                }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill(home: &Path, name: &str, frontmatter: &str, body: &str) -> PathBuf {
        let dir = home.join("skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        let md = dir.join("SKILL.md");
        fs::write(&md, format!("---\n{frontmatter}\n---\n{body}")).unwrap();
        dir
    }

    #[test]
    fn list_empty_when_no_skills_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(list_skills(tmp.path()).is_empty());
    }

    #[test]
    fn list_returns_only_metadata() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "refactor-rust",
            "description: Refactor Rust code\ncategory: rust",
            "lots of body text here",
        );
        let skills = list_skills(tmp.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "refactor-rust");
        assert_eq!(skills[0].description, "Refactor Rust code");
        assert_eq!(skills[0].category, "rust");
    }

    #[test]
    fn list_sorts_by_name() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "zebra", "description: z\ncategory: x", "");
        write_skill(tmp.path(), "alpha", "description: a\ncategory: x", "");
        let skills = list_skills(tmp.path());
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[1].name, "zebra");
    }

    #[test]
    fn list_skips_dirs_without_skill_md() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("skills").join("incomplete")).unwrap();
        assert!(list_skills(tmp.path()).is_empty());
    }

    #[test]
    fn view_returns_body_and_metadata() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "s1",
            "description: test\ncategory: general",
            "## Instructions\nUse X.",
        );
        let v = view_skill(tmp.path(), "s1").expect("should exist");
        assert_eq!(v.metadata.description, "test");
        assert!(v.body.contains("Use X"));
        assert!(v.body.contains("Instructions"));
    }

    #[test]
    fn view_returns_linked_files_paths_only() {
        let tmp = TempDir::new().unwrap();
        let dir = write_skill(tmp.path(), "s2", "description: d\ncategory: c", "body");
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::write(dir.join("references").join("cheatsheet.md"), "content").unwrap();
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("scripts").join("setup.sh"), "#!/bin/sh").unwrap();

        let v = view_skill(tmp.path(), "s2").unwrap();
        assert_eq!(v.linked_files.len(), 2);
        // Content should NOT be inlined — only paths.
        assert!(!v.body.contains("#!/bin/sh"));
    }

    #[test]
    fn view_returns_none_for_missing_skill() {
        let tmp = TempDir::new().unwrap();
        assert!(view_skill(tmp.path(), "ghost").is_none());
    }

    #[test]
    fn frontmatter_with_unknown_keys_preserved_silently() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "s3",
            "description: d\ncategory: c\nauthor: paulo\nversion: 1.0",
            "body",
        );
        let skills = list_skills(tmp.path());
        assert_eq!(skills[0].description, "d");
    }

    #[test]
    fn missing_frontmatter_defaults_category_to_general() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("skills").join("noFM");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "body only, no frontmatter").unwrap();
        let skills = list_skills(tmp.path());
        assert_eq!(skills[0].category, "general");
    }
}
