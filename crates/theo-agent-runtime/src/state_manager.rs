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
    /// Primary path: `.theo/memory/episodes/` (decision: meeting 20260420-221947 #4).
    /// Legacy fallback: `.theo/wiki/episodes/` (earlier location). Both are scanned
    /// and merged; the memory path wins on duplicate `summary_id`. Returns an
    /// empty vec if no episodes exist or on any error.
    pub fn load_episode_summaries(
        project_dir: &Path,
    ) -> Vec<theo_domain::episode::EpisodeSummary> {
        let memory_dir = project_dir.join(".theo").join("memory").join("episodes");
        let legacy_dir = project_dir.join(".theo").join("wiki").join("episodes");

        let mut seen: std::collections::HashMap<String, theo_domain::episode::EpisodeSummary> =
            std::collections::HashMap::new();

        // Scan legacy first, then memory — memory entries overwrite duplicates.
        for dir in [&legacy_dir, &memory_dir] {
            if !dir.exists() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(&path)
                        && let Ok(summary) =
                            serde_json::from_str::<theo_domain::episode::EpisodeSummary>(&content)
                        {
                            seen.insert(summary.summary_id.clone(), summary);
                        }
                }
            }
        }

        let mut summaries: Vec<_> = seen.into_values().collect();
        // Sort by created_at (oldest first) for chronological context.
        summaries.sort_by_key(|s| s.created_at);
        summaries
    }

    /// Persist a single episode summary back to disk.
    ///
    /// Used by the promotion and hit-tracking paths to update episodes
    /// after lifecycle transitions or context assembly hits.
    pub fn save_episode_summary(
        project_dir: &Path,
        summary: &theo_domain::episode::EpisodeSummary,
    ) -> std::io::Result<()> {
        let dir = project_dir.join(".theo").join("memory").join("episodes");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", summary.summary_id));
        let json = serde_json::to_string_pretty(summary)
            .map_err(std::io::Error::other)?;
        std::fs::write(&path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_manager_creates_session_tree_on_disk() {
        // Arrange
        let dir = tempfile::tempdir().expect("t");

        // Act
        let sm = StateManager::create(dir.path(), "run-001").expect("t");

        // Assert
        let session_path = dir.path().join(".theo/state/run-001/session.jsonl");
        assert!(session_path.exists());
        assert_eq!(sm.len(), 1); // header only
        assert!(sm.is_empty());
    }

    #[test]
    fn test_state_manager_append_and_reload() {
        // Arrange
        let dir = tempfile::tempdir().expect("t");
        {
            let mut sm = StateManager::create(dir.path(), "run-002").expect("t");
            sm.append_message("user", "Hello").expect("t");
            sm.append_message("assistant", "Hi there!").expect("t");
            sm.append_message("user", "How are you?").expect("t");
            assert_eq!(sm.len(), 4); // header + 3 messages
        }

        // Act: reload from disk
        let loaded = StateManager::load(dir.path(), "run-002")
            .expect("t")
            .expect("should find existing session");

        // Assert
        assert_eq!(loaded.len(), 4);
        assert!(!loaded.is_empty());
    }

    #[test]
    fn test_state_manager_build_context_converts_to_pairs() {
        // Arrange
        let dir = tempfile::tempdir().expect("t");
        let mut sm = StateManager::create(dir.path(), "run-003").expect("t");
        sm.append_message("user", "question").expect("t");
        sm.append_message("assistant", "answer").expect("t");

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
        let dir = tempfile::tempdir().expect("t");

        // Act
        let result = StateManager::load(dir.path(), "nonexistent-run").expect("t");

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn test_episode_summaries_loadable_for_resume() {
        // Arrange
        let dir = tempfile::tempdir().expect("t");
        let episodes_dir = dir.path().join(".theo/memory/episodes");
        std::fs::create_dir_all(&episodes_dir).expect("t");

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
            serde_json::to_string_pretty(&episode_json).expect("t"),
        )
        .expect("t");

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
        let dir = tempfile::tempdir().expect("t");

        // Act
        let summaries = StateManager::load_episode_summaries(dir.path());

        // Assert
        assert!(summaries.is_empty());
    }

    #[test]
    fn test_p1_legacy_wiki_episodes_still_readable() {
        // Legacy path `.theo/wiki/episodes/` must keep loading so that
        // users upgrading across this change do not lose episode history
        // (decision: meeting 20260420-221947 #4).
        let dir = tempfile::tempdir().expect("t");
        let legacy_dir = dir.path().join(".theo/wiki/episodes");
        std::fs::create_dir_all(&legacy_dir).expect("t");
        let payload = serde_json::json!({
            "summary_id": "ep-legacy-1",
            "run_id": "run-L",
            "task_id": null,
            "window_start_event_id": "",
            "window_end_event_id": "",
            "machine_summary": {
                "objective": "legacy", "key_actions": [], "outcome": "Success",
                "successful_steps": [], "failed_attempts": [],
                "learned_constraints": [], "files_touched": []
            },
            "human_summary": null,
            "evidence_event_ids": [],
            "affected_files": [],
            "open_questions": [],
            "unresolved_hypotheses": [],
            "referenced_community_ids": [],
            "supersedes_summary_id": null,
            "schema_version": 1,
            "created_at": 1u64,
            "ttl_policy": "RunScoped",
            "lifecycle": "Active"
        });
        std::fs::write(
            legacy_dir.join("ep-legacy-1.json"),
            serde_json::to_string(&payload).expect("t"),
        )
        .expect("t");

        let summaries = StateManager::load_episode_summaries(dir.path());

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].summary_id, "ep-legacy-1");
    }

    #[test]
    fn test_p1_memory_path_wins_over_legacy_on_duplicate_id() {
        // Same summary_id present in both paths → the `.theo/memory/episodes/`
        // version is authoritative.
        let dir = tempfile::tempdir().expect("t");
        let legacy = dir.path().join(".theo/wiki/episodes");
        let memory = dir.path().join(".theo/memory/episodes");
        std::fs::create_dir_all(&legacy).expect("t");
        std::fs::create_dir_all(&memory).expect("t");
        let make_payload = |objective: &str| {
            serde_json::json!({
                "summary_id": "ep-dup",
                "run_id": "run-X",
                "task_id": null,
                "window_start_event_id": "",
                "window_end_event_id": "",
                "machine_summary": {
                    "objective": objective, "key_actions": [], "outcome": "Success",
                    "successful_steps": [], "failed_attempts": [],
                    "learned_constraints": [], "files_touched": []
                },
                "human_summary": null,
                "evidence_event_ids": [],
                "affected_files": [],
                "open_questions": [],
                "unresolved_hypotheses": [],
                "referenced_community_ids": [],
                "supersedes_summary_id": null,
                "schema_version": 1,
                "created_at": 1u64,
                "ttl_policy": "RunScoped",
                "lifecycle": "Active"
            })
        };
        std::fs::write(
            legacy.join("ep-dup.json"),
            serde_json::to_string(&make_payload("legacy")).expect("t"),
        )
        .expect("t");
        std::fs::write(
            memory.join("ep-dup.json"),
            serde_json::to_string(&make_payload("memory")).expect("t"),
        )
        .expect("t");

        let summaries = StateManager::load_episode_summaries(dir.path());

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].machine_summary.objective, "memory");
    }

    #[test]
    fn test_state_manager_state_dir_path() {
        // Arrange
        let dir = tempfile::tempdir().expect("t");
        let sm = StateManager::create(dir.path(), "run-004").expect("t");

        // Assert
        let expected = dir.path().join(".theo/state/run-004");
        assert_eq!(sm.state_dir(), expected);
    }
}
