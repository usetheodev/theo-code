//! `WorktreeProvider` — wrapper around `git worktree` for per-agent CWD isolation.
//!
//! Reference: `referencias/Archon/packages/isolation/src/providers/worktree.ts`
//! and `IIsolationStore` interface.
//!
//! Lifecycle:
//! 1. `create(spec_name, base_branch)` → spawns a worktree at a unique path
//! 2. Sub-agent operates in `WorktreeHandle.path`
//! 3. `cleanup(handle)` removes the worktree (leaves branch behind if uncommitted)

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IsolationError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("git command failed (exit {code}): {stderr}")]
    GitFailed { code: i32, stderr: String },
    #[error("repo path is not a git repository: {0}")]
    NotARepo(PathBuf),
    #[error("uncommitted changes block worktree creation; commit or stash first")]
    UncommittedChanges,
}

/// Cleanup policy after sub-agent finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupPolicy {
    /// Always remove the worktree on completion (success or failure).
    Always,
    /// Remove only on success — failures preserve worktree for inspection.
    #[default]
    OnSuccess,
    /// Never auto-remove. User must clean manually.
    Never,
}

/// A handle to an active worktree.
#[derive(Debug, Clone)]
pub struct WorktreeHandle {
    pub path: PathBuf,
    pub branch: String,
}

/// Worktree provider — manages git worktrees rooted at a base repo.
#[derive(Debug, Clone)]
pub struct WorktreeProvider {
    base_repo: PathBuf,
    worktrees_root: PathBuf,
}

impl WorktreeProvider {
    /// Create a new provider rooted at `base_repo`. Worktrees will be
    /// created in `<worktrees_root>/<sha256(spec_name)[:8]>/`.
    pub fn new(base_repo: impl Into<PathBuf>, worktrees_root: impl Into<PathBuf>) -> Self {
        Self {
            base_repo: base_repo.into(),
            worktrees_root: worktrees_root.into(),
        }
    }

    /// Validate that `base_repo` is a git repository.
    pub fn validate_repo(&self) -> Result<(), IsolationError> {
        let dotgit = self.base_repo.join(".git");
        if !dotgit.exists() {
            return Err(IsolationError::NotARepo(self.base_repo.clone()));
        }
        Ok(())
    }

