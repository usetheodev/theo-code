//! User-approval manifest for project-level agents (S3 / G1).
//!
//! Project agents from `.theo/agents/` can carry arbitrary system prompts and
//! must be approved by the user before being loaded. The manifest is
//! `.theo/.agents-approved` (JSON: list of `{file, sha256}`).
//!
//! Modification of any approved spec → SHA-256 changes → re-approval required.
//! This is the supply-chain attack defense for cloned repos with malicious
//! `.theo/agents/`.
//!
//! Track A — approval manifest.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Behavior when unapproved specs are encountered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    /// Interactive: callers (CLI/Desktop) must prompt the user. The registry
    /// returns unapproved specs as "pending" — they are NOT loaded until
    /// `persist_approved()` is called.
    Interactive,
    /// Non-interactive (CI mode): unapproved specs are silently skipped with
    /// a warning. Use for automation where prompts are impossible.
    NonInteractive,
    /// Trust all specs from the directory (e.g. `--trust-project-agents` flag).
    /// Logs a warning. Use sparingly.
    TrustAll,
}

#[derive(Debug, Error)]
pub enum ApprovalError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid manifest JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

/// One approved entry in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovedEntry {
    /// Filename (relative to the agents directory) — e.g. "security.md".
    pub file: String,
    /// SHA-256 hex digest of the file's full content (frontmatter + body).
    pub sha256: String,
}

/// Persisted approval manifest. Lives at `<project>/.theo/.agents-approved`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalManifest {
    #[serde(default)]
    pub approved: Vec<ApprovedEntry>,
}

impl ApprovalManifest {
    /// True if the given filename + SHA matches a previously-approved entry.
    pub fn is_approved(&self, file: &str, sha256: &str) -> bool {
        self.approved
            .iter()
            .any(|e| e.file == file && e.sha256 == sha256)
    }
}

/// Compute the SHA-256 hex digest of a string.
pub fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let bytes = hasher.finalize();
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        s.push(nibble_to_hex(byte >> 4));
        s.push(nibble_to_hex(byte & 0x0f));
    }
    s
}

fn nibble_to_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + n - 10) as char,
        _ => unreachable!(),
    }
}

/// Path to the manifest file for a given project directory.
pub fn manifest_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".theo").join(".agents-approved")
}

/// Load the approval manifest. Returns `Default` (empty) if file is absent.
pub fn load_approved(project_dir: &Path) -> Result<ApprovalManifest, ApprovalError> {
    let path = manifest_path(project_dir);
    if !path.exists() {
        return Ok(ApprovalManifest::default());
    }
    let content = fs::read_to_string(&path)?;
    let manifest: ApprovalManifest = serde_json::from_str(&content)?;
    Ok(manifest)
}

/// Persist a manifest to disk, ensuring parent dirs exist and `0600` perms.
pub fn persist_approved(
    project_dir: &Path,
    manifest: &ApprovalManifest,
) -> Result<(), ApprovalError> {
    let path = manifest_path(project_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(manifest)?;
    fs::write(&path, content)?;
    set_owner_only_permissions(&path)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Compute the manifest from the current state of a directory.
/// Returns `(filename, sha256)` for each `.md` file.
///
/// I/O errors on individual files are skipped (with no warning here — callers
/// should handle this through the directory-loading path).
pub fn compute_current_manifest(dir: &Path) -> io::Result<Vec<ApprovedEntry>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        entries.push(ApprovedEntry {
            file: file_name.to_string(),
            sha256: sha256_hex(&content),
        });
    }
    // Deterministic ordering for tests + diff stability
    entries.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(entries)
}

