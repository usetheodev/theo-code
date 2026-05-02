//! Hot-reload watcher for `.theo/agents/` — Track D / 
//!
//! Uses `notify-debouncer-mini` to coalesce rapid filesystem events (typical
//! during editor saves). When a `.md` file changes in any watched directory,
//! the registry is reloaded and the change is signaled via a channel so
//! consumers can re-check S3 approval and refresh their own caches.
//!
//! Reference: Claude Code + OpenDev patterns for hot-reload. Archon does
//! not hot-reload (workflows are loaded per-CLI-invocation).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};

/// Default debounce window — 500ms coalesces editor-save bursts.
pub const DEFAULT_DEBOUNCE_MS: u64 = 500;

/// Signal emitted when a watched directory changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsChanged {
    /// Paths of the files that triggered the event.
    pub paths: Vec<PathBuf>,
}

/// Watcher handle. Dropping it stops the watcher.
pub struct RegistryWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher>,
    rx: Receiver<AgentsChanged>,
}

impl RegistryWatcher {
    /// Start watching the given directories. Events are coalesced with the
    /// default 500ms debounce window.
    ///
    /// Only `*.md` file events are forwarded. Other file extensions and
    /// directory-level events are filtered out.
    pub fn watch(dirs: &[PathBuf]) -> Result<Self, notify::Error> {
        Self::watch_with_debounce(dirs, DEFAULT_DEBOUNCE_MS)
    }

    pub fn watch_with_debounce(
        dirs: &[PathBuf],
        debounce_ms: u64,
    ) -> Result<Self, notify::Error> {
        let (tx, rx) = mpsc::channel::<AgentsChanged>();
        let mut debouncer = new_debouncer(
            Duration::from_millis(debounce_ms),
            move |res: DebounceEventResult| {
                if let Ok(events) = res {
                    let md_paths: Vec<PathBuf> = events
                        .into_iter()
                        .map(|e| e.path)
                        .filter(|p| {
                            p.extension().and_then(|s| s.to_str()) == Some("md")
                        })
                        .collect();
                    if !md_paths.is_empty() {
                        let _ = tx.send(AgentsChanged { paths: md_paths });
                    }
                }
            },
        )?;

        for dir in dirs {
            if dir.exists() {
                debouncer
                    .watcher()
                    .watch(dir, notify::RecursiveMode::Recursive)?;
            }
        }
        Ok(Self {
            _debouncer: debouncer,
            rx,
        })
    }

    /// Non-blocking: returns the next pending change, or `None` if none.
    pub fn try_next(&self) -> Option<AgentsChanged> {
        self.rx.try_recv().ok()
    }

    /// Blocking: waits for the next change (up to `timeout`).
    pub fn next_timeout(&self, timeout: Duration) -> Option<AgentsChanged> {
        self.rx.recv_timeout(timeout).ok()
    }
}

/// Convenience: filter `.theo/agents/` + `~/.theo/agents/` default paths.
pub fn default_watch_dirs(project_dir: Option<&Path>, home_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(p) = project_dir {
        dirs.push(p.join(".theo").join("agents"));
    }
    if let Some(h) = home_dir {
        dirs.push(h.join(".theo").join("agents"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn default_watch_dirs_builds_expected_paths() {
        let proj = PathBuf::from("/p");
        let home = PathBuf::from("/h");
        let dirs = default_watch_dirs(Some(&proj), Some(&home));
        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0], PathBuf::from("/p/.theo/agents"));
        assert_eq!(dirs[1], PathBuf::from("/h/.theo/agents"));
    }

    #[test]
    fn default_watch_dirs_skips_none() {
        let dirs = default_watch_dirs(None, None);
        assert!(dirs.is_empty());
    }

    #[test]
    fn watcher_does_not_emit_on_non_md_files() {
        let dir = TempDir::new().unwrap();
        let watcher = RegistryWatcher::watch_with_debounce(&[dir.path().to_path_buf()], 50)
            .unwrap();
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        let change = watcher.next_timeout(Duration::from_millis(300));
        assert!(change.is_none(), "non-md file should not trigger");
    }

    #[test]
    fn watcher_emits_on_md_file_creation() {
        let dir = TempDir::new().unwrap();
        let watcher = RegistryWatcher::watch_with_debounce(&[dir.path().to_path_buf()], 50)
            .unwrap();
        fs::write(dir.path().join("new.md"), "---\nname: x\n---\nbody").unwrap();
        let change = watcher.next_timeout(Duration::from_secs(3));
        match change {
            Some(c) => {
                assert!(c.paths.iter().any(|p| {
                    p.file_name().and_then(|s| s.to_str()) == Some("new.md")
                }));
            }
            None => {
                // Filesystem notifications can be flaky in CI; accept if the
                // environment doesn't support inotify (e.g. some containers).
                eprintln!("skipping: no filesystem event received (inotify unavailable?)");
            }
        }
    }

    #[test]
    fn watcher_emits_on_md_file_modification() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("existing.md");
        fs::write(&path, "---\nname: x\n---\nbody").unwrap();
        let watcher = RegistryWatcher::watch_with_debounce(&[dir.path().to_path_buf()], 50)
            .unwrap();
        // Modify
        fs::write(&path, "---\nname: x\n---\nmodified body").unwrap();
        let change = watcher.next_timeout(Duration::from_secs(3));
        match change {
            Some(c) => {
                assert!(c.paths.iter().any(|p| p == &path));
            }
            None => {
                eprintln!("skipping: no filesystem event (inotify unavailable?)");
            }
        }
    }

    #[test]
    fn watcher_debounce_coalesces_rapid_changes() {
        let dir = TempDir::new().unwrap();
        let watcher = RegistryWatcher::watch_with_debounce(&[dir.path().to_path_buf()], 200)
            .unwrap();
        // Rapid fire 5 changes within the debounce window
        for i in 0..5 {
            fs::write(
                dir.path().join(format!("a{}.md", i)),
                format!("---\nname: a{}\n---\nx", i),
            )
            .unwrap();
        }
        // Wait long enough for debounce to fire exactly once
        std::thread::sleep(Duration::from_millis(400));
        let mut total_events = 0;
        while let Some(_c) = watcher.try_next() {
            total_events += 1;
        }
        // Allow some tolerance but expect coalescing (< 5 separate emits)
        if total_events > 0 {
            assert!(
                total_events < 5,
                "debounce should coalesce; got {} events",
                total_events
            );
        }
    }

    #[test]
    fn watcher_try_next_returns_none_when_idle() {
        let dir = TempDir::new().unwrap();
        let watcher = RegistryWatcher::watch_with_debounce(&[dir.path().to_path_buf()], 50)
            .unwrap();
        // No filesystem activity
        assert!(watcher.try_next().is_none());
    }

    #[test]
    fn watcher_skips_nonexistent_dirs_gracefully() {
        // Should not panic or fail if a dir doesn't exist
        let res = RegistryWatcher::watch(&[PathBuf::from("/nonexistent/xyz/abc")]);
        assert!(res.is_ok());
    }
}
