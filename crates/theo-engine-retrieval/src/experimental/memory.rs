/// Contextual memory with exponential decay.
///
/// Tracks what context was sent in previous interactions so that new
/// interactions prioritise FRESH information. Files that were recently sent
/// get penalised (low staleness) while unseen files get boosted (high
/// staleness). Staleness grows via exponential decay controlled by
/// `decay_rate`.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// How much detail was sent for a given file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailLevel {
    /// The entire source code was included.
    FullCode,
    /// Only function/type signatures were included.
    Signatures,
    /// A compressed / summarised representation was included.
    Compressed,
}

/// One entry in the context memory.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MemoryEntry {
    /// Interaction number when this file was last sent.
    last_seen: usize,
    /// What level of detail was sent.
    detail_level: DetailLevel,
    /// Relevance score at the time of sending.
    original_score: f64,
}

// ---------------------------------------------------------------------------
// ContextMemory
// ---------------------------------------------------------------------------

/// Tracks what context has been sent across interactions within a session.
///
/// # Staleness model
///
/// A file that was **never sent** has staleness = 1.0 (maximally stale, should
/// be included). A file sent in the **current** interaction has staleness = 0.0.
/// For files sent `age` interactions ago the staleness is:
///
/// ```text
/// staleness = 1.0 - decay_rate ^ age
/// ```
///
/// With the default `decay_rate = 0.5` this gives:
///   age 1 → 0.5, age 2 → 0.75, age 3 → 0.875, …
pub struct ContextMemory {
    /// file_path -> MemoryEntry
    entries: HashMap<String, MemoryEntry>,
    /// Current interaction number (incremented via [`next_interaction`]).
    interaction: usize,
    /// Decay factor per interaction (0.0 = instant forget, 1.0 = never forget).
    decay_rate: f64,
}

impl ContextMemory {
    /// Create a new memory with the default decay rate (0.5).
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            interaction: 0,
            decay_rate: 0.5,
        }
    }

    /// Create a new memory with a custom decay rate.
    ///
    /// # Panics
    /// Panics if `decay_rate` is not in the range `[0.0, 1.0]`.
    pub fn with_decay_rate(decay_rate: f64) -> Self {
        assert!(
            (0.0..=1.0).contains(&decay_rate),
            "decay_rate must be in [0.0, 1.0], got {decay_rate}"
        );
        Self {
            entries: HashMap::new(),
            interaction: 0,
            decay_rate,
        }
    }

    /// Advance to the next interaction. Must be called before recording sends.
    pub fn next_interaction(&mut self) {
        self.interaction += 1;
    }

    /// Record that a file was sent in the current interaction.
    pub fn record_sent(&mut self, file_path: &str, detail: DetailLevel, score: f64) {
        self.entries.insert(
            file_path.to_string(),
            MemoryEntry {
                last_seen: self.interaction,
                detail_level: detail,
                original_score: score,
            },
        );
    }

    /// Staleness of a file: 0.0 = just sent, 1.0 = never sent / fully decayed.
    ///
    /// Used to **boost** unseen files and **penalise** recently-sent files when
    /// scoring communities for the next interaction.
    pub fn staleness(&self, file_path: &str) -> f64 {
        match self.entries.get(file_path) {
            Some(entry) => {
                let age = self.interaction.saturating_sub(entry.last_seen);
                if age == 0 {
                    return 0.0;
                }
                1.0 - self.decay_rate.powi(age as i32)
            }
            None => 1.0,
        }
    }

    /// Adjust community scores based on staleness of their constituent files.
    ///
    /// For each community the average staleness of its files is computed and
    /// the score is scaled by `0.5 + 0.5 * avg_staleness`. This means:
    ///   - Fully stale (all unseen) → score unchanged (×1.0)
    ///   - Fully fresh (all just sent) → score halved (×0.5)
    pub fn adjust_scores(
        &self,
        files_per_community: &HashMap<String, Vec<String>>,
        scores: &mut HashMap<String, f64>,
    ) {
        for (comm_id, score) in scores.iter_mut() {
            if let Some(files) = files_per_community.get(comm_id.as_str()) {
                if files.is_empty() {
                    continue;
                }
                let avg_staleness: f64 =
                    files.iter().map(|f| self.staleness(f)).sum::<f64>() / files.len() as f64;
                *score *= 0.5 + 0.5 * avg_staleness;
            }
        }
    }

    /// Return impact files that have NOT been recently seen (staleness > 0.7).
    ///
    /// Useful to surface ripple-effect files after an edit without resending
    /// files the agent already has in its conversation history.
    pub fn get_unseen_impact(
        &self,
        _edited_files: &[String],
        impact_files: &[String],
    ) -> Vec<String> {
        impact_files
            .iter()
            .filter(|f| self.staleness(f) > 0.7)
            .cloned()
            .collect()
    }

    /// Clear all memory (start of a new session).
    pub fn reset(&mut self) {
        self.entries.clear();
        self.interaction = 0;
    }

    /// Current interaction counter.
    pub fn interaction_count(&self) -> usize {
        self.interaction
    }
}

