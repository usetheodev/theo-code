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

// ---------------------------------------------------------------------------
// — CRUD operations + origin tracking.
// ---------------------------------------------------------------------------

/// Constants lifted from Hermes's skill_manager_tool.py:83-104.
pub const MAX_SKILL_NAME_LENGTH: usize = 64;
pub const MAX_SKILL_DESCRIPTION_LENGTH: usize = 1024;
pub const MAX_SKILL_CONTENT_CHARS: usize = 100_000;
pub const ALLOWED_SUPPORTING_SUBDIRS: &[&str] = &["references", "templates", "scripts", "assets"];

/// Error type for catalog write operations.
#[derive(Debug, thiserror::Error)]
pub enum SkillCatalogError {
    #[error("skill name is empty")]
    EmptyName,
    #[error("skill name exceeds {max} characters: {name}")]
    NameTooLong { name: String, max: usize },
    #[error(
        "skill name '{name}' must match [a-z0-9][a-z0-9._-]* (lowercase letters, digits, . _ -)"
    )]
    InvalidName { name: String },
    #[error("skill '{name}' not found")]
    NotFound { name: String },
    #[error("skill '{name}' already exists — use 'edit' or 'patch' instead")]
    AlreadyExists { name: String },
    #[error("skill description exceeds {max} characters")]
    DescriptionTooLong { max: usize },
    #[error("skill body exceeds {max} characters")]
    BodyTooLong { max: usize },
    #[error("supporting directory must be one of {allowed:?}, got '{got}'")]
    InvalidSubdir { got: String, allowed: &'static [&'static str] },
    #[error("patch target not found: {needle}")]
    PatchTargetMissing { needle: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Validate a proposed skill name against the Hermes-compatible regex.
/// We do not take `regex` as a dep just for this — the validation is
/// simple enough to unroll by hand.
fn validate_skill_name(name: &str) -> Result<(), SkillCatalogError> {
    if name.is_empty() {
        return Err(SkillCatalogError::EmptyName);
    }
    if name.len() > MAX_SKILL_NAME_LENGTH {
        return Err(SkillCatalogError::NameTooLong {
            name: name.to_string(),
            max: MAX_SKILL_NAME_LENGTH,
        });
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    let first_ok = first.is_ascii_lowercase() || first.is_ascii_digit();
    if !first_ok {
        return Err(SkillCatalogError::InvalidName {
            name: name.to_string(),
        });
    }
    for c in chars {
        let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-';
        if !ok {
            return Err(SkillCatalogError::InvalidName {
                name: name.to_string(),
            });
        }
    }
    Ok(())
}

/// Create a new skill under `<home>/skills/<name>/SKILL.md`.
///
/// `frontmatter` is passed as a borrowed map so callers can hand us the
/// exact keys they want serialized (description, category, origin,
/// etc.). The `origin` key is the only one we treat as semantically
/// meaningful — its value is persisted verbatim so downstream policy
/// checks can read it back. Matches Hermes `_create_skill`
/// (`skill_manager_tool.py:304-355`).
pub fn create_skill(
    home: &Path,
    name: &str,
    frontmatter: &BTreeMap<String, String>,
    body: &str,
) -> Result<PathBuf, SkillCatalogError> {
    validate_skill_name(name)?;
    if let Some(desc) = frontmatter.get("description")
        && desc.len() > MAX_SKILL_DESCRIPTION_LENGTH
    {
        return Err(SkillCatalogError::DescriptionTooLong {
            max: MAX_SKILL_DESCRIPTION_LENGTH,
        });
    }
    if body.len() > MAX_SKILL_CONTENT_CHARS {
        return Err(SkillCatalogError::BodyTooLong {
            max: MAX_SKILL_CONTENT_CHARS,
        });
    }
    let dir = home.join("skills").join(name);
    if dir.exists() {
        return Err(SkillCatalogError::AlreadyExists {
            name: name.to_string(),
        });
    }
    fs::create_dir_all(&dir)?;
    let mut raw = String::from("---\n");
    for (k, v) in frontmatter {
        raw.push_str(&format!("{k}: {v}\n"));
    }
    raw.push_str("---\n");
    raw.push_str(body);
    let path = dir.join("SKILL.md");
    fs::write(&path, raw)?;
    Ok(path)
}

/// Replace an existing SKILL.md content entirely. Keeps the directory
/// and any supporting files intact.
pub fn edit_skill(
    home: &Path,
    name: &str,
    frontmatter: &BTreeMap<String, String>,
    body: &str,
) -> Result<PathBuf, SkillCatalogError> {
    let dir = home.join("skills").join(name);
    let path = dir.join("SKILL.md");
    if !path.is_file() {
        return Err(SkillCatalogError::NotFound {
            name: name.to_string(),
        });
    }
    if body.len() > MAX_SKILL_CONTENT_CHARS {
        return Err(SkillCatalogError::BodyTooLong {
            max: MAX_SKILL_CONTENT_CHARS,
        });
    }
    let mut raw = String::from("---\n");
    for (k, v) in frontmatter {
        raw.push_str(&format!("{k}: {v}\n"));
    }
    raw.push_str("---\n");
    raw.push_str(body);
    fs::write(&path, raw)?;
    Ok(path)
}

/// Find-and-replace inside SKILL.md (or a supporting file). Matches
/// Hermes `_patch_skill` (`skill_manager_tool.py:397-470`).
pub fn patch_skill(
    home: &Path,
    name: &str,
    old_string: &str,
    new_string: &str,
    file_path: Option<&str>,
) -> Result<PathBuf, SkillCatalogError> {
    let dir = home.join("skills").join(name);
    let target = match file_path {
        Some(rel) => {
            let rel_path = PathBuf::from(rel);
            // Sanity check: only the allowed subdirs (and SKILL.md) may
            // be patched. This prevents a malicious agent from using
            // patch to escape into .. or absolute paths.
            if rel != "SKILL.md" {
                let first = rel_path
                    .components()
                    .next()
                    .and_then(|c| c.as_os_str().to_str())
                    .unwrap_or_default();
                if !ALLOWED_SUPPORTING_SUBDIRS.contains(&first) {
                    return Err(SkillCatalogError::InvalidSubdir {
                        got: first.to_string(),
                        allowed: ALLOWED_SUPPORTING_SUBDIRS,
                    });
                }
            }
            dir.join(rel_path)
        }
        None => dir.join("SKILL.md"),
    };
    if !target.is_file() {
        return Err(SkillCatalogError::NotFound {
            name: name.to_string(),
        });
    }
    let raw = fs::read_to_string(&target)?;
    if !raw.contains(old_string) {
        return Err(SkillCatalogError::PatchTargetMissing {
            needle: old_string.to_string(),
        });
    }
    let patched = raw.replacen(old_string, new_string, 1);
    fs::write(&target, patched)?;
    Ok(target)
}

/// Delete an entire skill directory. Callers are responsible for
/// origin checks (only agent/user-created skills should be deletable).
pub fn delete_skill(home: &Path, name: &str) -> Result<(), SkillCatalogError> {
    validate_skill_name(name)?;
    let dir = home.join("skills").join(name);
    if !dir.is_dir() {
        return Err(SkillCatalogError::NotFound {
            name: name.to_string(),
        });
    }
    fs::remove_dir_all(&dir)?;
    Ok(())
}

/// Read the `origin` frontmatter value for a skill. Returns `None`
/// when the skill is missing or the field is absent.
pub fn skill_origin(home: &Path, name: &str) -> Option<String> {
    let path = home.join("skills").join(name).join("SKILL.md");
    let raw = fs::read_to_string(&path).ok()?;
    parse_frontmatter(&raw).get("origin").cloned()
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

    // ── CRUD operations ────────────────────────────────────

    fn fm(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_validate_skill_name_accepts_kebab_case() {
        assert!(validate_skill_name("deploy-staging").is_ok());
        assert!(validate_skill_name("0-prefix-digit").is_ok());
        assert!(validate_skill_name("a.b.c").is_ok());
        assert!(validate_skill_name("under_score").is_ok());
    }

    #[test]
    fn test_validate_skill_name_rejects_uppercase() {
        assert!(matches!(
            validate_skill_name("Deploy"),
            Err(SkillCatalogError::InvalidName { .. })
        ));
    }

    #[test]
    fn test_validate_skill_name_rejects_empty() {
        assert!(matches!(
            validate_skill_name(""),
            Err(SkillCatalogError::EmptyName)
        ));
    }

    #[test]
    fn test_create_skill_writes_frontmatter_and_body() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "deploy-staging",
            &fm(&[
                ("description", "Deploy to staging"),
                ("category", "deploy"),
                ("origin", "agent"),
            ]),
            "## Steps\n1. Build\n2. Push",
        )
        .expect("create");
        let v = view_skill(tmp.path(), "deploy-staging").expect("exists");
        assert_eq!(v.metadata.category, "deploy");
        assert!(v.body.contains("Steps"));
        assert_eq!(
            skill_origin(tmp.path(), "deploy-staging").as_deref(),
            Some("agent")
        );
    }

    #[test]
    fn test_create_skill_fails_when_already_exists() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "dup",
            &fm(&[("description", "x"), ("category", "y")]),
            "body",
        )
        .unwrap();
        let err = create_skill(
            tmp.path(),
            "dup",
            &fm(&[("description", "x2")]),
            "body2",
        )
        .unwrap_err();
        assert!(matches!(err, SkillCatalogError::AlreadyExists { .. }));
    }

    #[test]
    fn test_edit_skill_rewrites_body_without_touching_supporting_files() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "edit-me",
            &fm(&[("description", "d"), ("category", "c"), ("origin", "user")]),
            "original",
        )
        .unwrap();
        let skill_dir = tmp.path().join("skills").join("edit-me");
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(skill_dir.join("references").join("a.md"), "kept").unwrap();

        edit_skill(
            tmp.path(),
            "edit-me",
            &fm(&[("description", "d"), ("category", "c"), ("origin", "user")]),
            "rewritten",
        )
        .unwrap();

        let v = view_skill(tmp.path(), "edit-me").unwrap();
        assert!(v.body.contains("rewritten"));
        assert!(skill_dir.join("references").join("a.md").exists());
    }

    #[test]
    fn test_edit_skill_fails_when_missing() {
        let tmp = TempDir::new().unwrap();
        let err = edit_skill(
            tmp.path(),
            "ghost",
            &fm(&[("description", "d")]),
            "body",
        )
        .unwrap_err();
        assert!(matches!(err, SkillCatalogError::NotFound { .. }));
    }

    #[test]
    fn test_patch_skill_replaces_first_occurrence() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "patch-me",
            &fm(&[("description", "d"), ("category", "c")]),
            "old text appears twice: old text",
        )
        .unwrap();
        patch_skill(tmp.path(), "patch-me", "old text", "new text", None).unwrap();
        let v = view_skill(tmp.path(), "patch-me").unwrap();
        // replacen(1) replaces only the first occurrence.
        assert!(
            v.body.contains("new text appears twice"),
            "first occurrence replaced; body: {:?}",
            v.body
        );
        assert!(
            v.body.contains("old text"),
            "second occurrence preserved; body: {:?}",
            v.body
        );
    }

    #[test]
    fn test_patch_skill_rejects_escape_to_parent_dir() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "safe",
            &fm(&[("description", "d"), ("category", "c")]),
            "body",
        )
        .unwrap();
        let err = patch_skill(
            tmp.path(),
            "safe",
            "body",
            "x",
            Some("../../etc/passwd"),
        )
        .unwrap_err();
        assert!(matches!(err, SkillCatalogError::InvalidSubdir { .. }));
    }

    #[test]
    fn test_patch_skill_fails_when_needle_missing() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "no-needle",
            &fm(&[("description", "d"), ("category", "c")]),
            "body",
        )
        .unwrap();
        let err = patch_skill(tmp.path(), "no-needle", "missing", "x", None).unwrap_err();
        assert!(matches!(err, SkillCatalogError::PatchTargetMissing { .. }));
    }

    #[test]
    fn test_delete_skill_removes_directory() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "to-delete",
            &fm(&[("description", "d")]),
            "body",
        )
        .unwrap();
        delete_skill(tmp.path(), "to-delete").unwrap();
        assert!(view_skill(tmp.path(), "to-delete").is_none());
    }

    #[test]
    fn test_skill_origin_reads_frontmatter_field() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "with-origin",
            &fm(&[
                ("description", "d"),
                ("category", "c"),
                ("origin", "community"),
            ]),
            "body",
        )
        .unwrap();
        assert_eq!(
            skill_origin(tmp.path(), "with-origin").as_deref(),
            Some("community")
        );
    }

    #[test]
    fn test_skill_origin_returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "no-origin",
            &fm(&[("description", "d"), ("category", "c")]),
            "body",
        )
        .unwrap();
        assert!(skill_origin(tmp.path(), "no-origin").is_none());
    }

    #[test]
    fn test_create_skill_rejects_oversize_body() {
        let tmp = TempDir::new().unwrap();
        let big = "x".repeat(MAX_SKILL_CONTENT_CHARS + 1);
        let err = create_skill(
            tmp.path(),
            "huge",
            &fm(&[("description", "d")]),
            &big,
        )
        .unwrap_err();
        assert!(matches!(err, SkillCatalogError::BodyTooLong { .. }));
    }
}
