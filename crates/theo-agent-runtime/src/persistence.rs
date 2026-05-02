//! Persistence layer for run snapshots.
//!
//! Reserved-for-future-use snapshot store; the trait is implemented but no
//! runtime caller wires it yet.
#![allow(dead_code)]

use std::path::PathBuf;

use async_trait::async_trait;

use theo_domain::identifiers::RunId;

use crate::snapshot::RunSnapshot;

/// Errors that can occur during snapshot persistence.
#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("snapshot not found for run {0}")]
    NotFound(String),
}

/// Trait for persisting and loading agent run snapshots.
#[async_trait]
pub trait SnapshotStore: Send + Sync {
    async fn save(&self, run_id: &RunId, snapshot: &RunSnapshot) -> Result<(), PersistenceError>;
    async fn load(&self, run_id: &RunId) -> Result<Option<RunSnapshot>, PersistenceError>;
    async fn list_runs(&self) -> Result<Vec<RunId>, PersistenceError>;
    async fn delete(&self, run_id: &RunId) -> Result<(), PersistenceError>;
}

/// File-based snapshot store. Serializes snapshots as JSON files.
///
/// Directory structure: `{base_dir}/{run_id}.json`
pub struct FileSnapshotStore {
    base_dir: PathBuf,
}

impl FileSnapshotStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    fn snapshot_path(&self, run_id: &RunId) -> PathBuf {
        self.base_dir.join(format!("{}.json", run_id.as_str()))
    }
}

#[async_trait]
impl SnapshotStore for FileSnapshotStore {
    async fn save(&self, run_id: &RunId, snapshot: &RunSnapshot) -> Result<(), PersistenceError> {
        // Create directory if it doesn't exist
        tokio::fs::create_dir_all(&self.base_dir).await?;

        let json = serde_json::to_string_pretty(snapshot)
            .map_err(|e| PersistenceError::Serialization(e.to_string()))?;

        let path = self.snapshot_path(run_id);
        tokio::fs::write(&path, json).await?;
        Ok(())
    }

    async fn load(&self, run_id: &RunId) -> Result<Option<RunSnapshot>, PersistenceError> {
        let path = self.snapshot_path(run_id);

        if !path.exists() {
            return Ok(None);
        }

        let json = tokio::fs::read_to_string(&path).await?;
        let snapshot: RunSnapshot = serde_json::from_str(&json)
            .map_err(|e| PersistenceError::Serialization(e.to_string()))?;

        // Validate checksum (Invariant 7: consistent snapshot)
        let computed = snapshot.compute_checksum();
        if snapshot.checksum != computed {
            return Err(PersistenceError::ChecksumMismatch {
                expected: snapshot.checksum.clone(),
                actual: computed,
            });
        }

        Ok(Some(snapshot))
    }

    async fn list_runs(&self) -> Result<Vec<RunId>, PersistenceError> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let mut runs = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.base_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    runs.push(RunId::new(stem));
                }
        }

        Ok(runs)
    }

    async fn delete(&self, run_id: &RunId) -> Result<(), PersistenceError> {
        let path = self.snapshot_path(run_id);
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::agent_run::{AgentRun, RunState};
    use theo_domain::budget::BudgetUsage;
    use theo_domain::identifiers::TaskId;
    use theo_domain::session::SessionId;
    use theo_domain::task::{AgentType, Task, TaskState};

    fn make_snapshot(run_id: &str) -> RunSnapshot {
        let run = AgentRun {
            run_id: RunId::new(run_id),
            task_id: TaskId::new("t-1"),
            state: RunState::Executing,
            iteration: 5,
            max_iterations: 30,
            created_at: 1000,
            updated_at: 2000,
        };
        let task = Task {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            state: TaskState::Running,
            agent_type: AgentType::Coder,
            objective: "test".into(),
            artifacts: vec![],
            created_at: 1000,
            updated_at: 2000,
            completed_at: None,
        };
        RunSnapshot::new(
            run,
            task,
            vec![],
            vec![],
            vec![],
            BudgetUsage::default(),
            vec![],
            vec![],
        )
    }

    #[tokio::test]
    async fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSnapshotStore::new(dir.path());
        let run_id = RunId::new("test-run-1");
        let snapshot = make_snapshot("test-run-1");

        store.save(&run_id, &snapshot).await.unwrap();
        let loaded = store
            .load(&run_id)
            .await
            .unwrap()
            .expect("should find snapshot");

        assert_eq!(loaded.run.run_id, snapshot.run.run_id);
        assert_eq!(loaded.task.objective, "test");
        assert_eq!(loaded.checksum, snapshot.checksum);
        assert!(loaded.validate_checksum());
    }

    #[tokio::test]
    async fn load_detects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSnapshotStore::new(dir.path());
        let run_id = RunId::new("corrupt-run");
        let snapshot = make_snapshot("corrupt-run");

        store.save(&run_id, &snapshot).await.unwrap();

        // Corrupt the file by modifying a field
        let path = dir.path().join("corrupt-run.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&tokio::fs::read_to_string(&path).await.unwrap()).unwrap();
        json["task"]["objective"] = serde_json::Value::String("TAMPERED".into());
        tokio::fs::write(&path, serde_json::to_string_pretty(&json).unwrap())
            .await
            .unwrap();

        let result = store.load(&run_id).await;
        assert!(matches!(
            result,
            Err(PersistenceError::ChecksumMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn load_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSnapshotStore::new(dir.path());
        let result = store.load(&RunId::new("nonexistent")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_runs_returns_all_saved() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSnapshotStore::new(dir.path());

        store
            .save(&RunId::new("run-a"), &make_snapshot("run-a"))
            .await
            .unwrap();
        store
            .save(&RunId::new("run-b"), &make_snapshot("run-b"))
            .await
            .unwrap();
        store
            .save(&RunId::new("run-c"), &make_snapshot("run-c"))
            .await
            .unwrap();

        let runs = store.list_runs().await.unwrap();
        assert_eq!(runs.len(), 3);
    }

    #[tokio::test]
    async fn delete_removes_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSnapshotStore::new(dir.path());
        let run_id = RunId::new("delete-me");

        store
            .save(&run_id, &make_snapshot("delete-me"))
            .await
            .unwrap();
        assert!(store.load(&run_id).await.unwrap().is_some());

        store.delete(&run_id).await.unwrap();
        assert!(store.load(&run_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("deep").join("nested").join("snapshots");
        let store = FileSnapshotStore::new(&nested);

        store
            .save(&RunId::new("r-1"), &make_snapshot("r-1"))
            .await
            .unwrap();
        assert!(nested.exists());
        assert!(store.load(&RunId::new("r-1")).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn multiple_snapshots_independent() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSnapshotStore::new(dir.path());

        store
            .save(&RunId::new("run-a"), &make_snapshot("run-a"))
            .await
            .unwrap();
        store
            .save(&RunId::new("run-b"), &make_snapshot("run-b"))
            .await
            .unwrap();

        let a = store.load(&RunId::new("run-a")).await.unwrap().unwrap();
        let b = store.load(&RunId::new("run-b")).await.unwrap().unwrap();

        assert_eq!(a.run.run_id.as_str(), "run-a");
        assert_eq!(b.run.run_id.as_str(), "run-b");
    }

    #[tokio::test]
    async fn list_runs_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSnapshotStore::new(dir.path());
        let runs = store.list_runs().await.unwrap();
        assert!(runs.is_empty());
    }

    #[test]
    fn persistence_error_display() {
        let err = PersistenceError::ChecksumMismatch {
            expected: "abc".into(),
            actual: "def".into(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("abc"));
        assert!(msg.contains("def"));
    }
}
