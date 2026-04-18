//! Session bootstrap — progress tracking across agent sessions.
//!
//! Implements the Anthropic "initializer agent" pattern:
//! - Detects first session (no progress file exists)
//! - Reads progress from previous sessions at boot
//! - Writes progress summary at session end
//!
//! The progress file lives at `.theo/progress.json` in the project root.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Progress file location relative to project root.
const PROGRESS_FILE: &str = ".theo/progress.json";

/// Maximum number of completed tasks to keep in history (prevent unbounded growth).
const MAX_TASK_HISTORY: usize = 50;

/// A single completed task record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletedTask {
    pub name: String,
    pub status: String,
    pub files_changed: Vec<String>,
}

/// Session progress — persisted as JSON between sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionProgress {
    /// ISO 8601 timestamp of when progress was last updated.
    pub updated_at: String,
    /// Session identifier.
    pub session_id: String,
    /// Tasks completed across all sessions.
    pub tasks_completed: Vec<CompletedTask>,
    /// Next steps suggested by the last session.
    pub next_steps: Vec<String>,
    /// Last error encountered (if any).
    pub last_error: Option<String>,
}

impl Default for SessionProgress {
    fn default() -> Self {
        Self {
            updated_at: chrono_now(),
            session_id: String::new(),
            tasks_completed: Vec::new(),
            next_steps: Vec::new(),
            last_error: None,
        }
    }
}

/// Get current timestamp in ISO 8601.
fn chrono_now() -> String {
    // Simple UTC timestamp without chrono dependency.
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}Z", duration.as_secs())
}

/// Returns the full path to the progress file.
pub fn progress_file_path(project_dir: &Path) -> PathBuf {
    project_dir.join(PROGRESS_FILE)
}

/// Check if this is the first session (no progress file exists).
pub fn is_first_session(project_dir: &Path) -> bool {
    !progress_file_path(project_dir).exists()
}

