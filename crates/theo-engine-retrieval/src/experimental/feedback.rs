/// Context Miss Learning — tracks context precision/recall and learns from misses.
///
/// Records which files were provided, used, and missed per session, then uses
/// accumulated statistics to boost/penalise files for future queries.

use std::collections::{HashMap, HashSet};
use std::io;

use serde::{Deserialize, Serialize};

use crate::search::tokenise;

// ---------------------------------------------------------------------------
// Core type
// ---------------------------------------------------------------------------

/// Composite key for the count maps: `"<query_hash>:<file_path>"`.
///
/// Using a plain `String` key instead of a tuple so that serde_json can
/// serialize the HashMap directly (JSON object keys must be strings).
type CountKey = String;

fn make_key(hash: u64, file_path: &str) -> CountKey {
    format!("{hash}:{file_path}")
}

/// Tracks context effectiveness and learns from misses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFeedback {
    /// "query_hash:file_path" → number of times this file was a context miss
    miss_counts: HashMap<CountKey, usize>,
    /// "query_hash:file_path" → number of times this file was used when provided
    hit_counts: HashMap<CountKey, usize>,
    /// "query_hash:file_path" → number of times this file was provided but unused
    waste_counts: HashMap<CountKey, usize>,
    /// Total sessions tracked.
    total_sessions: usize,
}

impl ContextFeedback {
    pub fn new() -> Self {
        Self {
            miss_counts: HashMap::new(),
            hit_counts: HashMap::new(),
            waste_counts: HashMap::new(),
            total_sessions: 0,
        }
    }

    /// Record what happened in a session.
    ///
    /// * `query` — the original query text.
    /// * `provided_files` — files that were in the context window.
    /// * `used_files` — files the LLM actually referenced/edited.
    /// * `missed_files` — files the LLM asked for but were not in context.
    pub fn record_session(
        &mut self,
        query: &str,
        provided_files: &[String],
        used_files: &[String],
        missed_files: &[String],
    ) {
        let hash = Self::hash_query(query);
        let provided_set: HashSet<&str> = provided_files.iter().map(String::as_str).collect();
        let used_set: HashSet<&str> = used_files.iter().map(String::as_str).collect();

        // Hits: files that were provided AND used.
        for file in &used_set {
            if provided_set.contains(file) {
                *self
                    .hit_counts
                    .entry(make_key(hash, file))
                    .or_insert(0) += 1;
            }
        }

        // Waste: files that were provided but NOT used.
        for file in &provided_set {
            if !used_set.contains(file) {
                *self
                    .waste_counts
                    .entry(make_key(hash, file))
                    .or_insert(0) += 1;
            }
        }

        // Misses: files the LLM asked for but were absent.
        for file in missed_files {
            *self
                .miss_counts
                .entry(make_key(hash, file))
                .or_insert(0) += 1;
        }

        self.total_sessions += 1;
    }

    /// Get a boost score for a file given a query.
    ///
    /// Files that were frequently missed for similar queries get a positive
    /// boost. Files that were frequently wasted get a negative boost.
    ///
    /// Formula: `boost = miss * 0.3 - waste * 0.1 + hit * 0.05`, capped to
    /// \[-0.5, 0.5\].
    pub fn score_boost(&self, query: &str, file_path: &str) -> f64 {
        let hash = Self::hash_query(query);
        let key = make_key(hash, file_path);

        let miss = *self.miss_counts.get(&key).unwrap_or(&0) as f64;
        let waste = *self.waste_counts.get(&key).unwrap_or(&0) as f64;
        let hit = *self.hit_counts.get(&key).unwrap_or(&0) as f64;

        let raw = miss * 0.3 - waste * 0.1 + hit * 0.05;
        raw.clamp(-0.5, 0.5)
    }

    /// Overall precision: proportion of provided files that were actually used.
    ///
    /// `precision = total_hits / (total_hits + total_waste)`
    pub fn precision(&self) -> f64 {
        let total_hits: usize = self.hit_counts.values().sum();
        let total_waste: usize = self.waste_counts.values().sum();
        let denom = total_hits + total_waste;
        if denom == 0 {
            return 0.0;
        }
        total_hits as f64 / denom as f64
    }

