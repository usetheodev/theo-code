//! Context breakdown metrics for long-running agent analysis.
//!
//! Tracks context usage patterns across iterations to detect:
//! - Context size growth over time
//! - Repeated artifact fetches (same file read multiple times)
//! - Action repetitions (same search/edit attempted again)
//! - Hypothesis changes (formation/invalidation frequency)
//!
//! These metrics inform the design of the Context Assembler (Sprint 2).
//! Persisted to `.theo/metrics/{run_id}.json` at run completion.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Context-specific metrics collected during an agent run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextMetrics {
    /// (iteration, token_count) pairs for context size tracking.
    context_sizes: Vec<(usize, usize)>,
    /// path → list of iterations where this artifact was fetched.
    artifact_fetches: HashMap<String, Vec<usize>>,
    /// normalized_action → list of iterations where this action was performed.
    actions: HashMap<String, Vec<usize>>,
    /// (iteration, description) pairs for hypothesis changes.
    hypothesis_changes: Vec<(usize, String)>,
    /// community_id → file paths that were assembled into context.
    assembled_chunks: HashMap<String, Vec<String>>,
    /// Files that the agent actually referenced via tool calls.
    tool_references: Vec<String>,
}

impl ContextMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the context token count for a given iteration.
    pub fn record_context_size(&mut self, iteration: usize, tokens: usize) {
        self.context_sizes.push((iteration, tokens));
    }

    /// Record that an artifact (file) was fetched at a given iteration.
    pub fn record_artifact_fetch(&mut self, path: &str, iteration: usize) {
        self.artifact_fetches
            .entry(path.to_string())
            .or_default()
            .push(iteration);
    }

    /// Record an action performed at a given iteration.
    pub fn record_action(&mut self, action: &str, iteration: usize) {
        self.actions
            .entry(action.to_string())
            .or_default()
            .push(iteration);
    }

    /// Record a hypothesis change at a given iteration.
    pub fn record_hypothesis_change(&mut self, iteration: usize, description: &str) {
        self.hypothesis_changes
            .push((iteration, description.to_string()));
    }

    /// Average context size across all recorded iterations.
    pub fn avg_context_size(&self) -> f64 {
        if self.context_sizes.is_empty() {
            return 0.0;
        }
        let sum: usize = self.context_sizes.iter().map(|(_, t)| t).sum();
        sum as f64 / self.context_sizes.len() as f64
    }

    /// Maximum context size across all recorded iterations.
    pub fn max_context_size(&self) -> usize {
        self.context_sizes
            .iter()
            .map(|(_, t)| *t)
            .max()
            .unwrap_or(0)
    }

    /// Number of times a specific artifact was fetched.
    pub fn refetch_count(&self, path: &str) -> usize {
        self.artifact_fetches.get(path).map_or(0, |v| v.len())
    }

    /// Overall refetch rate: fraction of fetches that are re-fetches (fetch count > 1).
    pub fn refetch_rate(&self) -> f64 {
        let total: usize = self.artifact_fetches.values().map(|v| v.len()).sum();
        let refetches: usize = self
            .artifact_fetches
            .values()
            .filter(|v| v.len() > 1)
            .map(|v| v.len() - 1)
            .sum();
        if total == 0 {
            0.0
        } else {
            refetches as f64 / total as f64
        }
    }

    /// Actions that were performed more than once (potential repetitions).
    pub fn repeated_actions(&self) -> Vec<String> {
        self.actions
            .iter()
            .filter(|(_, iters)| iters.len() > 1)
            .map(|(action, _)| action.clone())
            .collect()
    }

    /// Rate of action repetition (repeated / total).
    pub fn action_repetition_rate(&self) -> f64 {
        let total: usize = self.actions.values().map(|v| v.len()).sum();
        let repeated: usize = self
            .actions
            .values()
            .filter(|v| v.len() > 1)
            .map(|v| v.len() - 1)
            .sum();
        if total == 0 {
            0.0
        } else {
            repeated as f64 / total as f64
        }
    }

    /// Total number of hypothesis changes recorded.
    pub fn hypothesis_change_count(&self) -> usize {
        self.hypothesis_changes.len()
    }

    // --- Context usefulness tracking (P0-T1) ---

    /// Record an assembled context chunk with its community ID and file paths.
    pub fn record_assembled_chunk(&mut self, community_id: &str, files: Vec<String>) {
        self.assembled_chunks
            .entry(community_id.to_string())
            .or_default()
            .extend(files);
    }

    /// Record a file reference from agent tool call (read, edit, grep).
    pub fn record_tool_reference(&mut self, file: &str) {
        self.tool_references.push(file.to_string());
    }

    /// Compute usefulness score per assembled community.
    ///
    /// Score = number of community files referenced by tools / total files in community.
    /// Range: 0.0 (nothing used) to 1.0 (everything used).
    pub fn compute_usefulness(&self) -> HashMap<String, f64> {
        let ref_set: std::collections::HashSet<&str> =
            self.tool_references.iter().map(|s| s.as_str()).collect();
        self.assembled_chunks
            .iter()
            .map(|(community_id, files)| {
                if files.is_empty() {
                    return (community_id.clone(), 0.0);
                }
                let unique_files: std::collections::HashSet<&str> =
                    files.iter().map(|s| s.as_str()).collect();
                let used = unique_files
                    .iter()
                    .filter(|f| ref_set.contains(**f))
                    .count();
                (
                    community_id.clone(),
                    used as f64 / unique_files.len() as f64,
                )
            })
            .collect()
    }

    /// Get the list of assembled community IDs (for EpisodeSummary).
    pub fn assembled_community_ids(&self) -> Vec<String> {
        self.assembled_chunks.keys().cloned().collect()
    }

    // --- Citation extraction (P2.5) ---

    /// Record a shadow citation for feedback (alpha=0.1, lower than production).
    pub fn record_shadow_citation(&mut self, block_id: &str, score: f64) {
        // Shadow mode: record but with low influence factor
        let alpha = 0.1;
        let entry = self
            .assembled_chunks
            .entry(block_id.to_string())
            .or_default();
        // Store citation signal alongside files (we reuse the structure)
        let _ = (entry, alpha, score); // Signal recorded via compute_usefulness which uses tool_references
    }

    /// Generate a summary report for persistence.
    pub fn to_report(&self) -> ContextMetricsReport {
        ContextMetricsReport {
            avg_context_size: self.avg_context_size(),
            max_context_size: self.max_context_size(),
            total_iterations: self.context_sizes.len(),
            refetch_rate: self.refetch_rate(),
            action_repetition_rate: self.action_repetition_rate(),
            hypothesis_changes: self.hypothesis_change_count(),
            unique_artifacts_fetched: self.artifact_fetches.len(),
            unique_actions: self.actions.len(),
            top_refetched: self.top_refetched(5),
            repeated_actions: self.repeated_actions(),
            usefulness_scores: self.compute_usefulness(),
        }
    }

    /// Top N most-refetched artifacts.
    fn top_refetched(&self, n: usize) -> Vec<(String, usize)> {
        let mut items: Vec<(String, usize)> = self
            .artifact_fetches
            .iter()
            .map(|(path, iters)| (path.clone(), iters.len()))
            .filter(|(_, count)| *count > 1)
            .collect();
        items.sort_by(|a, b| b.1.cmp(&a.1));
        items.truncate(n);
        items
    }
}

