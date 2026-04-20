//! `MockRetrievalEngine` — deterministic fake for RM2 wiring tests.
//!
//! Lets a test say "when queried with X, pretend embedding+BM25 returned
//! these three scored entries" without booting tantivy.

use std::sync::Mutex;

use theo_domain::memory::MemoryEntry;

/// One scored retrieval hit. `score` is the blended RRF score the real
/// engine would have produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredEntry {
    pub entry: MemoryEntry,
    pub score: f32,
}

impl ScoredEntry {
    pub fn new(source: &str, content: &str, score: f32) -> Self {
        Self {
            entry: MemoryEntry {
                source: source.to_string(),
                content: content.to_string(),
                relevance_score: score,
            },
            score,
        }
    }
}

/// Deterministic fake. Queries always return the same ordered list until
/// `set_results` rewires them.
pub struct MockRetrievalEngine {
    results: Mutex<Vec<ScoredEntry>>,
    calls: Mutex<Vec<String>>,
}

impl MockRetrievalEngine {
    pub fn scored(results: Vec<ScoredEntry>) -> Self {
        Self {
            results: Mutex::new(results),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn empty() -> Self {
        Self::scored(Vec::new())
    }

    pub fn set_results(&self, results: Vec<ScoredEntry>) {
        *self.results.lock().unwrap() = results;
    }

    /// Record the query and return the configured result slice.
    pub fn query(&self, q: &str) -> Vec<ScoredEntry> {
        self.calls.lock().unwrap().push(q.to_string());
        self.results.lock().unwrap().clone()
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_engine_returns_empty() {
        let m = MockRetrievalEngine::empty();
        assert!(m.query("anything").is_empty());
    }

    #[test]
    fn scored_engine_returns_fixture_results_in_order() {
        let m = MockRetrievalEngine::scored(vec![
            ScoredEntry::new("builtin", "fact-1", 0.9),
            ScoredEntry::new("builtin", "fact-2", 0.5),
        ]);
        let r = m.query("q");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].entry.content, "fact-1");
        assert!((r[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn calls_record_queries_in_order() {
        let m = MockRetrievalEngine::empty();
        m.query("alpha");
        m.query("beta");
        assert_eq!(m.calls(), vec!["alpha", "beta"]);
    }

    #[test]
    fn set_results_rewires_response() {
        let m = MockRetrievalEngine::scored(vec![ScoredEntry::new("s", "old", 0.1)]);
        m.set_results(vec![ScoredEntry::new("s", "new", 0.99)]);
        assert_eq!(m.query("q")[0].entry.content, "new");
    }
}
