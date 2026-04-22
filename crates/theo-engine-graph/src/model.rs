/// MCPH — Multi-Relational Code Property Hypergraph data model.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    File,
    Symbol,
    Import,
    Type,
    Test,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Constant,
    Trait,
    Interface,
    Module,
}

// ---------------------------------------------------------------------------
// Edge types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    /// Structural containment (file → symbol): weight 1.0
    Contains,
    /// Direct call site: weight 1.0
    Calls,
    /// Import / use declaration: weight 1.0
    Imports,
    /// Class / struct inheritance: weight 1.0
    Inherits,
    /// Type annotation dependency: weight 0.8
    TypeDepends,
    /// Test function exercises subject: weight 0.7
    Tests,
    /// Co-changed in git commit (temporal decay): weight dynamic
    CoChanges,
    /// Non-call reference (mention, read): weight 1.0
    References,
}

impl EdgeType {
    /// Default edge weight as specified in the GRAPHCTX spec.
    pub fn default_weight(&self) -> f64 {
        match self {
            EdgeType::Contains => 1.0,
            EdgeType::Calls => 1.0,
            EdgeType::Imports => 1.0,
            EdgeType::Inherits => 1.0,
            EdgeType::TypeDepends => 0.8,
            EdgeType::Tests => 0.7,
            EdgeType::CoChanges => 1.0, // overridden by temporal_decay at creation
            EdgeType::References => 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Node and Edge structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub node_type: NodeType,
    pub name: String,
    pub file_path: Option<String>,
    pub signature: Option<String>,
    pub kind: Option<SymbolKind>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    /// Unix timestamp (seconds) or 0.0 if unknown.
    pub last_modified: f64,
    /// Documentation comment text (extracted from source).
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub edge_type: EdgeType,
    pub weight: f64,
}

// ---------------------------------------------------------------------------
// CodeGraph
// ---------------------------------------------------------------------------

/// The core graph data structure.
///
/// Nodes are keyed by their `id`. Edges are stored in a flat `Vec` so that
/// multiple edges between the same pair of nodes are supported (multi-graph).
/// Adjacency lists are maintained for O(degree) neighbor queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodeGraph {
    nodes: HashMap<String, Node>,
    edges: Vec<Edge>,
    /// Forward adjacency: source_id → Vec<target_id>
    adjacency: HashMap<String, Vec<String>>,
    /// Reverse adjacency: target_id → Vec<source_id>
    reverse_adjacency: HashMap<String, Vec<String>>,
    /// Pre-built index: source_id → Vec<target_id> for Contains edges only.
    /// Enables O(1) lookup of file children instead of O(total_edges) scan.
    #[serde(default)]
    contains_children_index: HashMap<String, Vec<String>>,
    /// Name index: symbol name → Vec<node_id>.
    /// Enables O(1) symbol lookup by name for symbol-first retrieval.
    #[serde(default)]
    name_index: HashMap<String, Vec<String>>,
}

impl CodeGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    // --- Mutation ---------------------------------------------------------

    /// Insert or overwrite a node. Overwrites if `id` already exists.
    pub fn add_node(&mut self, node: Node) {
        let id = node.id.clone();
        let name = node.name.clone();
        self.nodes.insert(id.clone(), node);
        // Ensure adjacency entries exist even for isolated nodes.
        self.adjacency.entry(id.clone()).or_default();
        self.reverse_adjacency.entry(id.clone()).or_default();
        // Update name index for symbol-first lookup.
        self.name_index.entry(name).or_default().push(id);
    }

