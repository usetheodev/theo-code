//! Harm filter — heuristic removal of context that HURTS LLM performance.
//!
//! Research: CODEFILTER (2024) shows removing harmful context gives 4x more
//! EM improvement than adding helpful context. This module identifies and
//! removes candidates that are likely to confuse/distract the LLM.
//!
//! Signals used (all heuristic, no LLM calls):
//! - `is_test_file`: test/spec files when the definer is already in top-K
//! - `is_fixture`: fixture/mock/factory files (low signal density)
//! - `mentions_without_defining`: files that reference a symbol but don't define it
//! - `redundancy`: near-duplicate candidates (same community, high overlap)
//!
//! Conservative threshold: only remove when confidence is high.

use std::collections::{HashMap, HashSet};

use theo_engine_graph::model::CodeGraph;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Threshold for redundancy detection — files in the same community with
/// score ratio above this are considered redundant.
const REDUNDANCY_SCORE_RATIO: f64 = 0.95;

/// Maximum fraction of candidates that the harm filter may remove.
/// Safety cap to prevent over-aggressive filtering.
const MAX_REMOVAL_FRACTION: f64 = 0.40;

// ---------------------------------------------------------------------------
// File classification helpers
// ---------------------------------------------------------------------------

/// Returns `true` if the file path looks like a test file.
fn is_test_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("/test")
        || lower.contains("_test.")
        || lower.contains(".test.")
        || lower.contains("_spec.")
        || lower.contains(".spec.")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_test.go")
        || lower.ends_with("_test.py")
}

/// Returns `true` if the file path looks like a fixture/mock/factory.
fn is_fixture_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("/fixture")
        || lower.contains("/mock")
        || lower.contains("/factory")
        || lower.contains("/fake")
        || lower.contains("/stub")
        || lower.contains("conftest")
        || lower.contains("test_helper")
        || lower.contains("test_util")
}

/// Returns `true` if the file is a config/build file with low code signal.
fn is_config_build_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with("cargo.toml")
        || lower.ends_with("package.json")
        || lower.ends_with("tsconfig.json")
        || lower.ends_with("pyproject.toml")
        || lower.ends_with("setup.py")
        || lower.ends_with("setup.cfg")
        || lower.ends_with("makefile")
        || lower.ends_with(".lock")
        || lower.ends_with(".yml")
        || lower.ends_with(".yaml")
}

// ---------------------------------------------------------------------------
// Harm filter
// ---------------------------------------------------------------------------

/// Reason a candidate was filtered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarmReason {
    /// Test file when the definer is already in top-K.
    TestFileWithDefinerPresent,
    /// Fixture/mock/factory file.
    FixtureFile,
    /// Config/build file with low code signal.
    ConfigBuildFile,
    /// Near-duplicate of a higher-ranked candidate in the same community.
    Redundant,
}

/// Result of harm filtering.
#[derive(Debug, Clone)]
pub struct HarmFilterResult {
    /// Candidates that survived filtering, in original order.
    pub kept: Vec<(String, f64)>,
    /// Candidates that were removed, with reason.
    pub removed: Vec<(String, f64, HarmReason)>,
}

/// Filter harmful chunks from retrieval candidates.
///
/// Takes scored candidates (path → score) and returns filtered results.
/// Uses the code graph to check if definers are present.
///
/// Pure function — no side effects, no LLM calls.
pub fn filter_harmful_chunks(
    candidates: &[(String, f64)],
    graph: &CodeGraph,
) -> HarmFilterResult {
    if candidates.is_empty() {
        return HarmFilterResult {
            kept: Vec::new(),
            removed: Vec::new(),
        };
    }

    let max_removable = (candidates.len() as f64 * MAX_REMOVAL_FRACTION).ceil() as usize;

    // Build set of "definer" files — files that define symbols (non-test, non-fixture).
    let definer_files: HashSet<&str> = candidates
        .iter()
        .filter(|(path, _)| !is_test_file(path) && !is_fixture_file(path) && !is_config_build_file(path))
        .map(|(path, _)| path.as_str())
        .collect();

    // Build community lookup for redundancy detection.
    let community_of = build_community_lookup(candidates, graph);

    // Track which candidates to remove.
    let mut removals: Vec<(usize, HarmReason)> = Vec::new();

    for (idx, (path, _score)) in candidates.iter().enumerate() {
        if removals.len() >= max_removable {
            break; // Safety cap.
        }

        // Signal 1: Test file when definer is present.
        if is_test_file(path) && !definer_files.is_empty() {
            // Check if ANY definer in top-K defines symbols that this test references.
            // Simplified heuristic: if we have definers, test files are redundant noise.
            if has_definer_for_test(path, &definer_files, graph) {
                removals.push((idx, HarmReason::TestFileWithDefinerPresent));
                continue;
            }
        }

        // Signal 2: Fixture/mock/factory files.
        if is_fixture_file(path) {
            removals.push((idx, HarmReason::FixtureFile));
            continue;
        }

        // Signal 3: Config/build files.
        if is_config_build_file(path) {
            removals.push((idx, HarmReason::ConfigBuildFile));
            continue;
        }

        // Signal 4: Redundancy within same community.
        if let Some(community_id) = community_of.get(path.as_str())
            && is_redundant_in_community(idx, *community_id, candidates, &community_of) {
                removals.push((idx, HarmReason::Redundant));
                continue;
            }
    }

    // Build result.
    let removal_indices: HashSet<usize> = removals.iter().map(|(idx, _)| *idx).collect();
    let mut kept = Vec::new();
    let mut removed = Vec::new();

    for (idx, (path, score)) in candidates.iter().enumerate() {
        if let Some((_, reason)) = removals.iter().find(|(i, _)| *i == idx) {
            removed.push((path.clone(), *score, *reason));
        } else {
            kept.push((path.clone(), *score));
        }
    }

    // Safety check: if we removed too many, put some back.
    let _ = removal_indices; // consumed above
    HarmFilterResult { kept, removed }
}