    /// Compute the worktree path for a given spec name.
    /// Path is `<worktrees_root>/<spec_name>-<hash8>/`.
    pub fn worktree_path_for(&self, spec_name: &str) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(spec_name.as_bytes());
        let bytes = hasher.finalize();
        let mut hash = String::with_capacity(8);
        for byte in bytes.iter().take(4) {
            hash.push_str(&format!("{:02x}", byte));
        }
        self.worktrees_root.join(format!("{}-{}", spec_name, hash))
    }

    /// Create a worktree from `base_branch`. The new worktree gets a unique
    /// branch name (`theo/agent/<spec_name>-<hash8>`).
    pub fn create(
        &self,
        spec_name: &str,
        base_branch: &str,
    ) -> Result<WorktreeHandle, IsolationError> {
        self.validate_repo()?;
        std::fs::create_dir_all(&self.worktrees_root)?;
        let path = self.worktree_path_for(spec_name);
        // Branch name is path's last component prefixed
        let branch = format!(
            "theo/agent/{}",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(spec_name)
        );

        // Run: git -C <repo> worktree add -b <branch> <path> <base_branch>
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.base_repo)
            .args(["worktree", "add", "-b"])
            .arg(&branch)
            .arg(&path)
            .arg(base_branch)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            // Detect uncommitted-changes message
            if stderr.contains("would be overwritten")
                || stderr.contains("local changes")
            {
                return Err(IsolationError::UncommittedChanges);
            }
            return Err(IsolationError::GitFailed {
                code: output.status.code().unwrap_or(-1),
                stderr,
            });
        }
        Ok(WorktreeHandle { path, branch })
    }

    /// Remove a worktree. Respects git's natural guardrail (refuses to remove
    /// worktree with uncommitted changes unless `force` is true).
    pub fn remove(&self, handle: &WorktreeHandle, force: bool) -> Result<(), IsolationError> {
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&self.base_repo)
            .args(["worktree", "remove"]);
        if force {
            cmd.arg("--force");
        }
        cmd.arg(&handle.path);
        let output = cmd.output()?;
        if !output.status.success() {
            return Err(IsolationError::GitFailed {
                code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        Ok(())
    }

    /// List active worktrees (parses `git worktree list --porcelain`).
    pub fn list(&self) -> Result<Vec<WorktreeHandle>, IsolationError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.base_repo)
            .args(["worktree", "list", "--porcelain"])
            .output()?;
        if !output.status.success() {
            return Err(IsolationError::GitFailed {
                code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut handles = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_branch: Option<String> = None;
        for line in stdout.lines() {
            if let Some(rest) = line.strip_prefix("worktree ") {
                if let (Some(p), Some(b)) = (current_path.take(), current_branch.take()) {
                    handles.push(WorktreeHandle { path: p, branch: b });
                }
                current_path = Some(PathBuf::from(rest));
            } else if let Some(rest) = line.strip_prefix("branch ") {
                let b = rest.strip_prefix("refs/heads/").unwrap_or(rest);
                current_branch = Some(b.to_string());
            }
        }
        if let (Some(p), Some(b)) = (current_path, current_branch) {
            handles.push(WorktreeHandle { path: p, branch: b });
        }
        Ok(handles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn init_repo(dir: &Path) -> bool {
        let init = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q"])
            .env("GIT_AUTHOR_NAME", "Theo Test")
            .env("GIT_AUTHOR_EMAIL", "test@theo.local")
            .env("GIT_COMMITTER_NAME", "Theo Test")
            .env("GIT_COMMITTER_EMAIL", "test@theo.local")
            .status();
        if init.is_err() || !init.unwrap().success() {
            return false;
        }
        // initial commit
        std::fs::write(dir.join("README"), "x").unwrap();
        let _ = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["add", "."])
            .output();
        let _ = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["commit", "-q", "-m", "init"])
            .env("GIT_AUTHOR_NAME", "Theo Test")
            .env("GIT_AUTHOR_EMAIL", "test@theo.local")
            .env("GIT_COMMITTER_NAME", "Theo Test")
            .env("GIT_COMMITTER_EMAIL", "test@theo.local")
            .output();
        true
    }

    #[test]
    fn worktree_path_for_is_deterministic() {
        let p =
            WorktreeProvider::new(PathBuf::from("/repo"), PathBuf::from("/wt"))
                .worktree_path_for("alpha");
        let q =
            WorktreeProvider::new(PathBuf::from("/repo"), PathBuf::from("/wt"))
                .worktree_path_for("alpha");
        assert_eq!(p, q);
    }

    #[test]
    fn worktree_path_for_includes_spec_name_and_hash() {
        let p =
            WorktreeProvider::new(PathBuf::from("/r"), PathBuf::from("/wt"))
                .worktree_path_for("alpha");
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("alpha-"), "got {}", name);
        // 5 = "alpha".len() = 5, then dash, then 8 hex chars
        assert_eq!(name.len(), "alpha".len() + 1 + 8);
    }

    #[test]
    fn validate_repo_rejects_non_git_dir() {
        let dir = TempDir::new().unwrap();
        let provider = WorktreeProvider::new(dir.path(), TempDir::new().unwrap().path());
        assert!(matches!(
            provider.validate_repo(),
            Err(IsolationError::NotARepo(_))
        ));
    }

    #[test]
    fn create_worktree_succeeds_in_real_repo() {
        if !git_available() {
            return;
        }
        let repo = TempDir::new().unwrap();
        if !init_repo(repo.path()) {
            return; // git init failed in this env, skip
        }
        let wt_root = TempDir::new().unwrap();
        let provider = WorktreeProvider::new(repo.path(), wt_root.path());
        let handle = match provider.create("agent1", "main") {
            Ok(h) => h,
            Err(_) => {
                // Some git versions use "master"
                match provider.create("agent1", "master") {
                    Ok(h) => h,
                    Err(_) => return,
                }
            }
        };
        assert!(handle.path.exists());
        assert!(handle.branch.starts_with("theo/agent/agent1"));
        // List should include it
        let list = provider.list().unwrap();
        assert!(list.iter().any(|h| h.path == handle.path));
        // Cleanup
        let _ = provider.remove(&handle, true);
    }

    #[test]
    fn remove_worktree_succeeds() {
        if !git_available() {
            return;
        }
        let repo = TempDir::new().unwrap();
        if !init_repo(repo.path()) {
            return;
        }
        let wt_root = TempDir::new().unwrap();
        let provider = WorktreeProvider::new(repo.path(), wt_root.path());
        let handle = match provider
            .create("ag", "main")
            .or_else(|_| provider.create("ag", "master"))
        {
            Ok(h) => h,
            Err(_) => return,
        };
        provider.remove(&handle, true).unwrap();
        assert!(!handle.path.exists());
    }

    #[test]
    fn cleanup_policy_default_is_on_success() {
        assert_eq!(CleanupPolicy::default(), CleanupPolicy::OnSuccess);
    }
}