    /// Lookup node IDs by symbol name. Returns empty if not found.
    pub fn nodes_by_name(&self, name: &str) -> &[String] {
        self.name_index
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Append an edge. Both endpoints need not exist in the node map (the
    /// graph allows dangling edges for incremental construction), but the
    /// adjacency lists are always updated.
    pub fn add_edge(&mut self, edge: Edge) {
        let src = edge.source.clone();
        let tgt = edge.target.clone();

        self.adjacency
            .entry(src.clone())
            .or_default()
            .push(tgt.clone());
        self.reverse_adjacency
            .entry(tgt.clone())
            .or_default()
            .push(src.clone());

        // Maintain the contains-children index for O(1) child lookups.
        if edge.edge_type == EdgeType::Contains {
            self.contains_children_index
                .entry(src.clone())
                .or_default()
                .push(tgt.clone());
        }

        // Ensure reverse entry for src and forward entry for tgt.
        self.adjacency.entry(tgt).or_default();
        self.reverse_adjacency.entry(src).or_default();

        self.edges.push(edge);
    }

    // --- Read helpers -----------------------------------------------------

    /// Number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges (may include parallel edges).
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Retrieve a node by id.
    pub fn get_node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Outgoing neighbor ids (deduplicated by position, NOT by value — may
    /// contain duplicates if multiple edges go to the same target).
    pub fn neighbors(&self, id: &str) -> Vec<&str> {
        self.adjacency
            .get(id)
            .map(|v| v.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Incoming neighbor ids.
    pub fn reverse_neighbors(&self, id: &str) -> Vec<&str> {
        self.reverse_adjacency
            .get(id)
            .map(|v| v.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    // --- Filtered views ---------------------------------------------------

    /// All nodes with `node_type == NodeType::Symbol`.
    pub fn symbol_nodes(&self) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|n| matches!(n.node_type, NodeType::Symbol))
            .collect()
    }

    /// All nodes with `node_type == NodeType::File`.
    pub fn file_nodes(&self) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|n| matches!(n.node_type, NodeType::File))
            .collect()
    }

    /// All edges whose `edge_type` matches the given type.
    pub fn edges_of_type(&self, edge_type: &EdgeType) -> Vec<&Edge> {
        self.edges
            .iter()
            .filter(|e| &e.edge_type == edge_type)
            .collect()
    }

    /// All edges between a specific (source, target) pair (any edge type).
    pub fn edges_between(&self, source: &str, target: &str) -> Vec<&Edge> {
        self.edges
            .iter()
            .filter(|e| e.source == source && e.target == target)
            .collect()
    }

    /// Get child node IDs connected via Contains edges from `source_id`.
    ///
    /// Uses the pre-built contains_children_index for O(1) lookup.
    /// Falls back to edge scan if the index is empty (e.g., after mutation).
    pub fn contains_children(&self, source_id: &str) -> Vec<&str> {
        if let Some(children) = self.contains_children_index.get(source_id) {
            return children.iter().map(String::as_str).collect();
        }
        Vec::new()
    }

    /// Return all targets of Calls edges from `source_id`.
    ///
    /// O(E) scan over edges — acceptable for depth-limited traversals (max depth 3).
    /// If performance becomes an issue, add a calls_children_index analogous to
    /// contains_children_index.
    pub fn calls_children(&self, source_id: &str) -> Vec<&str> {
        self.edges
            .iter()
            .filter(|e| e.source == source_id && e.edge_type == EdgeType::Calls)
            .map(|e| e.target.as_str())
            .collect()
    }

    /// Return callers of a symbol — nodes with a Calls edge targeting `target_id`.
    pub fn calls_parents(&self, target_id: &str) -> Vec<&str> {
        self.edges
            .iter()
            .filter(|e| e.target == target_id && e.edge_type == EdgeType::Calls)
            .map(|e| e.source.as_str())
            .collect()
    }

    /// Return source location for a symbol node: (file_path, line_start, line_end).
    ///
    /// Returns `None` if the node doesn't exist or lacks location info.
    pub fn symbol_source_range(&self, node_id: &str) -> Option<(&str, usize, usize)> {
        let node = self.nodes.get(node_id)?;
        let file_path = node.file_path.as_deref()?;
        let start = node.line_start?;
        let end = node.line_end?;
        Some((file_path, start, end))
    }

    /// Get the max edge weight between two nodes. O(E) scan — prefer using
    /// `outgoing_edges_from` with pre-built index for bulk operations.
    pub fn max_edge_weight(&self, source: &str, target: &str) -> f64 {
        self.edges
            .iter()
            .filter(|e| e.source == source && e.target == target)
            .map(|e| e.weight)
            .fold(0.0_f64, f64::max)
    }

    /// Build a HashMap index: source_id -> Vec<(target_id, max_weight)>.
    ///
    /// Allows O(1) lookup of outgoing edges per node. Useful for graph
    /// attention propagation on large graphs.
    pub fn outgoing_edge_index(&self) -> HashMap<String, Vec<(String, f64)>> {
        let mut index: HashMap<String, HashMap<String, f64>> = HashMap::new();
        for edge in &self.edges {
            let entry = index
                .entry(edge.source.clone())
                .or_default()
                .entry(edge.target.clone())
                .or_insert(0.0);
            *entry = entry.max(edge.weight);
        }
        index
            .into_iter()
            .map(|(src, targets)| (src, targets.into_iter().collect()))
            .collect()
    }

    // --- Internal access for algorithms ----------------------------------

    /// Iterate over all node ids.
    pub fn node_ids(&self) -> impl Iterator<Item = &str> {
        self.nodes.keys().map(String::as_str)
    }

    /// Iterate over all edges.
    pub fn all_edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Mutable access to the edge list (for co-change weight updates).
    pub fn edges_mut(&mut self) -> &mut Vec<Edge> {
        &mut self.edges
    }

    /// Rebuild the contains_children_index from edges.
    ///
    /// Call this after loading a graph from disk (old serialized graphs
    /// may not have the index populated).
    pub fn rebuild_contains_index(&mut self) {
        self.contains_children_index.clear();
        for edge in &self.edges {
            if edge.edge_type == EdgeType::Contains {
                self.contains_children_index
                    .entry(edge.source.clone())
                    .or_default()
                    .push(edge.target.clone());
            }
        }
    }

    // --- Incremental mutation ------------------------------------------------

    /// Remove a file node and all its dependent nodes (symbols, imports, types,
    /// tests contained by this file) plus all edges touching these nodes.
    ///
    /// Returns the IDs of removed nodes.
    pub fn remove_file_and_dependents(&mut self, file_id: &str) -> Vec<String> {
        // Early return if the file node does not exist
        if !self.nodes.contains_key(file_id) {
            return Vec::new();
        }

        // 1. Collect dependent node IDs: targets of Contains edges from file_id
        //    Use the contains_children_index for O(1) lookup when available.
        let dependents: Vec<String> =
            if let Some(children) = self.contains_children_index.get(file_id) {
                children.clone()
            } else {
                self.edges
                    .iter()
                    .filter(|e| e.source == file_id && e.edge_type == EdgeType::Contains)
                    .map(|e| e.target.clone())
                    .collect()
            };

        // 2. Build the full set of IDs to remove (file + dependents)
        let mut removed_set: std::collections::HashSet<String> =
            dependents.iter().cloned().collect();
        removed_set.insert(file_id.to_string());

        // 3. Remove nodes
        for id in &removed_set {
            self.nodes.remove(id);
        }

        // 4. Remove all edges touching any removed node
        self.edges
            .retain(|e| !removed_set.contains(&e.source) && !removed_set.contains(&e.target));

        // 5. Rebuild adjacency maps and contains index from remaining edges
        self.adjacency.clear();
        self.reverse_adjacency.clear();
        self.contains_children_index.clear();

        // Ensure all remaining nodes have adjacency entries
        for id in self.nodes.keys() {
            self.adjacency.entry(id.clone()).or_default();
            self.reverse_adjacency.entry(id.clone()).or_default();
        }

        // Populate from edges
        for edge in &self.edges {
            self.adjacency
                .entry(edge.source.clone())
                .or_default()
                .push(edge.target.clone());
            self.reverse_adjacency
                .entry(edge.target.clone())
                .or_default()
                .push(edge.source.clone());

            if edge.edge_type == EdgeType::Contains {
                self.contains_children_index
                    .entry(edge.source.clone())
                    .or_default()
                    .push(edge.target.clone());
            }

            // Ensure both endpoints exist in both maps
            self.adjacency.entry(edge.target.clone()).or_default();
            self.reverse_adjacency
                .entry(edge.source.clone())
                .or_default();
        }

        // 6. Return the list of removed IDs
        removed_set.into_iter().collect()
    }

    /// Remove a node by ID and all edges touching it.
    /// Does NOT remove dependent nodes (use `remove_file_and_dependents` for files).
    pub fn remove_node(&mut self, node_id: &str) {
        self.nodes.remove(node_id);
        self.edges
            .retain(|e| e.source != node_id && e.target != node_id);

        // Clean adjacency
        self.adjacency.remove(node_id);
        self.reverse_adjacency.remove(node_id);
        for targets in self.adjacency.values_mut() {
            targets.retain(|t| t != node_id);
        }
        for sources in self.reverse_adjacency.values_mut() {
            sources.retain(|s| s != node_id);
        }

        // Clean contains index
        self.contains_children_index.remove(node_id);
        for children in self.contains_children_index.values_mut() {
            children.retain(|c| c != node_id);
        }
    }

    // --- Symbol-level hashing (S3-T1) ------------------------------------

    /// Compute a content hash for a single symbol node.
    ///
    /// Hash is based on: name + signature + doc (semantic content, not metadata).
    /// Used for fine-grained invalidation — if the hash hasn't changed, the
    /// symbol's wiki content doesn't need regeneration.
    pub fn symbol_content_hash(node: &Node) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(node.name.as_bytes());
        if let Some(ref sig) = node.signature {
            hasher.update(sig.as_bytes());
        }
        if let Some(ref doc) = node.doc {
            hasher.update(doc.as_bytes());
        }
        hasher.finalize().to_hex()[..16].to_string()
    }

    /// Compute content hashes for all symbol nodes in the graph.
    ///
    /// Returns a map: node_id → content_hash.
    /// Use this to detect which symbols changed between graph builds.
    pub fn compute_symbol_hashes(&self) -> HashMap<String, String> {
        self.nodes
            .iter()
            .filter(|(_, node)| matches!(node.node_type, NodeType::Symbol))
            .map(|(id, node)| (id.clone(), Self::symbol_content_hash(node)))
            .collect()
    }

    /// Compute a content hash for a community (aggregate of member symbol hashes).
    ///
    /// If the community hash hasn't changed, its wiki page doesn't need regeneration.
    /// Returns None if the community has no symbol nodes.
    pub fn community_content_hash(&self, node_ids: &[String]) -> Option<String> {
        let mut member_hashes: Vec<String> = node_ids
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .filter(|node| matches!(node.node_type, NodeType::Symbol))
            .map(|node| Self::symbol_content_hash(node))
            .collect();

        if member_hashes.is_empty() {
            return None;
        }

        // Sort for determinism (node order in community may vary)
        member_hashes.sort();

        let mut hasher = blake3::Hasher::new();
        for h in &member_hashes {
            hasher.update(h.as_bytes());
        }
        Some(hasher.finalize().to_hex()[..16].to_string())
    }

    // --- File-level graph traversal (File Retriever support) ---

    /// Find neighboring files reachable from a file via specific edge types.
    ///
    /// Follows: file → symbols (Contains) → neighbors (edge_types) → parent file.
    /// Returns file paths (not node IDs), excluding the seed file.
    /// Limited to `max` results to prevent explosion in dense graphs.
    pub fn file_neighbors(
        &self,
        file_id: &str,
        edge_types: &[EdgeType],
        max: usize,
    ) -> Vec<String> {
        let mut result_files: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Get symbols contained in this file
        for child_id in self.contains_children(file_id) {
            // Follow specified edge types from each symbol
            for neighbor_id in self.neighbors(child_id) {
                if let Some(neighbor) = self.get_node(neighbor_id) {
                    // Check if the edge type matches
                    let has_matching_edge = self.all_edges().iter().any(|e| {
                        (e.source == *child_id && e.target == *neighbor_id
                            || e.source == *neighbor_id && e.target == *child_id)
                            && edge_types.contains(&e.edge_type)
                    });

                    if !has_matching_edge {
                        continue;
                    }

                    // Find the file that contains this neighbor
                    let target_file = if let Some(ref fp) = neighbor.file_path {
                        fp.clone()
                    } else {
                        continue;
                    };

                    let target_file_id = format!("file:{}", target_file);
                    if target_file_id != file_id && seen.insert(target_file.clone()) {
                        result_files.push(target_file);
                        if result_files.len() >= max {
                            return result_files;
                        }
                    }
                }
            }
        }

        result_files
    }

    /// Find test files that test symbols in the given file.
    ///
    /// Follows reverse Tests edges: file → symbols ← test nodes → test file.
    pub fn test_files_for(&self, file_id: &str) -> Vec<String> {
        let mut tests: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for child_id in self.contains_children(file_id) {
            // Reverse: who tests this symbol?
            for src_id in self.reverse_neighbors(child_id) {
                if let Some(src_node) = self.get_node(src_id) {
                    if src_node.node_type == NodeType::Test {
                        if let Some(ref fp) = src_node.file_path {
                            if seen.insert(fp.clone()) {
                                tests.push(fp.clone());
                            }
                        }
                    }
                }
            }
        }
        tests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_symbol_node(id: &str, name: &str, file: &str, line_start: usize, line_end: usize) -> Node {
        Node {
            id: id.to_string(),
            node_type: NodeType::Symbol,
            name: name.to_string(),
            file_path: Some(file.to_string()),
            signature: Some(format!("fn {name}()")),
            kind: Some(SymbolKind::Function),
            line_start: Some(line_start),
            line_end: Some(line_end),
            last_modified: 0.0,
            doc: None,
        }
    }

    fn make_file_node(path: &str) -> Node {
        Node {
            id: format!("file:{path}"),
            node_type: NodeType::File,
            name: path.to_string(),
            file_path: Some(path.to_string()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        }
    }

    #[test]
    fn calls_children_returns_only_calls_targets() {
        let mut g = CodeGraph::new();
        g.add_node(make_symbol_node("fn:foo", "foo", "a.rs", 1, 10));
        g.add_node(make_symbol_node("fn:bar", "bar", "a.rs", 12, 20));
        g.add_node(make_symbol_node("fn:baz", "baz", "b.rs", 1, 10));

        // foo calls bar (Calls edge).
        g.add_edge(Edge {
            source: "fn:foo".into(),
            target: "fn:bar".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        // foo imports baz (Imports edge — should NOT appear in calls_children).
        g.add_edge(Edge {
            source: "fn:foo".into(),
            target: "fn:baz".into(),
            edge_type: EdgeType::Imports,
            weight: 1.0,
        });

        let callees = g.calls_children("fn:foo");
        assert_eq!(callees, vec!["fn:bar"]);
    }

    #[test]
    fn calls_children_returns_empty_for_leaf() {
        let mut g = CodeGraph::new();
        g.add_node(make_symbol_node("fn:leaf", "leaf", "a.rs", 1, 5));
        let callees = g.calls_children("fn:leaf");
        assert!(callees.is_empty());
    }

    #[test]
    fn calls_parents_returns_callers() {
        let mut g = CodeGraph::new();
        g.add_node(make_symbol_node("fn:a", "a", "a.rs", 1, 5));
        g.add_node(make_symbol_node("fn:b", "b", "b.rs", 1, 5));
        g.add_node(make_symbol_node("fn:target", "target", "c.rs", 1, 10));

        g.add_edge(Edge {
            source: "fn:a".into(),
            target: "fn:target".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "fn:b".into(),
            target: "fn:target".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        let mut callers = g.calls_parents("fn:target");
        callers.sort();
        assert_eq!(callers, vec!["fn:a", "fn:b"]);
    }

    #[test]
    fn symbol_source_range_returns_file_and_lines() {
        let mut g = CodeGraph::new();
        g.add_node(make_symbol_node("fn:foo", "foo", "src/lib.rs", 10, 25));

        let range = g.symbol_source_range("fn:foo");
        assert_eq!(range, Some(("src/lib.rs", 10, 25)));
    }

    #[test]
    fn symbol_source_range_none_for_missing_node() {
        let g = CodeGraph::new();
        assert_eq!(g.symbol_source_range("nonexistent"), None);
    }

    #[test]
    fn symbol_source_range_none_for_file_node() {
        let mut g = CodeGraph::new();
        g.add_node(make_file_node("src/lib.rs"));
        // File nodes have no line_start/line_end.
        assert_eq!(g.symbol_source_range("file:src/lib.rs"), None);
    }
}
