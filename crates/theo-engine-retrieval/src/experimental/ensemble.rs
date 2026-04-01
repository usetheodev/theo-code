/// Ensemble context builder: produces 3 complementary perspectives
/// (lexical, semantic, structural) for LLM consumption.
///
/// Each perspective surfaces the top-3 files from a different scoring signal,
/// reducing single-signal blind spots and giving the LLM triangulation data.
use std::collections::HashMap;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Three complementary context perspectives for ensemble prompting.
#[derive(Debug, Clone)]
pub struct EnsembleContext {
    /// Top files by BM25 (exact term match).
    pub lexical: String,
    /// Top files by neural similarity.
    pub semantic: String,
    /// Top files by graph centrality + co-changes.
    pub structural: String,
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Build ensemble context from pre-computed per-file scores.
///
/// Each perspective picks the top-3 files according to its signal and formats
/// them with relevant metadata extracted from `graph`.
pub fn build_ensemble(
    communities: &[Community],
    graph: &CodeGraph,
    _query: &str,
    bm25_scores: &HashMap<String, f64>,
    semantic_scores: &HashMap<String, f64>,
    centrality_scores: &HashMap<String, f64>,
) -> EnsembleContext {
    // Collect all file-level node ids that belong to at least one community.
    let community_files: Vec<String> = communities
        .iter()
        .flat_map(|c| c.node_ids.iter().cloned())
        .collect();

    let lexical = format_lexical(&community_files, graph, bm25_scores);
    let semantic = format_semantic(&community_files, graph, semantic_scores);
    let structural = format_structural(&community_files, graph, centrality_scores);

    EnsembleContext {
        lexical,
        semantic,
        structural,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Top-N files to include per perspective.
const TOP_N: usize = 3;

/// Pick top-N ids from `scores` that also appear in `allowed`.
fn top_n(allowed: &[String], scores: &HashMap<String, f64>, n: usize) -> Vec<(String, f64)> {
    let mut pairs: Vec<(String, f64)> = allowed
        .iter()
        .filter_map(|id| scores.get(id).map(|&s| (id.clone(), s)))
        .collect();
    pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    pairs.truncate(n);
    pairs
}

/// Collect signatures of symbol nodes contained by a file node.
fn file_signatures(file_id: &str, graph: &CodeGraph) -> Vec<String> {
    graph
        .edges_of_type(&EdgeType::Contains)
        .into_iter()
        .filter(|e| e.source == file_id)
        .filter_map(|e| {
            graph
                .get_node(&e.target)
                .and_then(|n| n.signature.clone().or_else(|| Some(n.name.clone())))
        })
        .collect()
}

fn format_lexical(
    files: &[String],
    graph: &CodeGraph,
    bm25_scores: &HashMap<String, f64>,
) -> String {
    let mut out = String::from("=== LEXICAL MATCH ===\nFiles where your query terms appear literally:\n");
    let top = top_n(files, bm25_scores, TOP_N);
    if top.is_empty() {
        out.push_str("(no lexical matches)\n");
        return out;
    }
    for (id, _score) in &top {
        let sigs = file_signatures(id, graph);
        let sig_str = if sigs.is_empty() {
            String::new()
        } else {
            format!(": {}", sigs.join(", "))
        };
        out.push_str(&format!("- {}{}\n", id, sig_str));
    }
    out
}

fn format_semantic(
    files: &[String],
    graph: &CodeGraph,
    semantic_scores: &HashMap<String, f64>,
) -> String {
    let mut out = String::from("=== SEMANTIC MATCH ===\nFiles semantically related to your intent:\n");
    let top = top_n(files, semantic_scores, TOP_N);
    if top.is_empty() {
        out.push_str("(no semantic matches)\n");
        return out;
    }
    for (id, score) in &top {
        let _node = graph.get_node(id);
        out.push_str(&format!("- {} (similarity: {:.2})\n", id, score));
    }
    out
}

fn format_structural(
    files: &[String],
    graph: &CodeGraph,
    centrality_scores: &HashMap<String, f64>,
) -> String {
    let mut out = String::from(
        "=== STRUCTURAL MATCH ===\nFiles structurally important in the dependency graph:\n",
    );
    let top = top_n(files, centrality_scores, TOP_N);
    if top.is_empty() {
        out.push_str("(no structural matches)\n");
        return out;
    }
    for (id, _score) in &top {
        let mut relations: Vec<String> = Vec::new();

        // Outgoing edges (CALLS)
        for neighbor in graph.neighbors(id) {
            let edges = graph.edges_between(id, neighbor);
            for edge in edges {
                relations.push(format!("-> {} ({:?})", neighbor, edge.edge_type));
            }
        }

        // Incoming edges (CALLED_BY)
        for neighbor in graph.reverse_neighbors(id) {
            let edges = graph.edges_between(neighbor, id);
            for edge in edges {
                relations.push(format!("<- {} ({:?})", neighbor, edge.edge_type));
            }
        }

        if relations.is_empty() {
            out.push_str(&format!("- {} (isolated)\n", id));
        } else {
            // Show at most 3 relations to keep it compact.
            let shown: Vec<&str> = relations.iter().map(|s| s.as_str()).take(3).collect();
            out.push_str(&format!("- {} {}\n", id, shown.join(", ")));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::cluster::Community;
    use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType};

    fn make_graph() -> CodeGraph {
        let mut g = CodeGraph::new();
        g.add_node(Node {
            id: "file_a.rs".into(),
            node_type: NodeType::File,
            name: "file_a.rs".into(),
            file_path: Some("file_a.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });
        g.add_node(Node {
            id: "fn_foo".into(),
            node_type: NodeType::Symbol,
            name: "foo".into(),
            file_path: Some("file_a.rs".into()),
            signature: Some("fn foo()".into()),
            kind: None,
            line_start: Some(1),
            line_end: Some(10),
            last_modified: 0.0,
            doc: None,
        });
        g.add_edge(Edge {
            source: "file_a.rs".into(),
            target: "fn_foo".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        g.add_node(Node {
            id: "file_b.rs".into(),
            node_type: NodeType::File,
            name: "file_b.rs".into(),
            file_path: Some("file_b.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });
        g
    }

    fn make_communities() -> Vec<Community> {
        vec![Community {
            id: "c1".into(),
            name: "cluster1".into(),
            level: 0,
            node_ids: vec!["file_a.rs".into(), "file_b.rs".into()],
            parent_id: None,
            version: 1,
        }]
    }

    #[test]
    fn lexical_section_contains_top_file() {
        let graph = make_graph();
        let comms = make_communities();
        let mut bm25 = HashMap::new();
        bm25.insert("file_a.rs".into(), 1.5);
        bm25.insert("file_b.rs".into(), 0.3);

        let ctx = build_ensemble(&comms, &graph, "foo", &bm25, &HashMap::new(), &HashMap::new());

        assert!(ctx.lexical.contains("file_a.rs"));
        assert!(ctx.lexical.contains("LEXICAL MATCH"));
    }

    #[test]
    fn semantic_section_shows_similarity() {
        let graph = make_graph();
        let comms = make_communities();
        let mut sem = HashMap::new();
        sem.insert("file_b.rs".into(), 0.89);

        let ctx = build_ensemble(&comms, &graph, "bar", &HashMap::new(), &sem, &HashMap::new());

        assert!(ctx.semantic.contains("0.89"));
        assert!(ctx.semantic.contains("file_b.rs"));
    }

    #[test]
    fn structural_section_shows_edges() {
        let graph = make_graph();
        let comms = make_communities();
        let mut cent = HashMap::new();
        cent.insert("file_a.rs".into(), 0.95);

        let ctx = build_ensemble(&comms, &graph, "foo", &HashMap::new(), &HashMap::new(), &cent);

        assert!(ctx.structural.contains("file_a.rs"));
        assert!(ctx.structural.contains("STRUCTURAL MATCH"));
    }

    #[test]
    fn empty_scores_produce_no_match_labels() {
        let graph = make_graph();
        let comms = make_communities();

        let ctx = build_ensemble(
            &comms,
            &graph,
            "nope",
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(ctx.lexical.contains("no lexical matches"));
        assert!(ctx.semantic.contains("no semantic matches"));
        assert!(ctx.structural.contains("no structural matches"));
    }
}
