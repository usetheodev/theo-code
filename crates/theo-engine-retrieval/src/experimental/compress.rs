/// Intent-aware semantic compression for code symbols.
///
/// Generates compact (~8-line) representations of functions/symbols by
/// extracting structural metadata from the code graph instead of sending
/// full source code. This dramatically reduces token usage while preserving
/// the information an LLM needs to reason about the code.
use std::collections::HashSet;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A compressed representation of a function/symbol.
#[derive(Debug, Clone)]
pub struct CompressedSymbol {
    pub signature: String,
    pub flow: Vec<String>,
    pub errors: Vec<String>,
    pub calls: Vec<String>,
    pub tested_by: Vec<String>,
    pub untested_paths: Vec<String>,
    pub co_changes: Vec<(String, f64)>,
}

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

/// Generate compressed representations for all symbols in a community.
pub fn compress_community(community: &Community, graph: &CodeGraph) -> Vec<CompressedSymbol> {
    let community_ids: HashSet<&str> = community.node_ids.iter().map(String::as_str).collect();

    community
        .node_ids
        .iter()
        .filter_map(|node_id| {
            let node = graph.get_node(node_id)?;
            if !matches!(node.node_type, NodeType::Symbol) {
                return None;
            }
            Some(compress_symbol(node_id, graph, &community_ids))
        })
        .collect()
}