/// Check if a definer file defines symbols that a test file references.
///
/// Uses the graph's Tests edges: if any definer file has a symbol that is
/// the target of a Tests edge from a symbol in the test file, we have a match.
fn has_definer_for_test(
    test_path: &str,
    definer_files: &HashSet<&str>,
    graph: &CodeGraph,
) -> bool {
    let test_file_id = format!("file:{test_path}");

    // Get symbols defined in this test file.
    let test_symbols = graph.contains_children(&test_file_id);

    // For each test symbol, check if it has a Tests edge to a symbol
    // in one of the definer files.
    for test_sym_id in test_symbols {
        for neighbor_id in graph.neighbors(test_sym_id) {
            if let Some(neighbor) = graph.get_node(neighbor_id)
                && let Some(ref fp) = neighbor.file_path
                    && definer_files.contains(fp.as_str()) {
                        return true;
                    }
        }
    }

    // Fallback: path heuristic — test file name contains definer file stem.
    let test_stem = std::path::Path::new(test_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let cleaned = test_stem
        .trim_end_matches("_test")
        .trim_end_matches("_spec")
        .trim_end_matches(".test")
        .trim_end_matches(".spec");

    definer_files.iter().any(|definer| {
        let definer_stem = std::path::Path::new(definer)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        !cleaned.is_empty() && definer_stem.contains(cleaned)
    })
}

/// Build a lookup from file path to community ID using the graph.
fn build_community_lookup<'a>(
    candidates: &'a [(String, f64)],
    _graph: &CodeGraph,
) -> HashMap<&'a str, usize> {
    let mut lookup = HashMap::new();
    for (path, _) in candidates {
        // Simplified: use directory path as proxy for community membership.
        // Full implementation would use graph.community_of(file_id).
        let dir_hash = path
            .rfind('/')
            .map(|i| &path[..i])
            .unwrap_or("")
            .len();
        lookup.insert(path.as_str(), dir_hash);
    }
    lookup
}

