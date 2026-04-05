//! Failure pattern tracker — counts error types and suggests harness improvements.
//!
//! Implements the "steering loop" from Harness Engineering research:
//! when an error pattern occurs N times, suggest adding a guide or sensor.
//!
//! Persists to `.theo/failure_patterns.json` for cross-session tracking.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File location relative to project root.
const PATTERNS_FILE: &str = ".theo/failure_patterns.json";

/// Threshold: after this many occurrences, suggest a rule.
const SUGGEST_THRESHOLD: u32 = 3;

/// A tracked failure pattern.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PatternEntry {
    /// Number of times this pattern was observed.
    pub count: u32,
    /// Last occurrence timestamp (Unix seconds).
    pub last_seen: u64,
    /// Whether a suggestion was already emitted for this pattern.
    pub suggestion_emitted: bool,
}

/// Persisted failure patterns across sessions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FailurePatterns {
    pub patterns: HashMap<String, PatternEntry>,
}

/// In-memory failure tracker with optional persistence.
#[derive(Debug)]
pub struct FailurePatternTracker {
    data: FailurePatterns,
    project_dir: PathBuf,
}

impl FailurePatternTracker {
    /// Create a new tracker, loading existing patterns from disk if available.
    pub fn new(project_dir: &Path) -> Self {
        let data = load_patterns(project_dir).unwrap_or_default();
        Self {
            data,
            project_dir: project_dir.to_path_buf(),
        }
    }

    /// Record a failure occurrence.
    pub fn record(&mut self, pattern_name: &str) {
        let entry = self.data.patterns.entry(pattern_name.to_string()).or_default();
        entry.count += 1;
        entry.last_seen = unix_now();
    }

    /// Check if a pattern has exceeded the suggestion threshold.
    /// Returns a suggestion message if threshold exceeded and not yet emitted.
    pub fn check_suggestion(&mut self, pattern_name: &str) -> Option<String> {
        let entry = self.data.patterns.get_mut(pattern_name)?;
        if entry.count >= SUGGEST_THRESHOLD && !entry.suggestion_emitted {
            entry.suggestion_emitted = true;
            Some(format!(
                "Pattern '{}' has occurred {} times. Consider adding a harness rule or sensor to prevent it.",
                pattern_name, entry.count
            ))
        } else {
            None
        }
    }

    /// Record a failure and immediately check for suggestion.
    pub fn record_and_check(&mut self, pattern_name: &str) -> Option<String> {
        self.record(pattern_name);
        self.check_suggestion(pattern_name)
    }

    /// Get the count for a specific pattern.
    pub fn count(&self, pattern_name: &str) -> u32 {
        self.data.patterns.get(pattern_name).map(|e| e.count).unwrap_or(0)
    }

    /// Get all patterns that have exceeded the threshold.
    pub fn hot_patterns(&self) -> Vec<(&str, u32)> {
        self.data
            .patterns
            .iter()
            .filter(|(_, e)| e.count >= SUGGEST_THRESHOLD)
            .map(|(name, e)| (name.as_str(), e.count))
            .collect()
    }

    /// Persist current state to disk. Best-effort, never fails.
    pub fn save(&self) {
        save_patterns(&self.project_dir, &self.data);
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn patterns_path(project_dir: &Path) -> PathBuf {
    project_dir.join(PATTERNS_FILE)
}

fn load_patterns(project_dir: &Path) -> Option<FailurePatterns> {
    let content = std::fs::read_to_string(patterns_path(project_dir)).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_patterns(project_dir: &Path, data: &FailurePatterns) {
    let path = patterns_path(project_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn new_tracker_starts_empty() {
        let dir = temp_project();
        let tracker = FailurePatternTracker::new(dir.path());
        assert_eq!(tracker.count("compilation_error"), 0);
        assert!(tracker.hot_patterns().is_empty());
    }

    #[test]
    fn record_increments_count() {
        let dir = temp_project();
        let mut tracker = FailurePatternTracker::new(dir.path());
        tracker.record("compilation_error");
        tracker.record("compilation_error");
        assert_eq!(tracker.count("compilation_error"), 2);
    }

    #[test]
    fn suggestion_at_threshold() {
        let dir = temp_project();
        let mut tracker = FailurePatternTracker::new(dir.path());

        // Below threshold: no suggestion
        assert!(tracker.record_and_check("test_failure").is_none());
        assert!(tracker.record_and_check("test_failure").is_none());

        // At threshold: suggestion emitted
        let suggestion = tracker.record_and_check("test_failure");
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("3 times"));

        // After suggestion: no duplicate
        assert!(tracker.record_and_check("test_failure").is_none());
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = temp_project();

        // Session 1: record patterns
        {
            let mut tracker = FailurePatternTracker::new(dir.path());
            tracker.record("compilation_error");
            tracker.record("compilation_error");
            tracker.record("test_failure");
            tracker.save();
        }

        // Session 2: patterns survived
        {
            let tracker = FailurePatternTracker::new(dir.path());
            assert_eq!(tracker.count("compilation_error"), 2);
            assert_eq!(tracker.count("test_failure"), 1);
        }
    }

    #[test]
    fn hot_patterns_returns_only_above_threshold() {
        let dir = temp_project();
        let mut tracker = FailurePatternTracker::new(dir.path());

        for _ in 0..5 {
            tracker.record("hot_pattern");
        }
        tracker.record("cold_pattern");

        let hot = tracker.hot_patterns();
        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0].0, "hot_pattern");
        assert_eq!(hot[0].1, 5);
    }

    #[test]
    fn suggestion_emitted_flag_persists() {
        let dir = temp_project();

        // Session 1: trigger suggestion
        {
            let mut tracker = FailurePatternTracker::new(dir.path());
            tracker.record("error_x");
            tracker.record("error_x");
            let s = tracker.record_and_check("error_x");
            assert!(s.is_some());
            tracker.save();
        }

        // Session 2: suggestion already emitted
        {
            let mut tracker = FailurePatternTracker::new(dir.path());
            assert!(tracker.check_suggestion("error_x").is_none());
        }
    }
}
