//! Runtime feature activation from CLI flags.
//!
//! Wires `--watch-agents` and `--enable-checkpoints` to the corresponding
//! sub-systems (RegistryWatcher → ReloadableRegistry / CheckpointManager).
//! Constructed once at startup and held for the session.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use theo_agent_runtime::checkpoint::CheckpointManager;
use theo_agent_runtime::subagent::watcher::{default_watch_dirs, RegistryWatcher};
use theo_agent_runtime::subagent::ReloadableRegistry;

/// Activated runtime features (held for the lifetime of the session).
pub struct RuntimeFeatures {
    /// `--watch-agents` watcher (None when flag is off).
    /// When Some, a background thread also drains its events and calls
    /// `reloadable.reload()` to keep the registry fresh.
    pub watcher: Option<RegistryWatcher>,
    /// Reloadable registry shared with the agent (when --watch-agents).
    pub reloadable: Option<ReloadableRegistry>,
    /// `--enable-checkpoints` manager (None when flag is off).
    pub checkpoint: Option<Arc<CheckpointManager>>,
    /// Handle for the background reload thread (held to keep it alive).
    /// Dropping this terminates the loop on next iteration.
    _reload_thread: Option<thread::JoinHandle<()>>,
}

impl RuntimeFeatures {
    /// Build features from CLI flags. Errors are logged but do NOT abort
    /// startup — features that fail to initialize become no-ops.
    pub fn from_flags(
        watch_agents: bool,
        enable_checkpoints: bool,
        project_dir: &std::path::Path,
    ) -> Self {
        let (watcher, reloadable, reload_thread) = if watch_agents {
            init_watcher_with_reload(project_dir)
        } else {
            (None, None, None)
        };
        let checkpoint = if enable_checkpoints {
            init_checkpoint(project_dir)
        } else {
            None
        };
        Self {
            watcher,
            reloadable,
            checkpoint,
            _reload_thread: reload_thread,
        }
    }

    /// Print a concise status banner when any feature is active.
    pub fn print_status(&self) {
        if self.watcher.is_some() {
            eprintln!(
                "✓ --watch-agents active: filesystem changes in .theo/agents/ trigger registry reload"
            );
        }
        if self.checkpoint.is_some() {
            eprintln!(
                "✓ --enable-checkpoints active: pre-mutation snapshots at {}",
                checkpoint_base_dir().display()
            );
        }
    }
}

/// Initialize watcher + reloadable registry + background thread that
/// drains events and triggers `reload()`.
///
/// The watcher is owned by the background thread (it can't be Cloned).
/// When the thread terminates, the watcher drops and stops emitting events.
fn init_watcher_with_reload(
    project_dir: &std::path::Path,
) -> (
    Option<RegistryWatcher>,
    Option<ReloadableRegistry>,
    Option<thread::JoinHandle<()>>,
) {
    use theo_agent_runtime::subagent::{ApprovalMode, SubAgentRegistry};
    let home = dirs::home_dir();
    let dirs = default_watch_dirs(Some(project_dir), home.as_deref());

    // Build reloadable registry pointing to project + global agent dirs.
    let mut initial = SubAgentRegistry::with_builtins();
    let _ = initial.load_all(Some(project_dir), home.as_deref(), ApprovalMode::TrustAll);
    let reloadable = ReloadableRegistry::new(
        initial,
        Some(project_dir.to_path_buf()),
        home.clone(),
        ApprovalMode::TrustAll,
    );

    // Background drain thread: own the watcher, poll events with timeout,
    // and reload the shared registry on every change.
    let reloadable_for_thread = reloadable.clone();
    let dirs_for_thread = dirs.clone();
    let thread = thread::Builder::new()
        .name("theo-agents-watcher".into())
        .spawn(move || {
            let watcher = match RegistryWatcher::watch(&dirs_for_thread) {
                Ok(w) => w,
                Err(err) => {
                    eprintln!(
                        "warning: --watch-agents thread failed to start watcher: {}",
                        err
                    );
                    return;
                }
            };
            loop {
                if watcher
                    .next_timeout(std::time::Duration::from_millis(500))
                    .is_some()
                {
                    reloadable_for_thread.reload();
                }
                // Drain any extra coalesced events.
                while watcher.try_next().is_some() {}
            }
        })
        .ok();

    (None, Some(reloadable), thread)
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
        assert!(f.reloadable.is_none());
        assert!(f.checkpoint.is_none());
    }

    #[test]
    fn from_flags_watch_only_creates_reloadable() {
        let dir = TempDir::new().unwrap();
        let f = RuntimeFeatures::from_flags(true, false, dir.path());
        // The watcher field is None because ownership moves to the
        // background thread; the reloadable registry MUST be present.
        assert!(f.reloadable.is_some());
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
            reloadable: None,
            checkpoint: None,
            _reload_thread: None,
        };
        // Should not panic
        f.print_status();
    }
}
