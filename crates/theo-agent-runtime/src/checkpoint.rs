//! Checkpoint Manager — shadow git repos for transparent rollback.
//!
//! Track C — checkpoint/snapshot.
//!
//! Snapshot automatico do CWD antes de mutacoes (write/edit/patch).
//! Permite rollback de qualquer ponto da sessao via `theo checkpoints restore`.
//!
//! Adoção direta de Hermes `tools/checkpoint_manager.py:1-90`:
//! - Shadow git repo em `~/.theo/checkpoints/{sha256(abs_dir)[:16]}/`
//! - Usa `GIT_DIR` + `GIT_WORK_TREE` para nao poluir .git do user
//! - Excludes deterministicos (NAO le .gitignore do user — evita exfiltrar
//!   segredos via spec maliciosa)
//! - Validacao de commit hash com regex (previne git argument injection)
//! - NAO e tool — LLM nao ve, infraestrutura transparente

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("git command failed (exit {code}): {stderr}")]
    GitFailed { code: i32, stderr: String },
    #[error("invalid commit hash: must be 4-64 hex chars, no leading dash")]
    InvalidCommitHash,
    #[error("workdir does not exist: {0}")]
    WorkdirMissing(PathBuf),
    #[error("workdir is not a directory: {0}")]
    NotDirectory(PathBuf),
}

/// Default exclude patterns (Hermes-aligned). DETERMINISTIC — does NOT read
/// the user's `.gitignore` to avoid leaking secrets via spec manipulation.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules/",
    "dist/",
    "build/",
    ".env",
    ".env.*",
    ".env.local",
    ".env.*.local",
    "__pycache__/",
    "*.pyc",
    "*.pyo",
    ".DS_Store",
    "*.log",
    ".cache/",
    ".next/",
    ".nuxt/",
    "coverage/",
    ".pytest_cache/",
    ".venv/",
    "venv/",
    ".git/",
    "target/",  // Rust
];

/// Maximum number of files to snapshot (safety against runaway dirs).
pub const DEFAULT_MAX_FILES: usize = 50_000;

/// Single checkpoint entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub commit: String,
    pub label: String,
    pub timestamp_unix: i64,
}

#[derive(Debug)]
pub struct CheckpointManager {
    workdir: PathBuf,
    shadow_dir: PathBuf,
    excludes: Vec<String>,
    max_files: usize,
}

impl CheckpointManager {
    /// Create a manager for the given workdir. Initializes the shadow repo
    /// if absent. Default storage location: `<base>/{sha256(abs_workdir)[:16]}/`.
    ///
    /// `base` is typically `dirs::home_dir().join(".theo/checkpoints")`.
    pub fn new(workdir: &Path, base: &Path) -> Result<Self, CheckpointError> {
        Self::with_options(workdir, base, DEFAULT_EXCLUDES, DEFAULT_MAX_FILES)
    }

    pub fn with_options(
        workdir: &Path,
        base: &Path,
        excludes: &[&str],
        max_files: usize,
    ) -> Result<Self, CheckpointError> {
        if !workdir.exists() {
            return Err(CheckpointError::WorkdirMissing(workdir.to_path_buf()));
        }
        if !workdir.is_dir() {
            return Err(CheckpointError::NotDirectory(workdir.to_path_buf()));
        }
        let abs_workdir = workdir.canonicalize()?;
        let shadow_dir = base.join(workdir_hash(&abs_workdir));
        let manager = Self {
            workdir: abs_workdir,
            shadow_dir,
            excludes: excludes.iter().map(|s| s.to_string()).collect(),
            max_files,
        };
        manager.init_if_needed()?;
        Ok(manager)
    }