    /// Overall recall: proportion of needed files that were actually provided.
    ///
    /// `recall = total_hits / (total_hits + total_misses)`
    pub fn recall(&self) -> f64 {
        let total_hits: usize = self.hit_counts.values().sum();
        let total_misses: usize = self.miss_counts.values().sum();
        let denom = total_hits + total_misses;
        if denom == 0 {
            return 0.0;
        }
        total_hits as f64 / denom as f64
    }

    /// Save feedback data to a JSON file.
    pub fn save(&self, path: &str) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Load feedback data from a JSON file.
    pub fn load(path: &str) -> io::Result<Self> {
        let data = std::fs::read_to_string(path)?;
        serde_json::from_str(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Simple query hashing for grouping similar queries.
    ///
    /// Tokenises the query, lowercases, sorts tokens, then computes a
    /// deterministic hash. This groups queries like "fix auth bug" and
    /// "bug fix auth" together.
    fn hash_query(query: &str) -> u64 {
        let mut tokens: Vec<String> = tokenise(query)
            .into_iter()
            .map(|t| t.to_lowercase())
            .collect();
        tokens.sort();
        let joined = tokens.join(" ");

        // Simple FNV-1a style hash.
        let mut h: u64 = 14695981039346656037;
        for byte in joined.as_bytes() {
            h ^= *byte as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_session_and_metrics() {
        // Arrange
        let mut fb = ContextFeedback::new();
        let provided = vec!["a.rs".into(), "b.rs".into(), "c.rs".into()];
        let used = vec!["a.rs".into(), "b.rs".into()];
        let missed = vec!["d.rs".into()];

        // Act
        fb.record_session("fix auth bug", &provided, &used, &missed);

        // Assert
        assert_eq!(fb.total_sessions, 1);
        // precision: 2 hits / (2 hits + 1 waste) = 2/3
        let prec = fb.precision();
        assert!((prec - 2.0 / 3.0).abs() < 1e-9, "precision={prec}");
        // recall: 2 hits / (2 hits + 1 miss) = 2/3
        let rec = fb.recall();
        assert!((rec - 2.0 / 3.0).abs() < 1e-9, "recall={rec}");
    }

    #[test]
    fn test_score_boost_positive_for_missed_files() {
        // Arrange
        let mut fb = ContextFeedback::new();

        // Record the same miss 3 times to build signal.
        for _ in 0..3 {
            fb.record_session("fix auth", &[], &[], &["auth.rs".into()]);
        }

        // Act
        let boost = fb.score_boost("fix auth", "auth.rs");

        // Assert — 3 misses * 0.3 = 0.9, capped to 0.5
        assert!((boost - 0.5).abs() < 1e-9, "boost={boost}");
    }

    #[test]
    fn test_score_boost_negative_for_wasted_files() {
        // Arrange
        let mut fb = ContextFeedback::new();

        // Provide a file that is never used — pure waste.
        for _ in 0..5 {
            fb.record_session("fix auth", &["noise.rs".into()], &[], &[]);
        }

        // Act
        let boost = fb.score_boost("fix auth", "noise.rs");

        // Assert — 5 waste * -0.1 = -0.5
        assert!((boost - (-0.5)).abs() < 1e-9, "boost={boost}");
    }

    #[test]
    fn test_hash_query_order_invariant() {
        // "fix auth bug" and "bug auth fix" should hash the same.
        let h1 = ContextFeedback::hash_query("fix auth bug");
        let h2 = ContextFeedback::hash_query("bug auth fix");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feedback.json");
        let path_str = path.to_str().unwrap();

        let mut fb = ContextFeedback::new();
        fb.record_session(
            "test query",
            &["a.rs".into()],
            &["a.rs".into()],
            &["b.rs".into()],
        );

        // Act
        fb.save(path_str).unwrap();
        let loaded = ContextFeedback::load(path_str).unwrap();

        // Assert
        assert_eq!(loaded.total_sessions, 1);
        assert!((loaded.precision() - fb.precision()).abs() < 1e-9);
        assert!((loaded.recall() - fb.recall()).abs() < 1e-9);
    }

    #[test]
    fn test_empty_feedback_returns_zero() {
        let fb = ContextFeedback::new();
        assert_eq!(fb.precision(), 0.0);
        assert_eq!(fb.recall(), 0.0);
        assert_eq!(fb.score_boost("anything", "any.rs"), 0.0);
    }
}