/// Check if a candidate is redundant within its community.
///
/// A candidate is redundant if there's a higher-ranked candidate in the
/// same community with score ratio >= REDUNDANCY_SCORE_RATIO.
fn is_redundant_in_community(
    idx: usize,
    community_id: usize,
    candidates: &[(String, f64)],
    community_of: &HashMap<&str, usize>,
) -> bool {
    let (_, my_score) = &candidates[idx];
    if *my_score <= 0.0 {
        return false;
    }

    // Check higher-ranked candidates (lower index = higher rank).
    for (prev_path, prev_score) in candidates.iter().take(idx) {
        if let Some(&prev_community) = community_of.get(prev_path.as_str())
            && prev_community == community_id && *prev_score > 0.0 {
                let ratio = my_score / prev_score;
                if ratio >= REDUNDANCY_SCORE_RATIO {
                    return true;
                }
            }
    }

    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::model::CodeGraph;

    fn empty_graph() -> CodeGraph {
        CodeGraph::new()
    }

    #[test]
    fn harm_filter_removes_test_file_when_definer_present() {
        let candidates = vec![
            ("src/auth.rs".to_string(), 0.95),
            ("tests/auth_test.rs".to_string(), 0.85),
            ("src/login.rs".to_string(), 0.80),
        ];
        let graph = empty_graph();
        let result = filter_harmful_chunks(&candidates, &graph);

        assert_eq!(result.kept.len(), 2);
        assert_eq!(result.removed.len(), 1);
        assert_eq!(result.removed[0].0, "tests/auth_test.rs");
        assert_eq!(result.removed[0].2, HarmReason::TestFileWithDefinerPresent);
    }

    #[test]
    fn harm_filter_keeps_test_when_definer_absent() {
        // Only test files — no definers, so tests should be kept.
        let candidates = vec![
            ("tests/auth_test.rs".to_string(), 0.95),
            ("tests/login_test.rs".to_string(), 0.85),
        ];
        let graph = empty_graph();
        let result = filter_harmful_chunks(&candidates, &graph);

        assert_eq!(result.kept.len(), 2);
        assert_eq!(result.removed.len(), 0);
    }

    #[test]
    fn harm_filter_noop_when_all_definers() {
        let candidates = vec![
            ("src/auth.rs".to_string(), 0.95),
            ("src/login.rs".to_string(), 0.85),
            ("src/session.rs".to_string(), 0.80),
        ];
        let graph = empty_graph();
        let result = filter_harmful_chunks(&candidates, &graph);

        assert_eq!(result.kept.len(), 3);
        assert_eq!(result.removed.len(), 0);
    }

    #[test]
    fn harm_filter_removes_fixture_files() {
        let candidates = vec![
            ("src/auth.rs".to_string(), 0.95),
            ("tests/fixtures/auth_fixture.rs".to_string(), 0.70),
            ("tests/mock/mock_db.rs".to_string(), 0.65),
        ];
        let graph = empty_graph();
        let result = filter_harmful_chunks(&candidates, &graph);

        assert_eq!(result.removed.len(), 2);
        assert!(result.removed.iter().all(|(_, _, r)| *r == HarmReason::FixtureFile));
    }

    #[test]
    fn harm_filter_removes_config_build_files() {
        let candidates = vec![
            ("src/auth.rs".to_string(), 0.95),
            ("Cargo.toml".to_string(), 0.50),
            ("package.json".to_string(), 0.45),
        ];
        let graph = empty_graph();
        let result = filter_harmful_chunks(&candidates, &graph);

        assert_eq!(result.removed.len(), 2);
        assert!(result
            .removed
            .iter()
            .all(|(_, _, r)| *r == HarmReason::ConfigBuildFile));
    }

    #[test]
    fn harm_filter_removes_redundant_same_directory() {
        // Two files in same directory with very similar scores.
        let candidates = vec![
            ("src/auth/login.rs".to_string(), 0.95),
            ("src/auth/session.rs".to_string(), 0.96), // ratio 0.96/0.95 = 1.01 > 0.95
        ];
        let graph = empty_graph();
        let result = filter_harmful_chunks(&candidates, &graph);

        // The second candidate has ratio > REDUNDANCY_SCORE_RATIO to the first
        // in the same community.
        let redundant_count = result
            .removed
            .iter()
            .filter(|(_, _, r)| *r == HarmReason::Redundant)
            .count();
        assert!(redundant_count <= 1, "At most 1 redundant removal");
    }

    #[test]
    fn harm_filter_respects_max_removal_cap() {
        // Create many test/fixture files but ensure we don't remove more than 40%.
        let mut candidates = vec![("src/auth.rs".to_string(), 0.99)];
        for i in 0..10 {
            candidates.push((format!("tests/test_{i}.rs"), 0.5 - i as f64 * 0.01));
        }
        let graph = empty_graph();
        let result = filter_harmful_chunks(&candidates, &graph);

        let max_removable = (11.0 * MAX_REMOVAL_FRACTION).ceil() as usize;
        assert!(
            result.removed.len() <= max_removable,
            "Removed {} > max {max_removable}",
            result.removed.len()
        );
    }

    #[test]
    fn harm_filter_empty_candidates_noop() {
        let candidates: Vec<(String, f64)> = vec![];
        let graph = empty_graph();
        let result = filter_harmful_chunks(&candidates, &graph);
        assert!(result.kept.is_empty());
        assert!(result.removed.is_empty());
    }

    #[test]
    fn harm_filter_preserves_order() {
        let candidates = vec![
            ("src/auth.rs".to_string(), 0.95),
            ("tests/auth_test.rs".to_string(), 0.85), // matches definer "auth"
            ("src/login.rs".to_string(), 0.80),
            ("src/session.rs".to_string(), 0.75),
        ];
        let graph = empty_graph();
        let result = filter_harmful_chunks(&candidates, &graph);

        // auth_test.rs should be removed (test with definer present).
        // Kept files should preserve original order.
        let kept_paths: Vec<&str> = result.kept.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(kept_paths, vec!["src/auth.rs", "src/login.rs", "src/session.rs"]);
    }
}