/// Load progress from previous sessions.
///
/// Returns `None` if file doesn't exist or is unreadable (best-effort).
pub fn load_progress(project_dir: &Path) -> Option<SessionProgress> {
    let path = progress_file_path(project_dir);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save progress to disk with atomic replace semantics.
///
/// This implementation does not take an advisory lock; it writes a temp file
/// and renames it into place on a best-effort basis. I/O errors are ignored so
/// session progress persistence never blocks the agent.
pub fn save_progress(project_dir: &Path, progress: &SessionProgress) {
    let path = progress_file_path(project_dir);

    // Ensure .theo/ directory exists.
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Serialize and write atomically (write to temp, then rename).
    let content = match serde_json::to_string_pretty(progress) {
        Ok(c) => c,
        Err(_) => return,
    };

    let tmp_path = path.with_extension("json.tmp");
    if std::fs::write(&tmp_path, &content).is_ok() {
        let _ = std::fs::rename(&tmp_path, &path);
    }
}

/// Build a boot message summarizing previous session progress.
///
/// Returns `None` if no previous progress exists.
pub fn boot_message(project_dir: &Path) -> Option<String> {
    let progress = load_progress(project_dir)?;

    let mut parts = Vec::new();
    parts.push(format!(
        "Previous session progress (updated: {}):",
        progress.updated_at
    ));

    if !progress.tasks_completed.is_empty() {
        let recent: Vec<&CompletedTask> = progress.tasks_completed.iter().rev().take(5).collect();
        parts.push("Recent completed tasks:".to_string());
        for task in recent {
            parts.push(format!(
                "  - {} [{}] (files: {})",
                task.name,
                task.status,
                task.files_changed.join(", ")
            ));
        }
    }

    if !progress.next_steps.is_empty() {
        parts.push("Suggested next steps:".to_string());
        for step in &progress.next_steps {
            parts.push(format!("  - {step}"));
        }
    }

    if let Some(ref err) = progress.last_error {
        parts.push(format!("Last error: {err}"));
    }

    Some(parts.join("\n"))
}

/// Update progress at session end.
pub fn record_session_end(
    project_dir: &Path,
    session_id: &str,
    tasks: Vec<CompletedTask>,
    next_steps: Vec<String>,
    last_error: Option<String>,
) {
    let mut progress = load_progress(project_dir).unwrap_or_default();

    progress.updated_at = chrono_now();
    progress.session_id = session_id.to_string();
    progress.tasks_completed.extend(tasks);
    progress.next_steps = next_steps;
    progress.last_error = last_error;

    // Trim history to prevent unbounded growth.
    if progress.tasks_completed.len() > MAX_TASK_HISTORY {
        let drain_count = progress.tasks_completed.len() - MAX_TASK_HISTORY;
        progress.tasks_completed.drain(..drain_count);
    }

    save_progress(project_dir, &progress);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn first_session_detected_on_empty_dir() {
        let dir = setup_temp_dir();
        assert!(is_first_session(dir.path()));
    }

    #[test]
    fn first_session_false_after_save() {
        let dir = setup_temp_dir();
        let progress = SessionProgress::default();
        save_progress(dir.path(), &progress);
        assert!(!is_first_session(dir.path()));
    }

    #[test]
    fn roundtrip_serialize_deserialize() {
        let dir = setup_temp_dir();
        let progress = SessionProgress {
            updated_at: "1234567890Z".to_string(),
            session_id: "sess-001".to_string(),
            tasks_completed: vec![CompletedTask {
                name: "Fix login bug".to_string(),
                status: "completed".to_string(),
                files_changed: vec!["src/auth.rs".to_string()],
            }],
            next_steps: vec!["Add tests for auth module".to_string()],
            last_error: None,
        };

        save_progress(dir.path(), &progress);
        let loaded = load_progress(dir.path()).expect("Should load progress");

        assert_eq!(loaded.session_id, "sess-001");
        assert_eq!(loaded.tasks_completed.len(), 1);
        assert_eq!(loaded.tasks_completed[0].name, "Fix login bug");
        assert_eq!(loaded.next_steps, vec!["Add tests for auth module"]);
        assert!(loaded.last_error.is_none());
    }

    #[test]
    fn boot_message_none_on_empty() {
        let dir = setup_temp_dir();
        assert!(boot_message(dir.path()).is_none());
    }

    #[test]
    fn boot_message_contains_progress() {
        let dir = setup_temp_dir();
        let progress = SessionProgress {
            updated_at: "1234567890Z".to_string(),
            session_id: "sess-001".to_string(),
            tasks_completed: vec![CompletedTask {
                name: "Implement auth".to_string(),
                status: "completed".to_string(),
                files_changed: vec!["src/auth.rs".to_string()],
            }],
            next_steps: vec!["Write tests".to_string()],
            last_error: Some("compilation error".to_string()),
        };
        save_progress(dir.path(), &progress);

        let msg = boot_message(dir.path()).expect("Should have boot message");
        assert!(msg.contains("Implement auth"));
        assert!(msg.contains("Write tests"));
        assert!(msg.contains("compilation error"));
    }

    #[test]
    fn record_session_end_appends_tasks() {
        let dir = setup_temp_dir();
        record_session_end(
            dir.path(),
            "sess-001",
            vec![CompletedTask {
                name: "Task A".to_string(),
                status: "completed".to_string(),
                files_changed: vec![],
            }],
            vec!["Next: Task B".to_string()],
            None,
        );

        record_session_end(
            dir.path(),
            "sess-002",
            vec![CompletedTask {
                name: "Task B".to_string(),
                status: "completed".to_string(),
                files_changed: vec!["src/b.rs".to_string()],
            }],
            vec![],
            None,
        );

        let progress = load_progress(dir.path()).unwrap();
        assert_eq!(progress.tasks_completed.len(), 2);
        assert_eq!(progress.session_id, "sess-002");
    }

    #[test]
    fn history_trimmed_to_max() {
        let dir = setup_temp_dir();
        let tasks: Vec<CompletedTask> = (0..60)
            .map(|i| CompletedTask {
                name: format!("Task {i}"),
                status: "completed".to_string(),
                files_changed: vec![],
            })
            .collect();

        record_session_end(dir.path(), "sess", tasks, vec![], None);

        let progress = load_progress(dir.path()).unwrap();
        assert_eq!(progress.tasks_completed.len(), MAX_TASK_HISTORY);
        // Should keep the most recent
        assert_eq!(progress.tasks_completed.last().unwrap().name, "Task 59");
    }

    #[test]
    fn atomic_write_no_corruption() {
        let dir = setup_temp_dir();
        let progress = SessionProgress {
            updated_at: "test".to_string(),
            session_id: "atomic-test".to_string(),
            tasks_completed: vec![],
            next_steps: vec![],
            last_error: None,
        };
        save_progress(dir.path(), &progress);

        // The .tmp file should not exist after save.
        let tmp = progress_file_path(dir.path()).with_extension("json.tmp");
        assert!(!tmp.exists(), "Temp file should be cleaned up");

        // The actual file should exist and be valid JSON.
        let content = fs::read_to_string(progress_file_path(dir.path())).unwrap();
        let _: SessionProgress = serde_json::from_str(&content).expect("Should be valid JSON");
    }
}
