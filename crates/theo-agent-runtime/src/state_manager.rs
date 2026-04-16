//! State manager — orchestrates file-backed state persistence via SessionTree.
//!
//! Provides a unified interface for persisting and loading agent session state
//! that survives compaction, truncation, and restart. State is externalized
//! to `.theo/state/{run_id}/session.jsonl` using append-only JSONL.
//!
//! Also handles loading EpisodeSummary records from previous runs for
//! cross-session context (resume/continue flows).

use std::path::{Path, PathBuf};

use crate::session_tree::{SessionEntry, SessionTree, SessionTreeError};

/// Orchestrates file-backed state persistence.
///
/// Wraps `SessionTree` and provides a higher-level interface for the agent
/// runtime to persist conversation state incrementally.
pub struct StateManager {
    session_tree: SessionTree,
    state_dir: PathBuf,
}

impl StateManager {
    /// Create a new state manager, initializing a fresh SessionTree on disk.
    ///
    /// Creates the directory `.theo/state/{run_id}/` if it doesn't exist.
    pub fn create(project_dir: &Path, run_id: &str) -> Result<Self, SessionTreeError> {
        let state_dir = project_dir.join(".theo").join("state").join(run_id);
        std::fs::create_dir_all(&state_dir)?;

        let session_path = state_dir.join("session.jsonl");
        let cwd = project_dir.display().to_string();
        let session_tree = SessionTree::create(&session_path, &cwd)?;

        Ok(Self {
            session_tree,
            state_dir,
        })
    }

    /// Load an existing state manager from a previous run.
    ///
    /// Returns `None` if the session file doesn't exist (no prior state).
    pub fn load(project_dir: &Path, run_id: &str) -> Result<Option<Self>, SessionTreeError> {
        let state_dir = project_dir.join(".theo").join("state").join(run_id);
        let session_path = state_dir.join("session.jsonl");

        if !session_path.exists() {
            return Ok(None);
        }

        let session_tree = SessionTree::load(&session_path)?;
        Ok(Some(Self {
            session_tree,
            state_dir,
        }))
    }

    /// Append a message to the session tree (persisted immediately to disk).
    pub fn append_message(&mut self, role: &str, content: &str) -> Result<(), SessionTreeError> {
        self.session_tree.append_message(role, content)?;
        Ok(())
    }

    /// Build conversation context from the session tree.
    ///
    /// Returns `(role, content)` pairs in root-to-leaf order, suitable for
    /// reconstructing the message history.
    pub fn build_context(&self) -> Vec<(String, String)> {
        self.session_tree
            .build_context()
            .into_iter()
            .filter_map(|entry| match entry {
                SessionEntry::Message { role, content, .. } => {
                    Some((role.clone(), content.clone()))
                }
                SessionEntry::Compaction { summary, .. } => {
                    Some(("system".to_string(), summary.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Number of entries in the session tree (including header).
    pub fn len(&self) -> usize {
        self.session_tree.len()
    }

    /// Whether the session has any non-header entries.
    pub fn is_empty(&self) -> bool {
        self.session_tree.is_empty()
    }

    /// Path to the state directory.
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// Load episode summaries from previous runs.
    ///
    /// Scans `.theo/wiki/episodes/` for JSON files and deserializes them.
    /// Returns an empty vec if no episodes exist or on any error.
    pub fn load_episode_summaries(
        project_dir: &Path,
    ) -> Vec<theo_domain::episode::EpisodeSummary> {
        let episodes_dir = project_dir.join(".theo").join("wiki").join("episodes");
        if !episodes_dir.exists() {
            return Vec::new();
        }

        let mut summaries = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&episodes_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(summary) =
                            serde_json::from_str::<theo_domain::episode::EpisodeSummary>(&content)
                        {
                            summaries.push(summary);
                        }
                    }
                }
            }
        }

        // Sort by created_at (oldest first) for chronological context.
        summaries.sort_by_key(|s| s.created_at);
        summaries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_manager_creates_session_tree_on_disk() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();

        // Act
        let sm = StateManager::create(dir.path(), "run-001").unwrap();

        // Assert
        let session_path = dir.path().join(".theo/state/run-001/session.jsonl");
        assert!(session_path.exists());
        assert_eq!(sm.len(), 1); // header only
        assert!(sm.is_empty());
    }

    #[test]
    fn test_state_manager_append_and_reload() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        {
            let mut sm = StateManager::create(dir.path(), "run-002").unwrap();
            sm.append_message("user", "Hello").unwrap();
            sm.append_message("assistant", "Hi there!").unwrap();
            sm.append_message("user", "How are you?").unwrap();
            assert_eq!(sm.len(), 4); // header + 3 messages
        }

        // Act: reload from disk
        let loaded = StateManager::load(dir.path(), "run-002")
            .unwrap()
            .expect("should find existing session");

        // Assert
        assert_eq!(loaded.len(), 4);
        assert!(!loaded.is_empty());
    }

    #[test]
    fn test_state_manager_build_context_converts_to_pairs() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let mut sm = StateManager::create(dir.path(), "run-003").unwrap();
        sm.append_message("user", "question").unwrap();
        sm.append_message("assistant", "answer").unwrap();

        // Act
        let context = sm.build_context();

        // Assert
        assert_eq!(context.len(), 2);
        assert_eq!(context[0], ("user".to_string(), "question".to_string()));
        assert_eq!(context[1], ("assistant".to_string(), "answer".to_string()));
    }