/// Diff current state vs approved manifest. Returns specs needing approval.
pub fn diff_unapproved(current: &[ApprovedEntry], approved: &ApprovalManifest) -> Vec<ApprovedEntry> {
    current
        .iter()
        .filter(|c| !approved.is_approved(&c.file, &c.sha256))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_project_with_agents(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        let agents = dir.path().join(".theo").join("agents");
        fs::create_dir_all(&agents).unwrap();
        for (name, content) in files {
            fs::write(agents.join(name), content).unwrap();
        }
        dir
    }

    #[test]
    fn sha256_hex_returns_64_lowercase_hex() {
        let h = sha256_hex("hello");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        // Known SHA-256("hello")
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn sha256_hex_changes_on_content_change() {
        assert_ne!(sha256_hex("a"), sha256_hex("b"));
    }

    #[test]
    fn manifest_is_approved_returns_true_for_match() {
        let m = ApprovalManifest {
            approved: vec![ApprovedEntry {
                file: "x.md".into(),
                sha256: "abc".into(),
            }],
        };
        assert!(m.is_approved("x.md", "abc"));
        assert!(!m.is_approved("x.md", "different"));
        assert!(!m.is_approved("y.md", "abc"));
    }

    #[test]
    fn load_approved_returns_empty_when_file_absent() {
        let dir = TempDir::new().unwrap();
        let m = load_approved(dir.path()).unwrap();
        assert!(m.approved.is_empty());
    }

    #[test]
    fn load_approved_parses_valid_json() {
        let dir = TempDir::new().unwrap();
        let theo_dir = dir.path().join(".theo");
        fs::create_dir_all(&theo_dir).unwrap();
        let manifest_str = r#"{"approved":[{"file":"a.md","sha256":"deadbeef"}]}"#;
        fs::write(theo_dir.join(".agents-approved"), manifest_str).unwrap();

        let m = load_approved(dir.path()).unwrap();
        assert_eq!(m.approved.len(), 1);
        assert_eq!(m.approved[0].file, "a.md");
        assert_eq!(m.approved[0].sha256, "deadbeef");
    }

    #[test]
    fn load_approved_returns_error_on_invalid_json() {
        let dir = TempDir::new().unwrap();
        let theo_dir = dir.path().join(".theo");
        fs::create_dir_all(&theo_dir).unwrap();
        fs::write(theo_dir.join(".agents-approved"), "not json {").unwrap();
        let err = load_approved(dir.path()).unwrap_err();
        assert!(matches!(err, ApprovalError::InvalidJson(_)));
    }

    #[test]
    fn persist_approved_writes_file_and_round_trips() {
        let dir = TempDir::new().unwrap();
        let m = ApprovalManifest {
            approved: vec![ApprovedEntry {
                file: "x.md".into(),
                sha256: "abc".into(),
            }],
        };
        persist_approved(dir.path(), &m).unwrap();
        let back = load_approved(dir.path()).unwrap();
        assert_eq!(back, m);
    }

    #[cfg(unix)]
    #[test]
    fn persist_approved_sets_chmod_600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let m = ApprovalManifest::default();
        persist_approved(dir.path(), &m).unwrap();
        let perms = fs::metadata(manifest_path(dir.path())).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    fn compute_current_manifest_empty_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        let agents = dir.path().join(".theo").join("agents");
        fs::create_dir_all(&agents).unwrap();
        let entries = compute_current_manifest(&agents).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn compute_current_manifest_skips_non_md_files() {
        let dir = make_project_with_agents(&[
            ("a.md", "content a"),
            ("b.txt", "ignored"),
            ("c.md", "content c"),
        ]);
        let entries = compute_current_manifest(&dir.path().join(".theo").join("agents")).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file, "a.md");
        assert_eq!(entries[1].file, "c.md");
    }

    #[test]
    fn compute_current_manifest_returns_sha256_per_file() {
        let dir = make_project_with_agents(&[("a.md", "hello")]);
        let entries = compute_current_manifest(&dir.path().join(".theo").join("agents")).unwrap();
        assert_eq!(entries[0].sha256, sha256_hex("hello"));
    }

    #[test]
    fn diff_unapproved_returns_only_pending() {
        let current = vec![
            ApprovedEntry {
                file: "old.md".into(),
                sha256: "1".into(),
            },
            ApprovedEntry {
                file: "new.md".into(),
                sha256: "2".into(),
            },
        ];
        let approved = ApprovalManifest {
            approved: vec![ApprovedEntry {
                file: "old.md".into(),
                sha256: "1".into(),
            }],
        };
        let pending = diff_unapproved(&current, &approved);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].file, "new.md");
    }

    #[test]
    fn modified_spec_invalidates_previous_approval() {
        // Approve a spec
        let approved = ApprovalManifest {
            approved: vec![ApprovedEntry {
                file: "x.md".into(),
                sha256: sha256_hex("v1"),
            }],
        };
        // Now spec content changed
        let current = vec![ApprovedEntry {
            file: "x.md".into(),
            sha256: sha256_hex("v2"),
        }];
        let pending = diff_unapproved(&current, &approved);
        assert_eq!(pending.len(), 1, "modified spec should be pending again");
        assert_eq!(pending[0].file, "x.md");
    }

    #[test]
    fn unmodified_specs_are_not_pending() {
        let content = "spec content";
        let sha = sha256_hex(content);
        let approved = ApprovalManifest {
            approved: vec![ApprovedEntry {
                file: "x.md".into(),
                sha256: sha.clone(),
            }],
        };
        let current = vec![ApprovedEntry {
            file: "x.md".into(),
            sha256: sha,
        }];
        let pending = diff_unapproved(&current, &approved);
        assert!(pending.is_empty());
    }

    #[test]
    fn manifest_path_is_dot_theo_dot_agents_approved() {
        let p = manifest_path(Path::new("/proj"));
        assert_eq!(p, PathBuf::from("/proj/.theo/.agents-approved"));
    }
}
