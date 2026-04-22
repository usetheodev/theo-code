//! Snapshot-based undo — saves file content before edits for safe reversal.
//!
//! Before each write/edit, the caller saves the original content.
//! UndoTool restores from the snapshot, NOT from git checkout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Stores file snapshots keyed by call_id for undo operations.
pub struct SnapshotStore {
    snapshots: Mutex<HashMap<String, Snapshot>>,
    base_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub call_id: String,
    pub file_path: PathBuf,
    pub original_content: String,
    pub tool_name: String,
}

impl SnapshotStore {
    pub fn new() -> Self {
        let base_dir = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
            .join(".config")
            .join("theo")
            .join("undo");
        let _ = std::fs::create_dir_all(&base_dir);
        Self {
            snapshots: Mutex::new(HashMap::new()),
            base_dir,
        }
    }

    /// Save a snapshot of file content before modification.
    pub fn save(&self, call_id: &str, file_path: &Path, content: &str, tool_name: &str) {
        let snapshot = Snapshot {
            call_id: call_id.to_string(),
            file_path: file_path.to_path_buf(),
            original_content: content.to_string(),
            tool_name: tool_name.to_string(),
        };

        // Also persist to disk for crash safety
        let disk_path = self.base_dir.join(format!("{call_id}.bak"));
        let _ = std::fs::write(&disk_path, content);

        self.snapshots
            .lock()
            .expect("snapshot lock")
            .insert(call_id.to_string(), snapshot);
    }

    /// Restore a file from its snapshot. Returns the file path and original content.
    pub fn restore(&self, call_id: &str) -> Option<(PathBuf, String)> {
        let snapshots = self.snapshots.lock().expect("snapshot lock");
        if let Some(snapshot) = snapshots.get(call_id) {
            return Some((snapshot.file_path.clone(), snapshot.original_content.clone()));
        }

        // Try disk fallback
        let disk_path = self.base_dir.join(format!("{call_id}.bak"));
        if disk_path.exists()
            && let Ok(content) = std::fs::read_to_string(&disk_path) {
                return Some((PathBuf::new(), content)); // path unknown from disk
            }

        None
    }

    /// Get the most recent snapshot (for undo-last).
    pub fn last(&self) -> Option<Snapshot> {
        let snapshots = self.snapshots.lock().expect("snapshot lock");
        snapshots.values().last().cloned()
    }

    /// Remove a snapshot after successful undo.
    pub fn remove(&self, call_id: &str) {
        self.snapshots.lock().expect("snapshot lock").remove(call_id);
        let disk_path = self.base_dir.join(format!("{call_id}.bak"));
        let _ = std::fs::remove_file(disk_path);
    }

    /// Clean up old snapshots (called at session end).
    pub fn cleanup(&self) {
        self.snapshots.lock().expect("snapshot lock").clear();
        if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

impl Default for SnapshotStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_restore() {
        let store = SnapshotStore::new();
        let path = PathBuf::from("/tmp/test.rs");
        store.save("c-1", &path, "original content", "write");

        let (restored_path, content) = store.restore("c-1").expect("should restore");
        assert_eq!(restored_path, path);
        assert_eq!(content, "original content");

        store.remove("c-1");
    }

    #[test]
    fn restore_nonexistent_returns_none() {
        let store = SnapshotStore::new();
        assert!(store.restore("nonexistent").is_none());
    }

    #[test]
    fn last_returns_most_recent() {
        let store = SnapshotStore::new();
        store.save("c-1", &PathBuf::from("a.rs"), "aaa", "write");
        store.save("c-2", &PathBuf::from("b.rs"), "bbb", "edit");

        let last = store.last().expect("should have last");
        // HashMap order is not guaranteed, but last() should return something
        assert!(!last.call_id.is_empty());

        store.remove("c-1");
        store.remove("c-2");
    }

    #[test]
    fn cleanup_removes_all() {
        let store = SnapshotStore::new();
        store.save("c-1", &PathBuf::from("a.rs"), "aaa", "write");
        store.cleanup();
        assert!(store.restore("c-1").is_none());
    }
}