/// Compress a single symbol node into a `CompressedSymbol`.
fn compress_symbol(
    node_id: &str,
    graph: &CodeGraph,
    community_ids: &HashSet<&str>,
) -> CompressedSymbol {
    let node = graph.get_node(node_id);
    let signature = node
        .and_then(|n| n.signature.clone())
        .unwrap_or_else(|| node.map(|n| n.name.clone()).unwrap_or_default());

    // 1. Calls: all outgoing CALLS edges targets
    let calls: Vec<String> = outgoing_targets(graph, node_id, &EdgeType::Calls);

    // 2. Flow: call chain within the same community
    let flow: Vec<String> = calls
        .iter()
        .filter(|target_name| {
            // Find the node id for this call target and check if it's in the community
            graph.neighbors(node_id).iter().any(|neighbor_id| {
                if let Some(neighbor) = graph.get_node(neighbor_id) {
                    neighbor.name == **target_name && community_ids.contains(neighbor_id)
                } else {
                    false
                }
            })
        })
        .cloned()
        .collect();

    // 3. Errors: extract error types from the signature
    let errors = extract_error_types(&signature);

    // 4. Tested by: incoming TESTS edges (test nodes that test this symbol)
    let tested_by: Vec<String> = incoming_sources(graph, node_id, &EdgeType::Tests);

    // 5. Untested paths: outgoing CALLS targets that have no incoming TESTS edges
    let untested_paths: Vec<String> = calls
        .iter()
        .filter(|call_name| {
            // Find the node id for this call target
            let target_id = find_node_id_by_name(graph, node_id, call_name, &EdgeType::Calls);
            match target_id {
                Some(tid) => incoming_sources(graph, &tid, &EdgeType::Tests).is_empty(),
                None => false,
            }
        })
        .cloned()
        .collect();

    // 6. Co-changes: find the file containing this symbol, then its CO_CHANGES edges
    let co_changes = file_co_changes(graph, node_id);

    CompressedSymbol {
        signature,
        flow,
        errors,
        calls,
        tested_by,
        untested_paths,
        co_changes,
    }
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Format a compressed symbol as a compact text block (~8 lines).
pub fn format_compressed(sym: &CompressedSymbol) -> String {
    let mut lines: Vec<String> = Vec::new();

    // Line 1: signature
    lines.push(format!("/// {}", sym.signature));

    // Line 2: flow (if any)
    if !sym.flow.is_empty() {
        lines.push(format!("/// Flow: {}", sym.flow.join(" -> ")));
    }

    // Line 3: errors (if any)
    if !sym.errors.is_empty() {
        lines.push(format!("/// Errors: {}", sym.errors.join(", ")));
    }

    // Line 4: calls (if any)
    if !sym.calls.is_empty() {
        lines.push(format!("/// Calls: {}", sym.calls.join(", ")));
    }

    // Line 5: tested by (if any)
    if !sym.tested_by.is_empty() || !sym.untested_paths.is_empty() {
        let mut parts: Vec<String> = Vec::new();
        if !sym.tested_by.is_empty() {
            parts.push(sym.tested_by.join(", "));
        }
        if !sym.untested_paths.is_empty() {
            parts.push(format!("(UNTESTED: {})", sym.untested_paths.join(", ")));
        }
        lines.push(format!("/// Tested by: {}", parts.join(" ")));
    }

    // Line 6: co-changes (if any)
    if !sym.co_changes.is_empty() {
        let entries: Vec<String> = sym
            .co_changes
            .iter()
            .map(|(file, weight)| format!("{} ({:.0}%)", file, weight * 100.0))
            .collect();
        lines.push(format!("/// Co-changes: {}", entries.join(", ")));
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Collect names of outgoing edge targets of a specific type from a node.
fn outgoing_targets(graph: &CodeGraph, node_id: &str, edge_type: &EdgeType) -> Vec<String> {
    let mut targets: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for edge in graph.all_edges() {
        if edge.source == node_id && &edge.edge_type == edge_type {
            if let Some(target_node) = graph.get_node(&edge.target) {
                if seen.insert(target_node.name.clone()) {
                    targets.push(target_node.name.clone());
                }
            }
        }
    }
    targets
}

/// Collect names of incoming edge sources of a specific type to a node.
fn incoming_sources(graph: &CodeGraph, node_id: &str, edge_type: &EdgeType) -> Vec<String> {
    let mut sources: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for edge in graph.all_edges() {
        if edge.target == node_id && &edge.edge_type == edge_type {
            if let Some(source_node) = graph.get_node(&edge.source) {
                if seen.insert(source_node.name.clone()) {
                    sources.push(source_node.name.clone());
                }
            }
        }
    }
    sources
}

/// Find the node ID of a call target by name, starting from a specific edge.
fn find_node_id_by_name(
    graph: &CodeGraph,
    source_id: &str,
    target_name: &str,
    edge_type: &EdgeType,
) -> Option<String> {
    for edge in graph.all_edges() {
        if edge.source == source_id && &edge.edge_type == edge_type {
            if let Some(target_node) = graph.get_node(&edge.target) {
                if target_node.name == target_name {
                    return Some(edge.target.clone());
                }
            }
        }
    }
    None
}

/// Extract error type names from a function signature.
///
/// Looks for patterns like `Result<T, ErrorType>` or `Option<T>`.
fn extract_error_types(signature: &str) -> Vec<String> {
    let mut errors: Vec<String> = Vec::new();

    // Match Result<_, ErrorType> pattern
    if let Some(result_pos) = signature.find("Result<") {
        let after = &signature[result_pos + 7..];
        // Find the matching closing >
        let mut depth = 1;
        let mut comma_pos = None;
        let mut end_pos = None;
        for (i, ch) in after.char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = Some(i);
                        break;
                    }
                }
                ',' if depth == 1 && comma_pos.is_none() => {
                    comma_pos = Some(i);
                }
                _ => {}
            }
        }
        if let (Some(comma), Some(end)) = (comma_pos, end_pos) {
            let error_type = after[comma + 1..end].trim().to_string();
            if !error_type.is_empty() {
                errors.push(error_type);
            }
        }
    }

    errors
}