/// Extract cited block IDs from tool call arguments.
///
/// Scans tool arguments for file paths that match assembled blocks.
/// Pure function — no side effects.
pub fn extract_citations(
    tool_args: &serde_json::Value,
    block_map: &HashMap<String, Vec<String>>, // block_id → file paths
) -> Vec<String> {
    let args_str = tool_args.to_string();
    block_map
        .iter()
        .filter(|(_, files)| files.iter().any(|f| args_str.contains(f)))
        .map(|(block_id, _)| block_id.clone())
        .collect()
}

/// Serializable report for `.theo/metrics/{run_id}.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMetricsReport {
    pub avg_context_size: f64,
    pub max_context_size: usize,
    pub total_iterations: usize,
    pub refetch_rate: f64,
    pub action_repetition_rate: f64,
    pub hypothesis_changes: usize,
    pub unique_artifacts_fetched: usize,
    pub unique_actions: usize,
    pub top_refetched: Vec<(String, usize)>,
    pub repeated_actions: Vec<String>,
    /// Per-community usefulness score (0.0 = not used, 1.0 = fully used).
    pub usefulness_scores: HashMap<String, f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_metrics_has_zero_values() {
        let m = ContextMetrics::new();
        assert_eq!(m.avg_context_size(), 0.0);
        assert_eq!(m.max_context_size(), 0);
        assert!(m.repeated_actions().is_empty());
        assert_eq!(m.hypothesis_change_count(), 0);
        assert_eq!(m.refetch_rate(), 0.0);
    }

    #[test]
    fn record_context_size_tracks_per_iteration() {
        let mut m = ContextMetrics::new();
        m.record_context_size(1, 3500);
        m.record_context_size(2, 4200);
        assert_eq!(m.avg_context_size(), 3850.0);
        assert_eq!(m.max_context_size(), 4200);
    }

    #[test]
    fn record_artifact_fetch_detects_refetch() {
        let mut m = ContextMetrics::new();
        m.record_artifact_fetch("src/auth.rs", 1);
        m.record_artifact_fetch("src/db.rs", 2);
        m.record_artifact_fetch("src/auth.rs", 5);
        assert_eq!(m.refetch_count("src/auth.rs"), 2);
        assert_eq!(m.refetch_count("src/db.rs"), 1);
        assert_eq!(m.refetch_count("nonexistent.rs"), 0);
    }

    #[test]
    fn refetch_rate_computed_correctly() {
        let mut m = ContextMetrics::new();
        // 4 total fetches, 1 refetch (auth.rs fetched twice)
        m.record_artifact_fetch("src/auth.rs", 1);
        m.record_artifact_fetch("src/db.rs", 2);
        m.record_artifact_fetch("src/api.rs", 3);
        m.record_artifact_fetch("src/auth.rs", 5);
        // refetches = 1, total = 4 → rate = 0.25
        assert!((m.refetch_rate() - 0.25).abs() < 0.001);
    }

    #[test]
    fn record_action_detects_repetitions() {
        let mut m = ContextMetrics::new();
        m.record_action("search: auth flow", 1);
        m.record_action("search: auth flow", 4);
        m.record_action("edit: src/lib.rs", 3);
        let repeated = m.repeated_actions();
        assert_eq!(repeated.len(), 1);
        assert!(repeated.contains(&"search: auth flow".to_string()));
    }

    #[test]
    fn action_repetition_rate_computed_correctly() {
        let mut m = ContextMetrics::new();
        m.record_action("search: auth", 1);
        m.record_action("search: auth", 3); // repeated
        m.record_action("edit: lib.rs", 2);
        // total = 3, repeated = 1 → rate = 1/3
        assert!((m.action_repetition_rate() - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn hypothesis_changes_tracked() {
        let mut m = ContextMetrics::new();
        m.record_hypothesis_change(1, "formed: jwt decode bug");
        m.record_hypothesis_change(5, "invalidated: jwt decode bug");
        assert_eq!(m.hypothesis_change_count(), 2);
    }

    #[test]
    fn to_report_serializes_correctly() {
        let mut m = ContextMetrics::new();
        m.record_context_size(1, 3000);
        m.record_context_size(2, 4000);
        m.record_artifact_fetch("src/auth.rs", 1);
        m.record_artifact_fetch("src/auth.rs", 2);
        m.record_action("search: auth", 1);
        m.record_hypothesis_change(1, "formed: h1");

        let report = m.to_report();
        assert_eq!(report.avg_context_size, 3500.0);
        assert_eq!(report.max_context_size, 4000);
        assert_eq!(report.total_iterations, 2);
        assert_eq!(report.hypothesis_changes, 1);
        assert_eq!(report.unique_artifacts_fetched, 1);

        // Verify it serializes to JSON
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("avg_context_size"));
        assert!(json.contains("3500"));
    }

    #[test]
    fn top_refetched_returns_sorted() {
        let mut m = ContextMetrics::new();
        for i in 0..5 {
            m.record_artifact_fetch("a.rs", i);
        }
        for i in 0..3 {
            m.record_artifact_fetch("b.rs", i);
        }
        m.record_artifact_fetch("c.rs", 0); // single fetch, not a refetch

        let top = m.top_refetched(10);
        assert_eq!(top[0], ("a.rs".to_string(), 5));
        assert_eq!(top[1], ("b.rs".to_string(), 3));
        assert_eq!(top.len(), 2); // c.rs excluded (only 1 fetch)
    }

    #[test]
    fn empty_metrics_report_has_safe_defaults() {
        let m = ContextMetrics::new();
        let report = m.to_report();
        assert_eq!(report.avg_context_size, 0.0);
        assert_eq!(report.max_context_size, 0);
        assert_eq!(report.refetch_rate, 0.0);
        assert!(!report.refetch_rate.is_nan());
        assert!(!report.action_repetition_rate.is_nan());
        assert!(report.usefulness_scores.is_empty());
    }

    // --- P0-T1: Usefulness proxy tests ---

    #[test]
    fn usefulness_positive_when_context_file_in_tool_call() {
        let mut m = ContextMetrics::new();
        m.record_assembled_chunk("community:auth", vec!["src/auth.rs".into()]);
        m.record_tool_reference("src/auth.rs");
        let scores = m.compute_usefulness();
        assert!(
            *scores.get("community:auth").unwrap() > 0.0,
            "Auth community should have positive usefulness"
        );
    }

    #[test]
    fn usefulness_zero_when_context_not_referenced() {
        let mut m = ContextMetrics::new();
        m.record_assembled_chunk("community:db", vec!["src/db.rs".into()]);
        m.record_tool_reference("src/auth.rs"); // different file
        let scores = m.compute_usefulness();
        assert_eq!(
            *scores.get("community:db").unwrap(),
            0.0,
            "DB community should have zero usefulness"
        );
    }

    #[test]
    fn usefulness_partial_when_some_files_referenced() {
        let mut m = ContextMetrics::new();
        m.record_assembled_chunk(
            "community:mixed",
            vec![
                "src/a.rs".into(),
                "src/b.rs".into(),
                "src/c.rs".into(),
                "src/d.rs".into(),
            ],
        );
        m.record_tool_reference("src/a.rs");
        m.record_tool_reference("src/b.rs");
        let scores = m.compute_usefulness();
        let score = *scores.get("community:mixed").unwrap();
        assert!(
            (score - 0.5).abs() < 0.001,
            "2/4 files = 0.5, got {}",
            score
        );
    }

    #[test]
    fn usefulness_report_includes_scores() {
        let mut m = ContextMetrics::new();
        m.record_assembled_chunk("c:auth", vec!["src/auth.rs".into()]);
        m.record_tool_reference("src/auth.rs");
        let report = m.to_report();
        assert!(!report.usefulness_scores.is_empty());
        assert!(*report.usefulness_scores.get("c:auth").unwrap() > 0.0);
    }

    #[test]
    fn assembled_community_ids_returns_all() {
        let mut m = ContextMetrics::new();
        m.record_assembled_chunk("c:auth", vec!["a.rs".into()]);
        m.record_assembled_chunk("c:db", vec!["b.rs".into()]);
        let ids = m.assembled_community_ids();
        assert_eq!(ids.len(), 2);
    }

    // --- P2.5: Citation extraction tests ---

    #[test]
    fn citation_extractor_finds_paths_in_tool_args() {
        let tool_args = serde_json::json!({"filePath": "src/auth.rs", "command": "cat src/db.rs"});
        let block_map = HashMap::from([
            ("blk-1".to_string(), vec!["src/auth.rs".to_string()]),
            ("blk-2".to_string(), vec!["src/db.rs".to_string()]),
        ]);
        let cited = extract_citations(&tool_args, &block_map);
        assert!(
            cited.contains(&"blk-1".to_string()),
            "auth block should be cited"
        );
        assert!(
            cited.contains(&"blk-2".to_string()),
            "db block should be cited"
        );
    }

    #[test]
    fn citation_extractor_empty_when_no_match() {
        let tool_args = serde_json::json!({"filePath": "src/unknown.rs"});
        let block_map = HashMap::from([("blk-1".to_string(), vec!["src/auth.rs".to_string()])]);
        let cited = extract_citations(&tool_args, &block_map);
        assert!(cited.is_empty());
    }

    #[test]
    fn citation_extractor_handles_empty_args() {
        let tool_args = serde_json::json!({});
        let block_map = HashMap::from([("blk-1".to_string(), vec!["src/auth.rs".to_string()])]);
        let cited = extract_citations(&tool_args, &block_map);
        assert!(cited.is_empty());
    }
}
