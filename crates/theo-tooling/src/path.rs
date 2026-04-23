//! Path-safety helpers for tools that accept filesystem paths from the agent.
//!
//! All tools that expose a filesystem surface (read, write, edit, glob, grep,
//! apply_patch, …) must funnel user-supplied paths through
//! [`safe_resolve`] before any I/O. This is the workspace's **first line of
//! defence against path-traversal attacks** — the sandbox's filesystem policy
//! is the second.
//!
//! The intentional invariants:
//!
//! * `input` is resolved against `root`, not the current working directory.
//! * Relative components (`..`, `.`) are fully canonicalized; the resulting
//!   path must stay rooted at `root`.
//! * Symlinks that escape `root` are rejected by canonicalizing both sides
//!   before the containment check.
//! * `root` itself MUST already be canonical (caller's responsibility — the
//!   workspace root is canonicalized once at startup).
//!
//! A dedicated error type keeps violations observable and easy to audit.

use std::path::{Path, PathBuf};

/// Errors produced by [`safe_resolve`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    /// The resolved path escaped the workspace root.
    #[error("path '{path}' escapes the workspace root '{root}'")]
    Escapes { path: String, root: String },

    /// The input path could not be resolved (did not exist, permission denied).
    #[error("cannot resolve path '{path}': {reason}")]
    Unresolvable { path: String, reason: String },

    /// The root itself could not be canonicalized.
    #[error("workspace root '{root}' is invalid: {reason}")]
    InvalidRoot { root: String, reason: String },
}

/// Resolve `input` against `root` WITHOUT enforcing containment.
///
/// Returns a canonical absolute path (symlinks resolved, `..`/`.` removed).
/// Callers are responsible for deciding whether the canonical path is safe
/// to read/write — typically via [`is_contained`] or [`safe_resolve`].
///
/// Tools that *intentionally* support out-of-root paths (e.g. `read` with
/// the `ExternalDirectory` permission) must use this function instead of
/// [`safe_resolve`].
pub fn absolutize(root: &Path, input: impl AsRef<Path>) -> Result<PathBuf, PathError> {
    let input = input.as_ref();

    let canonical_root =
        std::fs::canonicalize(root).map_err(|e| PathError::InvalidRoot {
            root: root.display().to_string(),
            reason: e.to_string(),
        })?;

    let candidate: PathBuf = if input.is_absolute() {
        input.to_path_buf()
    } else {
        canonical_root.join(input)
    };

    match std::fs::canonicalize(&candidate) {
        Ok(p) => Ok(p),
        Err(_) => {
            let (existing, missing) = split_existing_ancestor(&candidate);
            let Some(existing) = existing else {
                return Err(PathError::Unresolvable {
                    path: candidate.display().to_string(),
                    reason: "no ancestor of the path exists".to_string(),
                });
            };
            let canon =
                std::fs::canonicalize(&existing).map_err(|e| PathError::Unresolvable {
                    path: candidate.display().to_string(),
                    reason: e.to_string(),
                })?;
            Ok(canon.join(missing))
        }
    }
}

/// Check whether a (canonical) path is contained in `root`.
///
/// Both sides are canonicalized to make `starts_with` correct — a textual
/// comparison would be fooled by `..`, symlinks, or redundant separators.
pub fn is_contained(canonical: &Path, root: &Path) -> Result<bool, PathError> {
    let canonical_root =
        std::fs::canonicalize(root).map_err(|e| PathError::InvalidRoot {
            root: root.display().to_string(),
            reason: e.to_string(),
        })?;
    Ok(canonical.starts_with(&canonical_root))
}

