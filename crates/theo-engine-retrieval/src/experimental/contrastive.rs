/// Contrastive context: finds confusable functions and adds "NOT THIS" markers
/// so the LLM can distinguish between similarly-named symbols.
///
/// Confusion is detected via shared token overlap (from `tokenise`).
/// When overlap > 50%, the pair is flagged as contrastive.
use std::collections::HashSet;

use theo_engine_graph::model::CodeGraph;

use crate::search::tokenise;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A pair of functions where one is the intended target and the other is a
/// similarly-named symbol that the LLM might confuse with it.
#[derive(Debug, Clone)]
pub struct ContrastiveItem {
    /// The function the LLM should use.
    pub target_name: String,
    /// Signature of the target function.
    pub target_signature: String,
    /// A similar function it might confuse with.
    pub confusable_name: String,
    /// Signature of the confusable function.
    pub confusable_signature: String,
    /// Human-readable explanation of why they are different.
    pub distinction: String,
}

// ---------------------------------------------------------------------------
// Core algorithm
// ---------------------------------------------------------------------------

/// Find functions in `graph` that could be confused with `target_node_id`.
///
/// Confusion = shared tokens between function names > 50%.
/// Only symbol nodes are considered as candidates.
pub fn find_confusables(
    target_node_id: &str,
    graph: &CodeGraph,
    query_tokens: &HashSet<String>,
) -> Vec<ContrastiveItem> {
    let target_node = match graph.get_node(target_node_id) {
        Some(n) => n,
        None => return Vec::new(),
    };

    let target_name_tokens: HashSet<String> = tokenise(&target_node.name)
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect();

    if target_name_tokens.is_empty() {
        return Vec::new();
    }

    let target_sig = target_node
        .signature
        .clone()
        .unwrap_or_else(|| target_node.name.clone());

    let mut items = Vec::new();

    for candidate in graph.symbol_nodes() {
        // Skip the target itself.
        if candidate.id == target_node_id {
            continue;
        }

        let cand_name_tokens: HashSet<String> = tokenise(&candidate.name)
            .into_iter()
            .map(|t| t.to_lowercase())
            .collect();

        if cand_name_tokens.is_empty() {
            continue;
        }

        let similarity = token_similarity(&target_name_tokens, &cand_name_tokens);

        if similarity > 0.5 {
            let cand_sig = candidate
                .signature
                .clone()
                .unwrap_or_else(|| candidate.name.clone());

            let distinction = build_distinction(
                &target_node.name,
                &candidate.name,
                &target_name_tokens,
                &cand_name_tokens,
                query_tokens,
            );

            items.push(ContrastiveItem {
                target_name: target_node.name.clone(),
                target_signature: target_sig.clone(),
                confusable_name: candidate.name.clone(),
                confusable_signature: cand_sig,
                distinction,
            });
        }
    }

    // Sort by descending similarity (most confusable first).
    items.sort_by(|a, b| {
        let sim_a = token_similarity(
            &target_name_tokens,
            &tokenise(&a.confusable_name)
                .into_iter()
                .map(|t| t.to_lowercase())
                .collect(),
        );
        let sim_b = token_similarity(
            &target_name_tokens,
            &tokenise(&b.confusable_name)
                .into_iter()
                .map(|t| t.to_lowercase())
                .collect(),
        );
        sim_b
            .partial_cmp(&sim_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    items
}

/// Format contrastive items into a prompt-ready string.
pub fn format_contrastive(items: &[ContrastiveItem]) -> String {
    if items.is_empty() {
        return String::new();
    }

    let mut out = String::from("=== CONTRASTIVE CONTEXT (NOT THIS) ===\n");
    for item in items {
        out.push_str(&format!(
            "USE: {} — {}\n  NOT: {} — {}\n  WHY: {}\n\n",
            item.target_name,
            item.target_signature,
            item.confusable_name,
            item.confusable_signature,
            item.distinction,
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Jaccard-like token similarity: |intersection| / |union|.
fn token_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

/// Build a human-readable distinction between two confusable names.
fn build_distinction(
    target_name: &str,
    confusable_name: &str,
    target_tokens: &HashSet<String>,
    confusable_tokens: &HashSet<String>,
    _query_tokens: &HashSet<String>,
) -> String {
    let only_in_target: Vec<&String> = target_tokens.difference(confusable_tokens).collect();
    let only_in_confusable: Vec<&String> = confusable_tokens.difference(target_tokens).collect();

    let mut parts = Vec::new();

    if !only_in_target.is_empty() {
        parts.push(format!(
            "'{}' contains [{}]",
            target_name,
            only_in_target
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    if !only_in_confusable.is_empty() {
        parts.push(format!(
            "'{}' contains [{}]",
            confusable_name,
            only_in_confusable
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }

    if parts.is_empty() {
        format!(
            "'{}' and '{}' share all tokens but are distinct symbols",
            target_name, confusable_name
        )
    } else {
        parts.join("; ")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::model::{CodeGraph, Node, NodeType, SymbolKind};

    fn make_graph() -> CodeGraph {
        let mut g = CodeGraph::new();

        // Target: build_context
        g.add_node(Node {
            id: "fn_build_context".into(),
            node_type: NodeType::Symbol,
            name: "build_context".into(),
            file_path: Some("context.rs".into()),
            signature: Some("fn build_context(q: &str) -> Context".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(50),
            last_modified: 0.0,
            doc: None,
        });

        // Confusable: build_context_map (shares "build" and "context")
        g.add_node(Node {
            id: "fn_build_context_map".into(),
            node_type: NodeType::Symbol,
            name: "build_context_map".into(),
            file_path: Some("context.rs".into()),
            signature: Some("fn build_context_map(g: &Graph) -> HashMap".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(60),
            line_end: Some(90),
            last_modified: 0.0,
            doc: None,
        });

        // Non-confusable: totally_different
        g.add_node(Node {
            id: "fn_totally_different".into(),
            node_type: NodeType::Symbol,
            name: "totally_different".into(),
            file_path: Some("other.rs".into()),
            signature: Some("fn totally_different()".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(5),
            last_modified: 0.0,
            doc: None,
        });

        g
    }

    #[test]
    fn finds_confusable_by_shared_tokens() {
        let graph = make_graph();
        let query_tokens: HashSet<String> = ["build", "context"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let items = find_confusables("fn_build_context", &graph, &query_tokens);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].confusable_name, "build_context_map");
    }

    #[test]
    fn does_not_flag_unrelated_functions() {
        let graph = make_graph();
        let query_tokens: HashSet<String> = ["build"].iter().map(|s| s.to_string()).collect();

        let items = find_confusables("fn_totally_different", &graph, &query_tokens);

        assert!(items.is_empty());
    }

    #[test]
    fn returns_empty_for_missing_node() {
        let graph = make_graph();
        let query_tokens: HashSet<String> = HashSet::new();

        let items = find_confusables("nonexistent", &graph, &query_tokens);

        assert!(items.is_empty());
    }

    #[test]
    fn format_contrastive_produces_markers() {
        let items = vec![ContrastiveItem {
            target_name: "build_context".into(),
            target_signature: "fn build_context()".into(),
            confusable_name: "build_context_map".into(),
            confusable_signature: "fn build_context_map()".into(),
            distinction: "different purpose".into(),
        }];

        let output = format_contrastive(&items);

        assert!(output.contains("USE: build_context"));
        assert!(output.contains("NOT: build_context_map"));
        assert!(output.contains("WHY: different purpose"));
    }
}