/// Find co-change edges for the file containing a symbol node.
fn file_co_changes(graph: &CodeGraph, node_id: &str) -> Vec<(String, f64)> {
    let file_path = match graph.get_node(node_id) {
        Some(node) => node.file_path.as_deref(),
        None => None,
    };
    let file_path = match file_path {
        Some(fp) => fp,
        None => return Vec::new(),
    };

    // Find the file node for this path
    let file_node_id = graph
        .file_nodes()
        .iter()
        .find(|n| n.file_path.as_deref() == Some(file_path))
        .map(|n| n.id.clone());

    let file_node_id = match file_node_id {
        Some(id) => id,
        None => return Vec::new(),
    };

    // Collect co-change edges from this file node
    let mut co_changes: Vec<(String, f64)> = Vec::new();
    for edge in graph.all_edges() {
        if edge.edge_type == EdgeType::CoChanges {
            let other_id = if edge.source == file_node_id {
                Some(&edge.target)
            } else if edge.target == file_node_id {
                Some(&edge.source)
            } else {
                None
            };
            if let Some(other) = other_id {
                if let Some(other_node) = graph.get_node(other) {
                    let name = other_node.file_path.as_deref().unwrap_or(&other_node.name);
                    co_changes.push((name.to_string(), edge.weight));
                }
            }
        }
    }

    // Sort by weight descending
    co_changes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    co_changes
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::cluster::Community;
    use theo_engine_graph::model::{Edge, Node, SymbolKind};

    /// Helper: build a graph with interconnected symbols for testing.
    fn build_test_graph() -> (Community, CodeGraph) {
        let mut graph = CodeGraph::new();

        // File node
        graph.add_node(Node {
            id: "file:src/auth.rs".into(),
            node_type: NodeType::File,
            name: "src/auth.rs".into(),
            file_path: Some("src/auth.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 1000.0,
            doc: None,
        });

        // Symbol: verify_token
        graph.add_node(Node {
            id: "sym:verify_token".into(),
            node_type: NodeType::Symbol,
            name: "verify_token".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn verify_token(token: &str) -> Result<Claims, AuthError>".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(10),
            last_modified: 1000.0,
            doc: None,
        });

        // Symbol: decode_header
        graph.add_node(Node {
            id: "sym:decode_header".into(),
            node_type: NodeType::Symbol,
            name: "decode_header".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn decode_header(raw: &[u8]) -> Header".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(12),
            line_end: Some(20),
            last_modified: 1000.0,
            doc: None,
        });

        // Symbol: decode_claims (no tests)
        graph.add_node(Node {
            id: "sym:decode_claims".into(),
            node_type: NodeType::Symbol,
            name: "decode_claims".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn decode_claims(payload: &[u8]) -> Claims".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(22),
            line_end: Some(30),
            last_modified: 1000.0,
            doc: None,
        });

        // Test node
        graph.add_node(Node {
            id: "test:test_verify_valid".into(),
            node_type: NodeType::Test,
            name: "test_verify_valid".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn test_verify_valid()".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(50),
            line_end: Some(60),
            last_modified: 1000.0,
            doc: None,
        });

        // Co-change file
        graph.add_node(Node {
            id: "file:src/crypto.rs".into(),
            node_type: NodeType::File,
            name: "src/crypto.rs".into(),
            file_path: Some("src/crypto.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 1000.0,
            doc: None,
        });

        // Edges: Contains
        graph.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "sym:decode_header".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "sym:decode_claims".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // Edges: Calls (verify_token calls decode_header and decode_claims)
        graph.add_edge(Edge {
            source: "sym:verify_token".into(),
            target: "sym:decode_header".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "sym:verify_token".into(),
            target: "sym:decode_claims".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        // Edge: Tests (test_verify_valid tests verify_token)
        graph.add_edge(Edge {
            source: "test:test_verify_valid".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Tests,
            weight: 0.7,
        });

        // Edge: CoChanges (auth.rs <-> crypto.rs)
        graph.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "file:src/crypto.rs".into(),
            edge_type: EdgeType::CoChanges,
            weight: 0.85,
        });

        let community = Community {
            id: "comm_auth".into(),
            name: "auth/jwt".into(),
            level: 0,
            node_ids: vec![
                "file:src/auth.rs".into(),
                "sym:verify_token".into(),
                "sym:decode_header".into(),
                "sym:decode_claims".into(),
                "test:test_verify_valid".into(),
            ],
            parent_id: None,
            version: 1,
        };

        (community, graph)
    }

    #[test]
    fn test_compress_extracts_calls() {
        let (community, graph) = build_test_graph();
        let compressed = compress_community(&community, &graph);

        // Find verify_token's compressed representation
        let vt = compressed
            .iter()
            .find(|c| c.signature.contains("verify_token"))
            .expect("should have compressed verify_token");

        assert!(
            !vt.calls.is_empty(),
            "verify_token should have non-empty calls, got: {:?}",
            vt.calls
        );
        assert!(
            vt.calls.contains(&"decode_header".to_string()),
            "calls should include decode_header, got: {:?}",
            vt.calls
        );
        assert!(
            vt.calls.contains(&"decode_claims".to_string()),
            "calls should include decode_claims, got: {:?}",
            vt.calls
        );
    }

    #[test]
    fn test_compress_includes_tests() {
        let (community, graph) = build_test_graph();
        let compressed = compress_community(&community, &graph);

        let vt = compressed
            .iter()
            .find(|c| c.signature.contains("verify_token"))
            .expect("should have compressed verify_token");

        assert!(
            !vt.tested_by.is_empty(),
            "verify_token should have non-empty tested_by, got: {:?}",
            vt.tested_by
        );
        assert!(
            vt.tested_by.contains(&"test_verify_valid".to_string()),
            "tested_by should include test_verify_valid, got: {:?}",
            vt.tested_by
        );
    }

    #[test]
    fn test_format_fits_in_10_lines() {
        let (community, graph) = build_test_graph();
        let compressed = compress_community(&community, &graph);

        for sym in &compressed {
            let formatted = format_compressed(sym);
            let line_count = formatted.lines().count();
            assert!(
                line_count <= 10,
                "formatted output should be <= 10 lines, got {} lines for {}: \n{}",
                line_count,
                sym.signature,
                formatted
            );
        }
    }

    #[test]
    fn test_compress_empty_symbol() {
        let mut graph = CodeGraph::new();

        // A symbol with no edges at all
        graph.add_node(Node {
            id: "sym:lonely".into(),
            node_type: NodeType::Symbol,
            name: "lonely".into(),
            file_path: None,
            signature: Some("fn lonely()".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(3),
            last_modified: 1000.0,
            doc: None,
        });

        let community = Community {
            id: "comm_lonely".into(),
            name: "lonely".into(),
            level: 0,
            node_ids: vec!["sym:lonely".into()],
            parent_id: None,
            version: 1,
        };

        let compressed = compress_community(&community, &graph);
        assert_eq!(compressed.len(), 1);

        let sym = &compressed[0];
        assert_eq!(sym.signature, "fn lonely()");
        assert!(sym.calls.is_empty());
        assert!(sym.flow.is_empty());
        assert!(sym.errors.is_empty());
        assert!(sym.tested_by.is_empty());
        assert!(sym.untested_paths.is_empty());
        assert!(sym.co_changes.is_empty());

        // Formatted output should be minimal (just the signature line)
        let formatted = format_compressed(sym);
        assert_eq!(formatted, "/// fn lonely()");
    }

    #[test]
    fn test_compress_extracts_errors_from_signature() {
        let (community, graph) = build_test_graph();
        let compressed = compress_community(&community, &graph);

        let vt = compressed
            .iter()
            .find(|c| c.signature.contains("verify_token"))
            .expect("should have compressed verify_token");

        assert!(
            vt.errors.contains(&"AuthError".to_string()),
            "errors should include AuthError from Result<Claims, AuthError>, got: {:?}",
            vt.errors
        );
    }

    #[test]
    fn test_compress_detects_untested_paths() {
        let (community, graph) = build_test_graph();
        let compressed = compress_community(&community, &graph);

        let vt = compressed
            .iter()
            .find(|c| c.signature.contains("verify_token"))
            .expect("should have compressed verify_token");

        // decode_header has no TESTS edge -> should appear in untested_paths
        assert!(
            vt.untested_paths.contains(&"decode_header".to_string()),
            "untested_paths should include decode_header, got: {:?}",
            vt.untested_paths
        );
        // decode_claims also has no TESTS edge
        assert!(
            vt.untested_paths.contains(&"decode_claims".to_string()),
            "untested_paths should include decode_claims, got: {:?}",
            vt.untested_paths
        );
    }

    #[test]
    fn test_compress_includes_co_changes() {
        let (community, graph) = build_test_graph();
        let compressed = compress_community(&community, &graph);

        let vt = compressed
            .iter()
            .find(|c| c.signature.contains("verify_token"))
            .expect("should have compressed verify_token");

        assert!(
            !vt.co_changes.is_empty(),
            "verify_token should have co_changes from its file, got: {:?}",
            vt.co_changes
        );
        assert!(
            vt.co_changes.iter().any(|(f, _)| f.contains("crypto")),
            "co_changes should include crypto.rs, got: {:?}",
            vt.co_changes
        );
    }
}