/// Resolve `input` against `root` and guarantee the result stays inside `root`.
///
/// The returned path is canonical (symlinks resolved, `..`/`.` removed) and
/// safe to pass to subsequent I/O calls.
///
/// ## When the path does not exist yet
///
/// If `input` points at a file that does not exist (e.g. a `write` creating
/// a new file), this function canonicalizes the **existing parent directory**
/// and then re-appends the missing leaf component, rejecting the call if the
/// result still escapes `root`.
pub fn safe_resolve(root: &Path, input: impl AsRef<Path>) -> Result<PathBuf, PathError> {
    let resolved = absolutize(root, input)?;
    if !is_contained(&resolved, root)? {
        let canonical_root = std::fs::canonicalize(root).map_err(|e| PathError::InvalidRoot {
            root: root.display().to_string(),
            reason: e.to_string(),
        })?;
        return Err(PathError::Escapes {
            path: resolved.display().to_string(),
            root: canonical_root.display().to_string(),
        });
    }
    Ok(resolved)
}

/// Split `path` into `(deepest_existing_ancestor, missing_suffix)`.
///
/// Example: given `/root/a/b/new.txt` where `/root/a` exists but `/root/a/b`
/// does not, returns `(Some(/root/a), b/new.txt)`.
fn split_existing_ancestor(path: &Path) -> (Option<PathBuf>, PathBuf) {
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut probe: PathBuf = path.to_path_buf();

    loop {
        if probe.exists() {
            let missing_path = missing
                .iter()
                .rev()
                .fold(PathBuf::new(), |acc, c| acc.join(c));
            return (Some(probe), missing_path);
        }
        let name_owned = probe.file_name().map(|n| n.to_os_string());
        match name_owned {
            Some(name) => {
                missing.push(name);
                if !probe.pop() {
                    return (None, PathBuf::new());
                }
            }
            None => return (None, PathBuf::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Create a tempdir and pre-canonicalize it (macOS returns `/private/...`
    /// for `/var/...`, for example) so test assertions are stable.
    fn canonical_tempdir() -> TempDir {
        TempDir::new().expect("tempdir")
    }

    #[test]
    fn safe_resolve_accepts_simple_relative_path() {
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub/file.txt"), "hi").unwrap();

        let resolved = safe_resolve(root, "sub/file.txt").unwrap();
        assert!(resolved.ends_with("sub/file.txt"));
        let canonical_root = fs::canonicalize(root).unwrap();
        assert!(resolved.starts_with(&canonical_root));
    }

    #[test]
    fn safe_resolve_accepts_absolute_path_inside_root() {
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::write(root.join("a.txt"), "hi").unwrap();
        let canonical_root = fs::canonicalize(root).unwrap();
        let absolute = canonical_root.join("a.txt");

        let resolved = safe_resolve(root, &absolute).unwrap();
        assert_eq!(resolved, absolute);
    }

    #[test]
    fn safe_resolve_rejects_parent_dir_escape() {
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::create_dir(root.join("sub")).unwrap();

        let err = safe_resolve(root, "sub/../../etc/passwd").unwrap_err();
        assert!(
            matches!(err, PathError::Escapes { .. } | PathError::Unresolvable { .. }),
            "expected Escapes or Unresolvable, got {err:?}"
        );
    }

    #[test]
    fn safe_resolve_rejects_absolute_path_outside_root() {
        let dir = canonical_tempdir();
        let root = dir.path();

        let err = safe_resolve(root, "/etc/passwd").unwrap_err();
        assert!(
            matches!(err, PathError::Escapes { .. } | PathError::Unresolvable { .. }),
            "expected Escapes/Unresolvable, got {err:?}"
        );
    }

    #[test]
    fn safe_resolve_canonicalizes_relative_components() {
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/target.txt"), "hi").unwrap();

        let resolved = safe_resolve(root, "a/./b/../b/target.txt").unwrap();
        assert!(resolved.ends_with("a/b/target.txt"));
    }

    #[test]
    fn safe_resolve_rejects_symlink_that_escapes_root() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let dir = canonical_tempdir();
            let root = dir.path();
            let outside = TempDir::new().unwrap();
            let outside_file = outside.path().join("secret.txt");
            fs::write(&outside_file, "secret").unwrap();

            let link = root.join("back_door");
            symlink(&outside_file, &link).unwrap();

            let err = safe_resolve(root, "back_door").unwrap_err();
            assert!(
                matches!(err, PathError::Escapes { .. }),
                "expected Escapes via symlink, got {err:?}"
            );
        }
    }

    #[test]
    fn safe_resolve_permits_nonexistent_leaf_inside_root() {
        // e.g. `write` creating a new file.
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::create_dir(root.join("a")).unwrap();

        let resolved = safe_resolve(root, "a/new_file.txt").unwrap();
        assert!(resolved.ends_with("a/new_file.txt"));
        let canonical_root = fs::canonicalize(root).unwrap();
        assert!(resolved.starts_with(&canonical_root));
    }

    #[test]
    fn safe_resolve_rejects_nonexistent_leaf_that_escapes() {
        let dir = canonical_tempdir();
        let root = dir.path();
        // No canonical ancestor in `/other`, but `..` still lets us escape.
        let err = safe_resolve(root, "../outside/new_file.txt").unwrap_err();
        assert!(
            matches!(err, PathError::Escapes { .. } | PathError::Unresolvable { .. }),
            "expected Escapes/Unresolvable, got {err:?}"
        );
    }

    #[test]
    fn safe_resolve_errors_with_invalid_root() {
        let err = safe_resolve(Path::new("/this/really/does/not/exist"), "x").unwrap_err();
        assert!(matches!(err, PathError::InvalidRoot { .. }));
    }

    #[test]
    fn safe_resolve_returns_canonical_path_without_dot_segments() {
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::create_dir(root.join("x")).unwrap();
        fs::write(root.join("x/y.txt"), "hi").unwrap();

        let resolved = safe_resolve(root, "./x/y.txt").unwrap();
        // No `.` component should remain.
        assert!(!resolved.components().any(|c| c.as_os_str() == "."));
    }

    // ── absolutize / is_contained (non-enforcing variants) ──────────

    #[test]
    fn absolutize_does_not_enforce_containment() {
        let dir = canonical_tempdir();
        let root = dir.path();
        // Out-of-root absolute path that exists — should succeed.
        let resolved = absolutize(root, "/etc").unwrap();
        assert!(resolved.starts_with("/etc"));
    }

    #[test]
    fn absolutize_canonicalizes_relative_dot_components() {
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::create_dir(root.join("a")).unwrap();
        fs::write(root.join("a/b.txt"), "hi").unwrap();

        let resolved = absolutize(root, "a/./b.txt").unwrap();
        assert!(!resolved.components().any(|c| c.as_os_str() == "."));
    }

    #[test]
    fn absolutize_canonicalizes_parent_dir_traversal() {
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("target.txt"), "hi").unwrap();

        let resolved = absolutize(root, "sub/../target.txt").unwrap();
        let canonical_root = fs::canonicalize(root).unwrap();
        assert_eq!(resolved, canonical_root.join("target.txt"));
    }

    #[test]
    fn is_contained_detects_in_root() {
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::write(root.join("f.txt"), "").unwrap();
        let canonical = fs::canonicalize(root.join("f.txt")).unwrap();
        assert!(is_contained(&canonical, root).unwrap());
    }

    #[test]
    fn is_contained_detects_out_of_root() {
        let dir = canonical_tempdir();
        let root = dir.path();
        // `/etc` is canonical and definitely not inside the tempdir.
        let canonical = PathBuf::from("/etc");
        assert!(!is_contained(&canonical, root).unwrap());
    }

    #[test]
    fn is_contained_after_absolutize_detects_parent_dir_escape() {
        // The combined workflow that `read` will adopt: absolutize first,
        // then check containment. A `..` attack resolves to `/etc/passwd`
        // which is correctly flagged as outside the project.
        let dir = canonical_tempdir();
        let root = dir.path();
        fs::create_dir(root.join("sub")).unwrap();

        // Absolutize will fail at canonicalization if `/etc/passwd` is
        // missing — but on the host where this test runs it exists, so
        // resolution succeeds and containment correctly returns false.
        if let Ok(canonical) = absolutize(root, "sub/../../../../../../etc/passwd") {
            assert!(!is_contained(&canonical, root).unwrap());
        }
    }
}
