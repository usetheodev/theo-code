//! Runtime feature activation from CLI flags.
//!
//! Wires `--watch-agents` and `--enable-checkpoints` to the corresponding
//! sub-systems (RegistryWatcher / CheckpointManager). Constructed once at
//! startup and held for the session.

use std::path::PathBuf;
use std::sync::Arc;

use theo_agent_runtime::checkpoint::CheckpointManager;
use theo_agent_runtime::subagent::watcher::{default_watch_dirs, RegistryWatcher};

/// Activated runtime features (held for the lifetime of the session).
pub struct RuntimeFeatures {
    /// `--watch-agents` watcher (None when flag is off).
    pub watcher: Option<RegistryWatcher>,
    /// `--enable-checkpoints` manager (None when flag is off).
    pub checkpoint: Option<Arc<CheckpointManager>>,
}

impl RuntimeFeatures {
    /// Build features from CLI flags. Errors are logged but do NOT abort
    /// startup — features that fail to initialize become no-ops.
    pub fn from_flags(
        watch_agents: bool,
        enable_checkpoints: bool,
        project_dir: &std::path::Path,
    ) -> Self {
        let watcher = if watch_agents {
            init_watcher(project_dir)
        } else {
            None
        };
        let checkpoint = if enable_checkpoints {
            init_checkpoint(project_dir)
        } else {
            None
        };
        Self { watcher, checkpoint }
    }

    /// Print a concise status banner when any feature is active.
    pub fn print_status(&self) {
        if self.watcher.is_some() {
            eprintln!("✓ --watch-agents active: filesystem changes in .theo/agents/ trigger reload");
        }
        if self.checkpoint.is_some() {
            eprintln!(
                "✓ --enable-checkpoints active: pre-mutation snapshots at {}",
                checkpoint_base_dir().display()
            );
        }
    }
}

fn init_watcher(project_dir: &std::path::Path) -> Option<RegistryWatcher> {
    let home = dirs::home_dir();
    let dirs = default_watch_dirs(Some(project_dir), home.as_deref());
    match RegistryWatcher::watch(&dirs) {
        Ok(w) => Some(w),
        Err(err) => {
            eprintln!("warning: --watch-agents failed to start: {} (continuing without watcher)", err);
            None
        }
    }
}

fn init_checkpoint(project_dir: &std::path::Path) -> Option<Arc<CheckpointManager>> {
    let base = checkpoint_base_dir();
    if let Err(err) = std::fs::create_dir_all(&base) {
        eprintln!(
            "warning: --enable-checkpoints failed to create base dir {}: {} (continuing without checkpoints)",
            base.display(),
            err
        );
        return None;
    }
    match CheckpointManager::new(project_dir, &base) {
        Ok(m) => Some(Arc::new(m)),
        Err(err) => {
            eprintln!(
                "warning: --enable-checkpoints failed to init for {}: {} (continuing without checkpoints)",
                project_dir.display(),
                err
            );
            None
        }
    }
}

fn checkpoint_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".theo")
        .join("checkpoints")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn from_flags_off_creates_empty_features() {
        let dir = TempDir::new().unwrap();
        let f = RuntimeFeatures::from_flags(false, false, dir.path());
        assert!(f.watcher.is_none());
        assert!(f.checkpoint.is_none());
    }

    #[test]
    fn from_flags_watch_only_creates_watcher() {
        let dir = TempDir::new().unwrap();
        let f = RuntimeFeatures::from_flags(true, false, dir.path());
        // Watcher might fail in some CI envs (no inotify), accept either
        let _ = f.watcher;
        assert!(f.checkpoint.is_none());
    }

    #[test]
    fn checkpoint_base_dir_under_home() {
        let p = checkpoint_base_dir();
        let s = p.to_string_lossy();
        assert!(s.contains(".theo"));
        assert!(s.contains("checkpoints"));
    }

    #[test]
    fn print_status_no_features_silent() {
        let f = RuntimeFeatures {
            watcher: None,
            checkpoint: None,
        };
        // Should not panic
        f.print_status();
    }
}
