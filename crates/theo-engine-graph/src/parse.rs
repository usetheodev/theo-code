/// Tree-sitter parsing stub.
///
/// Full tree-sitter parsing is handled in `intently-core`. This module
/// provides a lightweight helper that constructs `FileNode` + `SymbolNode`
/// entries from a simple descriptor, without requiring a tree-sitter grammar
/// at this layer.
use crate::model::{CodeGraph, Node, NodeType, SymbolKind};

// ---------------------------------------------------------------------------
// Simple symbol descriptor (no tree-sitter dependency)
// ---------------------------------------------------------------------------

/// Minimal description of a symbol found in a file.
#[derive(Debug, Clone)]
pub struct SymbolDescriptor {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub signature: Option<String>,
    pub last_modified: f64,
}

/// Add a file node and all its symbol nodes to `graph`.
///
/// This is the entry point called by the orchestration layer after it has
/// walked a file with tree-sitter (or any other parser).
pub fn ingest_file(graph: &mut CodeGraph, file_id: &str, file_path: &str, last_modified: f64) {
    graph.add_node(Node {
        id: file_id.to_string(),
        node_type: NodeType::File,
        name: file_path.to_string(),
        file_path: Some(file_path.to_string()),
        signature: None,
        kind: None,
        line_start: None,
        line_end: None,
        last_modified,
        doc: None,
    });
}

/// Add a symbol node to `graph`.
pub fn ingest_symbol(graph: &mut CodeGraph, desc: &SymbolDescriptor) {
    graph.add_node(Node {
        id: desc.id.clone(),
        node_type: NodeType::Symbol,
        name: desc.name.clone(),
        file_path: Some(desc.file_path.clone()),
        signature: desc.signature.clone(),
        kind: Some(desc.kind.clone()),
        line_start: Some(desc.line_start),
        line_end: Some(desc.line_end),
        last_modified: desc.last_modified,
        doc: None,
    });
}
