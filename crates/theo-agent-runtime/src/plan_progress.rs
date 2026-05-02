//! Runtime-only persistence for plan progress, errors, and reboot checks
//! (Manus principles: never repeat failures, 5-question reboot test).
//!
//! Layout: `<project>/.theo/plans/progress.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Format version for `progress.json`. Bump on incompatible changes.
pub const PLAN_PROGRESS_VERSION: u32 = 1;

/// Aggregated progress file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanProgress {
    pub version: u32,
    #[serde(default)]
    pub sessions: Vec<PlanSession>,
    #[serde(default)]
    pub errors: Vec<PlanErrorEntry>,
    #[serde(default)]
    pub reboot_check: RebootCheck,
}

impl Default for PlanProgress {
    fn default() -> Self {
        Self {
            version: PLAN_PROGRESS_VERSION,
            sessions: Vec::new(),
            errors: Vec::new(),
            reboot_check: RebootCheck::default(),
        }
    }
}

/// One execution session. Captures what happened during a single agent run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanSession {
    pub started_at: u64,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub files_modified: Vec<String>,
}

/// Recorded failure, with the attempt count so retries don't repeat the
/// same mistake.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanErrorEntry {
    pub error: String,
    pub attempt: u32,
    pub resolution: String,
    pub timestamp: u64,
}

/// Implements the Manus "5-question reboot test": before resuming work,
/// the agent must answer these five questions to demonstrate context.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RebootCheck {
    #[serde(default)]
    pub where_am_i: String,
    #[serde(default)]
    pub where_going: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub learned: String,
    #[serde(default)]
    pub done: String,
}

impl RebootCheck {
    /// Returns `true` when every field has been filled in (non-whitespace).
    pub fn is_complete(&self) -> bool {
        !self.where_am_i.trim().is_empty()
            && !self.where_going.trim().is_empty()
            && !self.goal.trim().is_empty()
            && !self.learned.trim().is_empty()
            && !self.done.trim().is_empty()
    }
}

/// Errors specific to progress I/O.
#[derive(Debug, thiserror::Error)]
pub enum PlanProgressError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid progress format: {0}")]
    InvalidFormat(String),
    #[error("unsupported progress version: found {found}, max supported {max_supported}")]
    UnsupportedVersion { found: u32, max_supported: u32 },
}

/// Loads progress from disk; returns `Default` when the file is missing.
pub fn load_progress(path: &Path) -> Result<PlanProgress, PlanProgressError> {
    if !path.exists() {
        return Ok(PlanProgress::default());
    }
    let content = std::fs::read_to_string(path)?;
    let progress: PlanProgress = serde_json::from_str(&content)
        .map_err(|e| PlanProgressError::InvalidFormat(e.to_string()))?;
    if progress.version > PLAN_PROGRESS_VERSION {
        return Err(PlanProgressError::UnsupportedVersion {
            found: progress.version,
            max_supported: PLAN_PROGRESS_VERSION,
        });
    }
    Ok(progress)
}

/// Saves progress atomically (write temp + rename).
pub fn save_progress(path: &Path, progress: &PlanProgress) -> Result<(), PlanProgressError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(progress)
        .map_err(|e| PlanProgressError::InvalidFormat(e.to_string()))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json.as_bytes())?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Appends a new error to the log, automatically incrementing `attempt`
/// for repeated occurrences (matched by `error` text).
pub fn append_error(progress: &mut PlanProgress, error: String, resolution: String, timestamp: u64) {
    let attempt = progress
        .errors
        .iter()
        .filter(|e| e.error == error)
        .count()
        .saturating_add(1) as u32;
    progress.errors.push(PlanErrorEntry {
        error,
        attempt,
        resolution,
        timestamp,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn progress_default_has_current_version() {
        let p = PlanProgress::default();
        assert_eq!(p.version, PLAN_PROGRESS_VERSION);
        assert!(p.sessions.is_empty());
        assert!(p.errors.is_empty());
    }

    #[test]
    fn progress_serde_roundtrip() {
        let p = PlanProgress {
            version: PLAN_PROGRESS_VERSION,
            sessions: vec![PlanSession {
                started_at: 1,
                actions: vec!["read main.rs".into()],
                files_modified: vec!["src/main.rs".into()],
            }],
            errors: vec![PlanErrorEntry {
                error: "compile fail".into(),
                attempt: 1,
                resolution: "Fixed import".into(),
                timestamp: 2,
            }],
            reboot_check: RebootCheck {
                where_am_i: "Phase 2".into(),
                where_going: "Phase 3".into(),
                goal: "Ship plan system".into(),
                learned: "Always validate".into(),
                done: "Wrote tests".into(),
            },
        };
        let json = serde_json::to_string_pretty(&p).unwrap();
        let back: PlanProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn reboot_check_complete_returns_true_when_all_fields_set() {
        let rc = RebootCheck {
            where_am_i: "x".into(),
            where_going: "y".into(),
            goal: "z".into(),
            learned: "a".into(),
            done: "b".into(),
        };
        assert!(rc.is_complete());
    }

    #[test]
    fn reboot_check_complete_returns_false_when_any_field_blank() {
        let rc = RebootCheck {
            where_am_i: "x".into(),
            where_going: "".into(),
            goal: "z".into(),
            learned: "a".into(),
            done: "b".into(),
        };
        assert!(!rc.is_complete());
    }

    #[test]
    fn progress_load_returns_default_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.json");
        let p = load_progress(&path).unwrap();
        assert_eq!(p, PlanProgress::default());
    }

    #[test]
    fn progress_save_load_roundtrip_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("progress.json");
        let p = PlanProgress::default();
        save_progress(&path, &p).unwrap();
        let loaded = load_progress(&path).unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn progress_load_rejects_future_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("progress.json");
        let p = PlanProgress {
            version: 999,
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&p).unwrap()).unwrap();
        let err = load_progress(&path).unwrap_err();
        assert!(matches!(err, PlanProgressError::UnsupportedVersion { .. }));
    }

    #[test]
    fn append_error_increments_attempt_for_repeats() {
        let mut p = PlanProgress::default();
        append_error(&mut p, "oops".into(), "retry".into(), 1);
        append_error(&mut p, "oops".into(), "retry again".into(), 2);
        append_error(&mut p, "different".into(), "fix".into(), 3);
        assert_eq!(p.errors[0].attempt, 1);
        assert_eq!(p.errors[1].attempt, 2);
        assert_eq!(p.errors[2].attempt, 1);
    }

    #[test]
    fn progress_optional_fields_default() {
        let json = r#"{"version": 1}"#;
        let p: PlanProgress = serde_json::from_str(json).unwrap();
        assert!(p.sessions.is_empty());
        assert!(p.errors.is_empty());
        assert_eq!(p.reboot_check, RebootCheck::default());
    }
}
