/// Predictive Pre-computation — pre-computes likely context from editor signals.
///
/// Uses active file, recent edits, branch name, and error messages to predict
/// which files will be needed before the user even issues a query.

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::tokenise;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Signals available before a query arrives.
#[derive(Debug, Clone)]
pub struct EditorSignals {
    /// File currently open in the editor.
    pub active_file: Option<String>,
    /// Files edited in the last 5 minutes.
    pub recent_edits: Vec<String>,
    /// Git branch name (e.g. "feature/add-rate-limiting").
    pub branch_name: Option<String>,
    /// Last error message from terminal.
    pub last_error: Option<String>,
}

/// Pre-computed context ready to serve instantly.
#[derive(Debug, Clone)]
pub struct PredictedContext {
    /// Predicted relevant files, ordered by likelihood (descending).
    pub predicted_files: Vec<(String, f64)>,
    /// Keywords extracted from signals (for query matching).
    pub predicted_keywords: Vec<String>,
    /// Confidence (0.0–1.0) in the prediction.
    pub confidence: f64,
}

// ---------------------------------------------------------------------------
// Signal weights
// ---------------------------------------------------------------------------

const WEIGHT_ACTIVE_FILE: f64 = 0.4;
const WEIGHT_RECENT_EDITS: f64 = 0.3;
const WEIGHT_BRANCH: f64 = 0.2;
const WEIGHT_ERROR: f64 = 0.1;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate predicted context from editor signals.
///
/// Combines four signals (active file, recent edits, branch name, last error)
/// into a ranked list of predicted files with an overall confidence score.
pub fn predict_context(
    signals: &EditorSignals,
    graph: &CodeGraph,
    communities: &[Community],
) -> PredictedContext {
    let mut file_scores: HashMap<String, f64> = HashMap::new();
    let mut keywords: Vec<String> = Vec::new();
    let mut active_signals = 0u32;

    // 1. Active file signal — community + 1-hop neighbours.
    if let Some(ref active) = signals.active_file {
        active_signals += 1;
        let community_files = files_in_same_community(active, communities);
        let neighbour_files = one_hop_file_neighbours(active, graph);

        for file in community_files.iter().chain(neighbour_files.iter()) {
            *file_scores.entry(file.clone()).or_insert(0.0) += WEIGHT_ACTIVE_FILE;
        }
        // The active file itself gets full weight.
        *file_scores.entry(active.clone()).or_insert(0.0) += WEIGHT_ACTIVE_FILE;
    }

    // 2. Recent edits signal — include neighbours of each edited file (impact).
    if !signals.recent_edits.is_empty() {
        active_signals += 1;
        for edited in &signals.recent_edits {
            *file_scores.entry(edited.clone()).or_insert(0.0) += WEIGHT_RECENT_EDITS;
            let neighbours = one_hop_file_neighbours(edited, graph);
            for nb in neighbours {
                *file_scores.entry(nb).or_insert(0.0) += WEIGHT_RECENT_EDITS * 0.5;
            }
        }
    }

    // 3. Branch name signal — extract keywords → match against file nodes.
    if let Some(ref branch) = signals.branch_name {
        active_signals += 1;
        let branch_keywords = extract_branch_keywords(branch);
        keywords.extend(branch_keywords.clone());

        let matched = match_keywords_to_files(&branch_keywords, graph);
        for (file, relevance) in matched {
            *file_scores.entry(file).or_insert(0.0) += WEIGHT_BRANCH * relevance;
        }
    }

    // 4. Last error signal — extract file paths and test names from error text.
    if let Some(ref error) = signals.last_error {
        active_signals += 1;
        let error_files = extract_files_from_error(error, graph);
        for file in error_files {
            *file_scores.entry(file).or_insert(0.0) += WEIGHT_ERROR;
        }
        // Also extract keywords from the error.
        let error_keywords = tokenise(error);
        keywords.extend(error_keywords);
    }

    // Sort files by score descending.
    let mut predicted_files: Vec<(String, f64)> = file_scores.into_iter().collect();
    predicted_files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Compute confidence based on how many signals contributed.
    let mut confidence = if active_signals == 0 {
        0.0
    } else {
        (active_signals as f64) / 4.0
    };

    // Boost confidence if active_file matches branch keywords.
    if let (Some(active), Some(branch)) = (&signals.active_file, &signals.branch_name) {
        let branch_kw: HashSet<String> = extract_branch_keywords(branch)
            .into_iter()
            .collect();
        let active_tokens: HashSet<String> = tokenise(active)
            .into_iter()
            .map(|t| t.to_lowercase())
            .collect();
        if branch_kw.iter().any(|kw| active_tokens.contains(kw)) {
            confidence = (confidence + 0.15).min(1.0);
        }
    }

    keywords.sort();
    keywords.dedup();

    PredictedContext {
        predicted_files,
        predicted_keywords: keywords,
        confidence,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Find all file paths in the same community as `file_path`.
fn files_in_same_community(file_path: &str, communities: &[Community]) -> Vec<String> {
    for community in communities {
        if community.node_ids.iter().any(|id| id == file_path) {
            return community
                .node_ids
                .iter()
                .filter(|id| *id != file_path)
                .cloned()
                .collect();
        }
    }
    Vec::new()
}

/// Get 1-hop file-node neighbours of a file in the graph.
///
/// Follows edges from the file node, then resolves each target symbol's
/// owning file via the `file_path` field on nodes.
fn one_hop_file_neighbours(file_path: &str, graph: &CodeGraph) -> Vec<String> {
    let mut files: HashSet<String> = HashSet::new();

    // Forward neighbours.
    for nb in graph.neighbors(file_path) {
        if let Some(node) = graph.get_node(nb) {
            if node.node_type == NodeType::File {
                files.insert(nb.to_string());
            } else if let Some(ref fp) = node.file_path {
                files.insert(fp.clone());
            }
        }
    }

    // Reverse neighbours.
    for nb in graph.reverse_neighbors(file_path) {
        if let Some(node) = graph.get_node(nb) {
            if node.node_type == NodeType::File {
                files.insert(nb.to_string());
            } else if let Some(ref fp) = node.file_path {
                files.insert(fp.clone());
            }
        }
    }

    files.remove(file_path);
    files.into_iter().collect()
}

/// Extract keywords from a branch name by splitting on `/`, `-`, `_`.
fn extract_branch_keywords(branch: &str) -> Vec<String> {
    branch
        .split(|c: char| c == '/' || c == '-' || c == '_')
        .filter(|s| !s.is_empty())
        // Filter out common branch prefixes that aren't useful keywords.
        .filter(|s| !matches!(s.to_lowercase().as_str(), "feature" | "fix" | "bugfix" | "hotfix" | "chore" | "refactor"))
        .map(|s| s.to_lowercase())
        .collect()
}

/// Match keywords against file nodes in the graph by checking if any keyword
/// appears in the file path or node name.
///
/// Returns (file_path, relevance) where relevance ∈ (0, 1].
fn match_keywords_to_files(keywords: &[String], graph: &CodeGraph) -> Vec<(String, f64)> {
    if keywords.is_empty() {
        return Vec::new();
    }
    let mut results = Vec::new();
    for node in graph.file_nodes() {
        let path_lower = node.id.to_lowercase();
        let name_lower = node.name.to_lowercase();

        let matched = keywords
            .iter()
            .filter(|kw| path_lower.contains(kw.as_str()) || name_lower.contains(kw.as_str()))
            .count();

        if matched > 0 {
            let relevance = matched as f64 / keywords.len() as f64;
            results.push((node.id.clone(), relevance));
        }
    }
    results
}

/// Extract file paths from error text by matching against known file nodes
/// in the graph.
fn extract_files_from_error(error_text: &str, graph: &CodeGraph) -> Vec<String> {
    let mut found = Vec::new();
    for node in graph.file_nodes() {
        // Check if the file path appears in the error text.
        if error_text.contains(&node.id) || error_text.contains(&node.name) {
            found.push(node.id.clone());
        }
    }

    // Also try to match tokens that look like file paths (contain `.` and `/`).
    for token in error_text.split_whitespace() {
        let clean = token.trim_matches(|c: char| c == '\'' || c == '"' || c == '(' || c == ')' || c == ':' || c == ',');
        if clean.contains('/') && clean.contains('.') {
            // Only add if it corresponds to a known file node.
            if graph.get_node(clean).is_some() {
                found.push(clean.to_string());
            }
        }
    }

    found.sort();
    found.dedup();
    found
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::model::{Edge, EdgeType, Node, SymbolKind};

    /// Build a small graph for testing:
    ///   auth.rs → (Contains) → verify_token → (Calls) → db.rs::get_user
    ///   db.rs → (Contains) → get_user
    ///   api.rs → (Imports) → auth.rs
    fn make_test_graph() -> CodeGraph {
        let mut g = CodeGraph::new();

        g.add_node(Node {
            id: "src/auth.rs".into(),
            node_type: NodeType::File,
            name: "auth.rs".into(),
            file_path: Some("src/auth.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });
        g.add_node(Node {
            id: "src/db.rs".into(),
            node_type: NodeType::File,
            name: "db.rs".into(),
            file_path: Some("src/db.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });
        g.add_node(Node {
            id: "src/api.rs".into(),
            node_type: NodeType::File,
            name: "api.rs".into(),
            file_path: Some("src/api.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });
        g.add_node(Node {
            id: "src/auth.rs::verify_token".into(),
            node_type: NodeType::Symbol,
            name: "verify_token".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn verify_token(token: &str) -> bool".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(30),
            last_modified: 0.0,
            doc: None,
        });
        g.add_node(Node {
            id: "src/db.rs::get_user".into(),
            node_type: NodeType::Symbol,
            name: "get_user".into(),
            file_path: Some("src/db.rs".into()),
            signature: Some("fn get_user(id: u64) -> User".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(5),
            line_end: Some(20),
            last_modified: 0.0,
            doc: None,
        });

        g.add_edge(Edge {
            source: "src/auth.rs".into(),
            target: "src/auth.rs::verify_token".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "src/db.rs".into(),
            target: "src/db.rs::get_user".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "src/auth.rs::verify_token".into(),
            target: "src/db.rs::get_user".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "src/api.rs".into(),
            target: "src/auth.rs".into(),
            edge_type: EdgeType::Imports,
            weight: 1.0,
        });

        g
    }

    fn make_test_communities() -> Vec<Community> {
        vec![Community {
            id: "auth-domain".into(),
            name: "auth-domain".into(),
            level: 0,
            node_ids: vec!["src/auth.rs".into(), "src/db.rs".into()],
            parent_id: None,
            version: 1,
        }]
    }

    #[test]
    fn test_active_file_pulls_community_and_neighbours() {
        // Arrange
        let graph = make_test_graph();
        let communities = make_test_communities();
        let signals = EditorSignals {
            active_file: Some("src/auth.rs".into()),
            recent_edits: vec![],
            branch_name: None,
            last_error: None,
        };

        // Act
        let result = predict_context(&signals, &graph, &communities);

        // Assert — should include db.rs (community mate) and api.rs (reverse neighbour).
        let files: Vec<&str> = result.predicted_files.iter().map(|(f, _)| f.as_str()).collect();
        assert!(files.contains(&"src/db.rs"), "db.rs should be predicted via community");
        assert!(files.contains(&"src/api.rs"), "api.rs should be predicted via reverse edge");
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn test_branch_name_extracts_keywords_and_matches_files() {
        // Arrange
        let graph = make_test_graph();
        let signals = EditorSignals {
            active_file: None,
            recent_edits: vec![],
            branch_name: Some("feature/auth-token".into()),
            last_error: None,
        };

        // Act
        let result = predict_context(&signals, &graph, &[]);

        // Assert — keywords "auth" and "token" should match auth.rs.
        assert!(
            result.predicted_keywords.contains(&"auth".to_string()),
            "keywords: {:?}",
            result.predicted_keywords
        );
        let files: Vec<&str> = result.predicted_files.iter().map(|(f, _)| f.as_str()).collect();
        assert!(files.contains(&"src/auth.rs"), "auth.rs should match branch keyword 'auth'");
    }

    #[test]
    fn test_error_signal_extracts_file_paths() {
        // Arrange
        let graph = make_test_graph();
        let signals = EditorSignals {
            active_file: None,
            recent_edits: vec![],
            branch_name: None,
            last_error: Some("error[E0308]: expected `bool`, found `u64` at src/auth.rs:15:10".into()),
        };

        // Act
        let result = predict_context(&signals, &graph, &[]);

        // Assert
        let files: Vec<&str> = result.predicted_files.iter().map(|(f, _)| f.as_str()).collect();
        assert!(files.contains(&"src/auth.rs"), "auth.rs should be extracted from error text");
    }

    #[test]
    fn test_confidence_boost_when_active_matches_branch() {
        // Arrange
        let graph = make_test_graph();
        let communities = make_test_communities();

        let signals_no_match = EditorSignals {
            active_file: Some("src/api.rs".into()),
            recent_edits: vec![],
            branch_name: Some("feature/db-migration".into()),
            last_error: None,
        };
        let signals_match = EditorSignals {
            active_file: Some("src/auth.rs".into()),
            recent_edits: vec![],
            branch_name: Some("feature/auth-refactor".into()),
            last_error: None,
        };

        // Act
        let result_no = predict_context(&signals_no_match, &graph, &communities);
        let result_yes = predict_context(&signals_match, &graph, &communities);

        // Assert — matching signal should have higher confidence.
        assert!(
            result_yes.confidence > result_no.confidence,
            "matched={} should be > unmatched={}",
            result_yes.confidence,
            result_no.confidence
        );
    }

    #[test]
    fn test_empty_signals_produce_empty_prediction() {
        // Arrange
        let graph = make_test_graph();
        let signals = EditorSignals {
            active_file: None,
            recent_edits: vec![],
            branch_name: None,
            last_error: None,
        };

        // Act
        let result = predict_context(&signals, &graph, &[]);

        // Assert
        assert!(result.predicted_files.is_empty());
        assert_eq!(result.confidence, 0.0);
    }
}