    #[test]
    fn test_state_manager_load_nonexistent_returns_none() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();

        // Act
        let result = StateManager::load(dir.path(), "nonexistent-run").unwrap();

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn test_episode_summaries_loadable_for_resume() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let episodes_dir = dir.path().join(".theo/wiki/episodes");
        std::fs::create_dir_all(&episodes_dir).unwrap();

        // Write a minimal episode summary JSON
        let episode_json = serde_json::json!({
            "summary_id": "ep-test-001",
            "run_id": "run-001",
            "task_id": null,
            "window_start_event_id": "evt-1",
            "window_end_event_id": "evt-5",
            "machine_summary": {
                "objective": "Fix login bug",
                "key_actions": ["read auth.rs", "edit verify_token"],
                "outcome": "Success",
                "successful_steps": ["identified root cause"],
                "failed_attempts": [],
                "learned_constraints": ["no unwrap in auth"],
                "files_touched": ["src/auth.rs"]
            },
            "human_summary": null,
            "evidence_event_ids": ["evt-1", "evt-2"],
            "affected_files": ["src/auth.rs"],
            "open_questions": [],
            "unresolved_hypotheses": [],
            "referenced_community_ids": [],
            "supersedes_summary_id": null,
            "schema_version": 1,
            "created_at": 1700000000000_u64,
            "ttl_policy": "Permanent",
            "lifecycle": "Active"
        });
        std::fs::write(
            episodes_dir.join("ep-test-001.json"),
            serde_json::to_string_pretty(&episode_json).unwrap(),
        )
        .unwrap();

        // Act
        let summaries = StateManager::load_episode_summaries(dir.path());

        // Assert
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].summary_id, "ep-test-001");
        assert_eq!(summaries[0].run_id, "run-001");
        assert_eq!(summaries[0].machine_summary.objective, "Fix login bug");
    }

    #[test]
    fn test_episode_summaries_empty_when_no_episodes() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();

        // Act
        let summaries = StateManager::load_episode_summaries(dir.path());

        // Assert
        assert!(summaries.is_empty());
    }

    #[test]
    fn test_state_manager_state_dir_path() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let sm = StateManager::create(dir.path(), "run-004").unwrap();

        // Assert
        let expected = dir.path().join(".theo/state/run-004");
        assert_eq!(sm.state_dir(), expected);
    }
}