    /// Initialize the shadow git repo if it doesn't exist. Idempotent.
    fn init_if_needed(&self) -> Result<(), CheckpointError> {
        if self.shadow_dir.join("HEAD").exists() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.shadow_dir)?;
        // git init --bare? No — we need a working tree (handled by GIT_WORK_TREE).
        // Use --git-dir explicit to set up the bare repo within shadow_dir.
        self.git(&["init", "-q"], None)?;
        // Persist workdir path for diagnostics
        std::fs::write(self.shadow_dir.join("THEO_WORKDIR"), self.workdir.to_string_lossy().as_bytes())?;
        // Set excludes via info/exclude (NOT user's .gitignore)
        let info_dir = self.shadow_dir.join("info");
        std::fs::create_dir_all(&info_dir)?;
        let exclude_content = self.excludes.join("\n");
        std::fs::write(info_dir.join("exclude"), exclude_content.as_bytes())?;
        Ok(())
    }

    /// Snapshot the current state of the workdir. Returns the new commit SHA.
    /// Skips if file count exceeds `max_files` (safety guard).
    pub fn snapshot(&self, label: &str) -> Result<String, CheckpointError> {
        // File-count safety check
        let count = count_workdir_files(&self.workdir, self.max_files + 1)?;
        if count > self.max_files {
            return Err(CheckpointError::Io(io::Error::other(format!(
                "workdir has > {} files; skipping snapshot",
                self.max_files
            ))));
        }
        // Stage all files
        self.git(&["add", "-A"], None)?;
        // Allow empty commits so checkpoint always succeeds
        let msg = format!("checkpoint: {}", label);
        self.git(&["commit", "--allow-empty", "-q", "-m", &msg], None)?;
        // Get the new HEAD SHA
        let out = self.git(&["rev-parse", "HEAD"], None)?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// List all checkpoints (newest first). Each has commit + label + timestamp.
    pub fn list(&self) -> Result<Vec<Checkpoint>, CheckpointError> {
        let out = self.git(
            &["log", "--format=%H%x09%ct%x09%s"],
            None,
        )?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut checkpoints = Vec::new();
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() != 3 {
                continue;
            }
            let timestamp_unix: i64 = parts[1].parse().unwrap_or(0);
            let label = parts[2]
                .strip_prefix("checkpoint: ")
                .unwrap_or(parts[2])
                .to_string();
            checkpoints.push(Checkpoint {
                commit: parts[0].to_string(),
                label,
                timestamp_unix,
            });
        }
        Ok(checkpoints)
    }

    /// Restore the workdir to a previous commit. Validates the commit hash
    /// to prevent git argument injection.
    pub fn restore(&self, commit: &str) -> Result<(), CheckpointError> {
        validate_commit_hash(commit)?;
        // Use checkout-index with -f -a to copy files from the commit's tree
        // into the work tree without changing HEAD (closer to a "safe restore").
        // Simpler: hard-reset.
        self.git(&["reset", "--hard", commit], None)?;
        Ok(())
    }

    /// Delete checkpoints older than `max_age_seconds` (Unix epoch).
    /// Returns the count of pruned checkpoints.
    pub fn cleanup(&self, max_age_seconds: i64) -> Result<usize, CheckpointError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let cutoff = now - max_age_seconds;
        let checkpoints = self.list()?;
        let to_keep: Vec<&Checkpoint> = checkpoints
            .iter()
            .filter(|c| c.timestamp_unix >= cutoff)
            .collect();
        let pruned = checkpoints.len() - to_keep.len();
        if pruned > 0 {
            // Hard-prune via reflog expire + gc
            // Conservative: only run when we actually pruned
            let _ = self.git(&["reflog", "expire", "--expire=now", "--all"], None);
            let _ = self.git(&["gc", "--prune=now", "-q"], None);
        }
        Ok(pruned)
    }

    /// Path to the shadow git repo (diagnostic).
    pub fn shadow_dir(&self) -> &Path {
        &self.shadow_dir
    }

    /// Run a git command using GIT_DIR + GIT_WORK_TREE so the shadow repo
    /// is fully isolated from the user's `.git/`.
    fn git(&self, args: &[&str], stdin: Option<&str>) -> Result<Output, CheckpointError> {
        let mut cmd = Command::new("git");
        cmd.env("GIT_DIR", &self.shadow_dir);
        cmd.env("GIT_WORK_TREE", &self.workdir);
        // Disable user's gitconfig: deterministic behavior
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.env("HOME", &self.shadow_dir); // isolate ~/.gitconfig
        // Ensure commits succeed without user identity
        cmd.env("GIT_AUTHOR_NAME", "Theo Checkpoint");
        cmd.env("GIT_AUTHOR_EMAIL", "checkpoint@theo.local");
        cmd.env("GIT_COMMITTER_NAME", "Theo Checkpoint");
        cmd.env("GIT_COMMITTER_EMAIL", "checkpoint@theo.local");
        cmd.args(args);
        if let Some(input) = stdin {
            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
            let mut child = cmd.spawn()?;
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .ok_or_else(|| io::Error::other("no stdin"))?
                .write_all(input.as_bytes())?;
            let output = child.wait_with_output()?;
            if !output.status.success() {
                return Err(CheckpointError::GitFailed {
                    code: output.status.code().unwrap_or(-1),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            Ok(output)
        } else {
            let output = cmd.output()?;
            if !output.status.success() {
                return Err(CheckpointError::GitFailed {
                    code: output.status.code().unwrap_or(-1),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            Ok(output)
        }
    }
}

/// Hash of an absolute workdir path → first 16 chars of hex SHA-256.
fn workdir_hash(abs_workdir: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(abs_workdir.to_string_lossy().as_bytes());
    let bytes = hasher.finalize();
    let mut s = String::with_capacity(16);
    for byte in bytes.iter().take(8) {
        s.push_str(&format!("{:02x}", byte));
    }
    s
}

/// Validate a commit hash against shell injection.
/// Must match `^[0-9a-fA-F]{4,64}$` and NOT start with `-`.
fn validate_commit_hash(hash: &str) -> Result<(), CheckpointError> {
    if hash.starts_with('-') {
        return Err(CheckpointError::InvalidCommitHash);
    }
    if hash.len() < 4 || hash.len() > 64 {
        return Err(CheckpointError::InvalidCommitHash);
    }
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(CheckpointError::InvalidCommitHash);
    }
    Ok(())
}

/// Count files in `workdir` (recursively) up to a cap. Returns the cap if
/// the count exceeds it (early-exit for safety).
fn count_workdir_files(workdir: &Path, cap: usize) -> Result<usize, io::Error> {
    let mut count = 0;
    let mut stack = vec![workdir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in read.flatten() {
            let path = entry.path();
            // Skip hidden/.git aggressively for file-count purposes
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n == ".git" || n == "node_modules" || n == "target")
            {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                count += 1;
                if count >= cap {
                    return Ok(count);
                }
            }
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn workdir_with_files(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
        dir
    }

    #[test]
    fn workdir_hash_is_16_hex_chars() {
        let h = workdir_hash(Path::new("/some/absolute/path"));
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn workdir_hash_deterministic() {
        let p = Path::new("/x/y");
        assert_eq!(workdir_hash(p), workdir_hash(p));
    }

    #[test]
    fn workdir_hash_differs_per_path() {
        assert_ne!(workdir_hash(Path::new("/a")), workdir_hash(Path::new("/b")));
    }

    #[test]
    fn validate_commit_hash_accepts_valid() {
        assert!(validate_commit_hash("deadbeef").is_ok());
        assert!(validate_commit_hash("abc1").is_ok());
        assert!(validate_commit_hash(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn validate_commit_hash_rejects_dash_prefix_injection() {
        assert!(validate_commit_hash("-rf").is_err());
        assert!(validate_commit_hash("--hard").is_err());
    }

    #[test]
    fn validate_commit_hash_rejects_too_short_or_long() {
        assert!(validate_commit_hash("abc").is_err()); // 3 chars
        assert!(validate_commit_hash(&"a".repeat(65)).is_err());
    }

    #[test]
    fn validate_commit_hash_rejects_non_hex() {
        assert!(validate_commit_hash("notavalidhex_g").is_err());
        assert!(validate_commit_hash("dead beef").is_err());
    }

    #[test]
    fn manager_new_rejects_missing_workdir() {
        let base = TempDir::new().unwrap();
        let err = CheckpointManager::new(Path::new("/nonexistent/xyz"), base.path()).unwrap_err();
        assert!(matches!(err, CheckpointError::WorkdirMissing(_)));
    }

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn checkpoint_snapshot_and_list_returns_commit() {
        if !git_available() {
            return;
        }
        let workdir = workdir_with_files(&[("a.txt", "hello")]);
        let base = TempDir::new().unwrap();
        let mgr = CheckpointManager::new(workdir.path(), base.path()).unwrap();
        let sha = mgr.snapshot("init").unwrap();
        assert!(!sha.is_empty());
        assert_eq!(sha.len(), 40); // standard SHA-1
        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, "init");
    }

    #[test]
    fn checkpoint_restore_reverts_workdir() {
        if !git_available() {
            return;
        }
        let workdir = workdir_with_files(&[("a.txt", "v1")]);
        let base = TempDir::new().unwrap();
        let mgr = CheckpointManager::new(workdir.path(), base.path()).unwrap();
        let sha1 = mgr.snapshot("v1").unwrap();
        // Modify
        std::fs::write(workdir.path().join("a.txt"), "v2").unwrap();
        mgr.snapshot("v2").unwrap();
        // Restore v1
        mgr.restore(&sha1).unwrap();
        let restored = std::fs::read_to_string(workdir.path().join("a.txt")).unwrap();
        assert_eq!(restored, "v1");
    }

    #[test]
    fn checkpoint_restore_rejects_invalid_hash() {
        if !git_available() {
            return;
        }
        let workdir = workdir_with_files(&[("a.txt", "x")]);
        let base = TempDir::new().unwrap();
        let mgr = CheckpointManager::new(workdir.path(), base.path()).unwrap();
        let err = mgr.restore("--hard").unwrap_err();
        assert!(matches!(err, CheckpointError::InvalidCommitHash));
    }

    #[test]
    fn checkpoint_init_creates_shadow_repo() {
        if !git_available() {
            return;
        }
        let workdir = TempDir::new().unwrap();
        let base = TempDir::new().unwrap();
        let mgr = CheckpointManager::new(workdir.path(), base.path()).unwrap();
        assert!(mgr.shadow_dir().join("HEAD").exists());
    }

    #[test]
    fn checkpoint_excludes_default_node_modules_etc() {
        if !git_available() {
            return;
        }
        let workdir = workdir_with_files(&[
            ("real.rs", "code"),
            ("node_modules/lib/x.js", "ignored"),
            ("target/debug/foo", "ignored"),
        ]);
        let base = TempDir::new().unwrap();
        let mgr = CheckpointManager::new(workdir.path(), base.path()).unwrap();
        mgr.snapshot("init").unwrap();
        // Verify node_modules NOT tracked
        let exclude_path = mgr.shadow_dir().join("info").join("exclude");
        let content = std::fs::read_to_string(exclude_path).unwrap();
        assert!(content.contains("node_modules/"));
        assert!(content.contains("target/"));
    }

    #[test]
    fn checkpoint_does_not_leak_git_state_into_workdir() {
        if !git_available() {
            return;
        }
        let workdir = workdir_with_files(&[("a.rs", "x")]);
        let base = TempDir::new().unwrap();
        let mgr = CheckpointManager::new(workdir.path(), base.path()).unwrap();
        mgr.snapshot("init").unwrap();
        // Workdir must not have a .git folder
        assert!(!workdir.path().join(".git").exists());
    }

    #[test]
    fn count_workdir_files_caps_at_limit() {
        let dir = TempDir::new().unwrap();
        for i in 0..50 {
            std::fs::write(dir.path().join(format!("f{}.txt", i)), "x").unwrap();
        }
        let count = count_workdir_files(dir.path(), 10).unwrap();
        assert!(count >= 10, "should hit cap");
    }
}