impl Default for ContextMemory {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staleness_unseen_is_one() {
        let mem = ContextMemory::new();
        assert!((mem.staleness("src/foo.rs") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_staleness_just_sent_is_zero() {
        let mut mem = ContextMemory::new();
        mem.next_interaction();
        mem.record_sent("src/foo.rs", DetailLevel::FullCode, 0.9);
        assert!((mem.staleness("src/foo.rs")).abs() < f64::EPSILON);
    }

    #[test]
    fn test_staleness_decays_over_interactions() {
        let mut mem = ContextMemory::new();
        mem.next_interaction(); // interaction 1
        mem.record_sent("src/foo.rs", DetailLevel::Signatures, 0.8);

        mem.next_interaction(); // interaction 2 — age 1
        let s1 = mem.staleness("src/foo.rs");
        assert!((s1 - 0.5).abs() < 1e-9, "age 1 staleness should be 0.5, got {s1}");

        mem.next_interaction(); // interaction 3 — age 2
        let s2 = mem.staleness("src/foo.rs");
        assert!((s2 - 0.75).abs() < 1e-9, "age 2 staleness should be 0.75, got {s2}");

        mem.next_interaction(); // interaction 4 — age 3
        let s3 = mem.staleness("src/foo.rs");
        assert!((s3 - 0.875).abs() < 1e-9, "age 3 staleness should be 0.875, got {s3}");

        // Staleness must be monotonically increasing with age.
        assert!(s1 < s2);
        assert!(s2 < s3);
    }

    #[test]
    fn test_adjust_scores_boosts_unseen() {
        let mut mem = ContextMemory::new();
        mem.next_interaction();
        mem.record_sent("a.rs", DetailLevel::FullCode, 1.0);

        mem.next_interaction(); // a.rs is now age 1

        let files: HashMap<String, Vec<String>> = HashMap::from([
            ("seen_comm".to_string(), vec!["a.rs".to_string()]),
            ("unseen_comm".to_string(), vec!["b.rs".to_string()]),
        ]);

        let mut scores: HashMap<String, f64> = HashMap::from([
            ("seen_comm".to_string(), 1.0),
            ("unseen_comm".to_string(), 1.0),
        ]);

        mem.adjust_scores(&files, &mut scores);

        let seen_score = scores["seen_comm"];
        let unseen_score = scores["unseen_comm"];

        // unseen community should have a higher adjusted score
        assert!(
            unseen_score > seen_score,
            "unseen ({unseen_score}) should beat seen ({seen_score})"
        );

        // unseen: avg_staleness = 1.0 → score * (0.5 + 0.5*1.0) = 1.0
        assert!((unseen_score - 1.0).abs() < 1e-9);

        // seen (age 1, staleness 0.5): score * (0.5 + 0.5*0.5) = 0.75
        assert!((seen_score - 0.75).abs() < 1e-9);
    }

    #[test]
    fn test_reset_clears_all() {
        let mut mem = ContextMemory::new();
        mem.next_interaction();
        mem.record_sent("src/foo.rs", DetailLevel::Compressed, 0.5);

        // Before reset the file is fresh.
        assert!(mem.staleness("src/foo.rs") < 1.0);
        assert_eq!(mem.interaction_count(), 1);

        mem.reset();

        // After reset everything is unseen again.
        assert!((mem.staleness("src/foo.rs") - 1.0).abs() < f64::EPSILON);
        assert_eq!(mem.interaction_count(), 0);
    }

    #[test]
    fn test_get_unseen_impact_filters_recently_seen() {
        let mut mem = ContextMemory::new();
        mem.next_interaction();
        mem.record_sent("recent.rs", DetailLevel::FullCode, 0.9);
        // "old.rs" was never sent → staleness 1.0 > 0.7

        let edited = vec!["edited.rs".to_string()];
        let impact = vec!["recent.rs".to_string(), "old.rs".to_string()];

        let unseen = mem.get_unseen_impact(&edited, &impact);
        assert_eq!(unseen, vec!["old.rs".to_string()]);
    }

    #[test]
    fn test_custom_decay_rate() {
        let mut mem = ContextMemory::with_decay_rate(0.8);
        mem.next_interaction();
        mem.record_sent("x.rs", DetailLevel::Signatures, 1.0);

        mem.next_interaction(); // age 1
        let s = mem.staleness("x.rs");
        // staleness = 1.0 - 0.8^1 = 0.2
        assert!((s - 0.2).abs() < 1e-9, "expected 0.2, got {s}");
    }

    #[test]
    #[should_panic(expected = "decay_rate must be in [0.0, 1.0]")]
    fn test_invalid_decay_rate_panics() {
        ContextMemory::with_decay_rate(1.5);
    }

    #[test]
    fn test_resend_resets_staleness() {
        let mut mem = ContextMemory::new();
        mem.next_interaction();
        mem.record_sent("f.rs", DetailLevel::FullCode, 0.9);

        mem.next_interaction(); // age 1 → staleness 0.5
        assert!((mem.staleness("f.rs") - 0.5).abs() < 1e-9);

        // Re-send in current interaction resets staleness to 0.
        mem.record_sent("f.rs", DetailLevel::Signatures, 0.7);
        assert!((mem.staleness("f.rs")).abs() < f64::EPSILON);
    }
}
