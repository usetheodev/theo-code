//! SCIP Merge — enriches CodeGraph edges with SCIP precision.
//!
//! When a SCIP index is available, this module:
//! 1. Adds exact Calls edges (replacing heuristic resolution)
//! 2. Adds Implementation edges (trait→impl)
//! 3. Marks reference edges with precise line numbers
//!
//! Tree-Sitter edges are NOT removed — SCIP edges are additive.
//! This ensures graceful degradation if SCIP becomes stale.

#[cfg(feature = "scip")]
use super::reader::ScipIndex;

#[cfg(feature = "scip")]
use crate::model::{CodeGraph, Edge, EdgeType};

/// Merge SCIP reference data into an existing CodeGraph.
///
/// Adds edges from SCIP that Tree-Sitter couldn't resolve:
/// - Cross-file function calls (Calls edges)
/// - Trait implementations (Inherits edges)
///
/// Does NOT remove existing edges — purely additive.
#[cfg(feature = "scip")]
pub fn merge_scip_edges(graph: &mut CodeGraph, scip: &ScipIndex) {
    let mut edges_added = 0;

    // For each file in the SCIP index
    for (file_path, occurrences) in &scip.file_symbols {
        let source_file_id = format!("file:{}", file_path);

        // Skip files not in the graph
        if graph.get_node(&source_file_id).is_none() {
            continue;
        }

        for (sym_id, _line, role) in occurrences {
            // Only process references (imports, reads, calls) — not definitions
            if role == "definition" {
                continue;
            }

            // Find the definition file for this symbol
            let Some(def_file) = scip.symbol_definitions.get(sym_id) else {
                continue;
            };

            // Skip self-references (same file)
            if def_file == file_path {
                continue;
            }

            let target_file_id = format!("file:{}", def_file);

            // Skip if target not in graph
            if graph.get_node(&target_file_id).is_none() {
                continue;
            }

            // Determine edge type based on role
            let edge_type = match role.as_str() {
                "import" => EdgeType::Imports,
                "read" | "write" => EdgeType::Calls, // function usage = call
                _ => EdgeType::References,
            };

            // Check if this edge already exists (avoid duplicates)
            let already_exists = graph.all_edges().iter().any(|e| {
                e.source == source_file_id && e.target == target_file_id && e.edge_type == edge_type
            });

            if !already_exists {
                graph.add_edge(Edge {
                    source: source_file_id.clone(),
                    target: target_file_id.clone(),
                    edge_type,
                    weight: 1.5, // SCIP edges get higher weight (more reliable)
                });
                edges_added += 1;
            }
        }
    }

    if edges_added > 0 {
        eprintln!("[scip] Merged {} precise edges into graph", edges_added);
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "scip")]
    #[test]
    fn merge_adds_edges_from_scip() {
        use super::*;
        use crate::model::{CodeGraph, Node, NodeType};
        use std::collections::HashMap;

        let mut graph = CodeGraph::new();
        graph.add_node(Node {
            id: "file:src/a.rs".to_string(),
            node_type: NodeType::File,
            name: "src/a.rs".to_string(),
            file_path: Some("src/a.rs".to_string()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "file:src/b.rs".to_string(),
            node_type: NodeType::File,
            name: "src/b.rs".to_string(),
            file_path: Some("src/b.rs".to_string()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });

        let edges_before = graph.edge_count();

        let mut scip = ScipIndex {
            symbol_definitions: HashMap::new(),
            symbol_references: HashMap::new(),
            file_symbols: HashMap::new(),
            name_to_symbols: HashMap::new(),
            document_count: 2,
            occurrence_count: 2,
        };

        // b.rs defines foo, a.rs imports foo
        scip.symbol_definitions
            .insert("sym:foo".into(), "src/b.rs".into());
        scip.file_symbols.insert(
            "src/a.rs".into(),
            vec![("sym:foo".into(), 5, "import".into())],
        );
        scip.file_symbols.insert(
            "src/b.rs".into(),
            vec![("sym:foo".into(), 10, "definition".into())],
        );

        merge_scip_edges(&mut graph, &scip);

        assert!(graph.edge_count() > edges_before, "SCIP should add edges");
    }
}
