/// Bridge between `intently_core` (theo-deep-code) and `theo-code-graph`.
///
/// Converts an `ExtractionResult` from Intently's CodeModel into a populated
/// `CodeGraph` with all node types and edge types from the GRAPHCTX spec.
///
/// This module does NOT depend on `intently_core` directly — instead it
/// defines a set of intermediate DTOs that the orchestrator populates from
/// Intently's output. This keeps the graph crate decoupled from the parser.
use crate::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Intermediate DTOs — orchestrator fills these from Intently output
// ---------------------------------------------------------------------------

/// A file with its extracted symbols, imports, and references.
#[derive(Debug, Clone)]
pub struct FileData {
    /// Relative path from repo root (e.g. "src/auth/jwt.rs")
    pub path: String,
    /// Language identifier
    pub language: String,
    /// Line count
    pub line_count: usize,
    /// Last modified timestamp (unix seconds)
    pub last_modified: f64,
    /// Symbols defined in this file
    pub symbols: Vec<SymbolData>,
    /// Import statements in this file
    pub imports: Vec<ImportData>,
    /// Cross-symbol references (calls, extends, type usage)
    pub references: Vec<ReferenceData>,
    /// Data models (structs, classes, interfaces)
    pub data_models: Vec<DataModelData>,
}

/// A symbol extracted from source code.
#[derive(Debug, Clone)]
pub struct SymbolData {
    /// Fully qualified name (e.g. "auth::jwt::verify_token")
    pub qualified_name: String,
    /// Short name
    pub name: String,
    pub kind: SymbolKindDto,
    pub line_start: usize,
    pub line_end: usize,
    pub signature: Option<String>,
    pub is_test: bool,
    pub parent: Option<String>,
    /// Docstring/doc comment text.
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKindDto {
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

impl From<SymbolKindDto> for SymbolKind {
    fn from(dto: SymbolKindDto) -> Self {
        match dto {
            SymbolKindDto::Function => SymbolKind::Function,
            SymbolKindDto::Method => SymbolKind::Method,
            SymbolKindDto::Class => SymbolKind::Class,
            SymbolKindDto::Struct => SymbolKind::Struct,
            SymbolKindDto::Enum => SymbolKind::Enum,
            SymbolKindDto::Constant => SymbolKind::Constant,
            SymbolKindDto::Trait => SymbolKind::Trait,
            SymbolKindDto::Interface => SymbolKind::Interface,
            SymbolKindDto::Module => SymbolKind::Module,
        }
    }
}

/// An import statement.
#[derive(Debug, Clone)]
pub struct ImportData {
    /// Import source (e.g. "std::collections::HashMap", "./auth")
    pub source: String,
    /// Imported names
    pub specifiers: Vec<String>,
    pub line: usize,
}

/// A reference between symbols.
#[derive(Debug, Clone)]
pub struct ReferenceData {
    /// Source symbol qualified name (caller)
    pub source_symbol: String,
    /// Source file path
    pub source_file: String,
    /// Target symbol qualified name (callee)
    pub target_symbol: String,
    /// Target file path (None if external/unresolved)
    pub target_file: Option<String>,
    /// Reference kind
    pub kind: ReferenceKindDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKindDto {
    Call,
    Extends,
    Implements,
    TypeUsage,
    Import,
}

/// A data model (struct, class, interface with fields).
#[derive(Debug, Clone)]
pub struct DataModelData {
    pub name: String,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub parent_type: Option<String>,
    pub implemented_interfaces: Vec<String>,
}

// ---------------------------------------------------------------------------
// Node ID generation — deterministic, stable across runs
// ---------------------------------------------------------------------------

/// Generate a stable file node ID from a relative path.
pub fn file_node_id(rel_path: &str) -> String {
    format!("file:{}", rel_path)
}

/// Generate a stable symbol node ID.
pub fn symbol_node_id(file_path: &str, qualified_name: &str) -> String {
    format!("sym:{}:{}", file_path, qualified_name)
}

/// Generate a stable import node ID.
pub fn import_node_id(file_path: &str, source: &str, line: usize) -> String {
    format!("imp:{}:{}:{}", file_path, source, line)
}

/// Generate a stable type node ID.
pub fn type_node_id(file_path: &str, type_name: &str) -> String {
    format!("type:{}:{}", file_path, type_name)
}

/// Generate a stable test node ID.
pub fn test_node_id(file_path: &str, test_name: &str) -> String {
    format!("test:{}:{}", file_path, test_name)
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Statistics from the bridge conversion.
#[derive(Debug, Clone, Default)]
pub struct BridgeStats {
    pub files: usize,
    pub symbols: usize,
    pub imports: usize,
    pub types: usize,
    pub tests: usize,
    pub edges_contains: usize,
    pub edges_calls: usize,
    pub edges_imports: usize,
    pub edges_inherits: usize,
    pub edges_type_depends: usize,
    pub edges_tests: usize,
    pub edges_references: usize,
}

impl BridgeStats {
    pub fn total_nodes(&self) -> usize {
        self.files + self.symbols + self.imports + self.types + self.tests
    }

    pub fn total_edges(&self) -> usize {
        self.edges_contains
            + self.edges_calls
            + self.edges_imports
            + self.edges_inherits
            + self.edges_type_depends
            + self.edges_tests
            + self.edges_references
    }
}

// ---------------------------------------------------------------------------
// Core conversion: Vec<FileData> → CodeGraph
// ---------------------------------------------------------------------------

/// Convert extracted file data into a populated CodeGraph.
///
/// This is the main entry point for the bridge. It:
/// 1. Creates File nodes for each file
/// 2. Creates Symbol nodes (with Contains edges)
/// 3. Creates Import nodes (with Imports edges)
/// 4. Creates Type nodes for data models (with TypeDepends edges)
/// 5. Creates Test nodes for test symbols (with Tests edges)
/// 6. Resolves References into Calls, Inherits, TypeDepends edges
///
/// Returns the populated graph and conversion statistics.
pub fn build_graph(files: &[FileData]) -> (CodeGraph, BridgeStats) {
    let mut graph = CodeGraph::new();
    let mut stats = BridgeStats::default();

    // Index: qualified_name → node_id for reference resolution
    let mut symbol_index: HashMap<String, String> = HashMap::new();
    // Index: short_name → Vec<node_id> for fuzzy resolution
    let mut name_index: HashMap<String, Vec<String>> = HashMap::new();

    // --- Pass 1: Create all nodes ---

    for file in files {
        let fid = file_node_id(&file.path);

        // File node
        graph.add_node(Node {
            id: fid.clone(),
            node_type: NodeType::File,
            name: file.path.clone(),
            file_path: Some(file.path.clone()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: file.last_modified,
            doc: None,
        });
        stats.files += 1;

        // Symbol nodes
        for sym in &file.symbols {
            let is_test = sym.is_test;

            if is_test {
                // Test node
                let tid = test_node_id(&file.path, &sym.qualified_name);
                graph.add_node(Node {
                    id: tid.clone(),
                    node_type: NodeType::Test,
                    name: sym.name.clone(),
                    file_path: Some(file.path.clone()),
                    signature: sym.signature.clone(),
                    kind: Some(sym.kind.into()),
                    line_start: Some(sym.line_start),
                    line_end: Some(sym.line_end),
                    last_modified: file.last_modified,
                    doc: sym.doc.clone(),
                });
                stats.tests += 1;

                // Contains edge: file → test
                graph.add_edge(Edge {
                    source: fid.clone(),
                    target: tid.clone(),
                    edge_type: EdgeType::Contains,
                    weight: EdgeType::Contains.default_weight(),
                });
                stats.edges_contains += 1;

                symbol_index.insert(sym.qualified_name.clone(), tid.clone());
                name_index.entry(sym.name.clone()).or_default().push(tid);
            } else {
                // Regular symbol node
                let sid = symbol_node_id(&file.path, &sym.qualified_name);
                graph.add_node(Node {
                    id: sid.clone(),
                    node_type: NodeType::Symbol,
                    name: sym.name.clone(),
                    file_path: Some(file.path.clone()),
                    signature: sym.signature.clone(),
                    kind: Some(sym.kind.into()),
                    line_start: Some(sym.line_start),
                    line_end: Some(sym.line_end),
                    last_modified: file.last_modified,
                    doc: sym.doc.clone(),
                });
                stats.symbols += 1;

                // Contains edge: file → symbol
                graph.add_edge(Edge {
                    source: fid.clone(),
                    target: sid.clone(),
                    edge_type: EdgeType::Contains,
                    weight: EdgeType::Contains.default_weight(),
                });
                stats.edges_contains += 1;

                symbol_index.insert(sym.qualified_name.clone(), sid.clone());
                name_index.entry(sym.name.clone()).or_default().push(sid);
            }
        }

        // Import nodes
        for imp in &file.imports {
            let iid = import_node_id(&file.path, &imp.source, imp.line);
            graph.add_node(Node {
                id: iid.clone(),
                node_type: NodeType::Import,
                name: imp.source.clone(),
                file_path: Some(file.path.clone()),
                signature: None,
                kind: None,
                line_start: Some(imp.line),
                line_end: Some(imp.line),
                last_modified: file.last_modified,
                doc: None,
            });
            stats.imports += 1;

            // Imports edge: file → import
            graph.add_edge(Edge {
                source: fid.clone(),
                target: iid.clone(),
                edge_type: EdgeType::Imports,
                weight: EdgeType::Imports.default_weight(),
            });
            stats.edges_imports += 1;
        }

        // Type nodes (from data models)
        for dm in &file.data_models {
            let tid = type_node_id(&file.path, &dm.name);
            graph.add_node(Node {
                id: tid.clone(),
                node_type: NodeType::Type,
                name: dm.name.clone(),
                file_path: Some(file.path.clone()),
                signature: None,
                kind: None,
                line_start: Some(dm.line_start),
                line_end: Some(dm.line_end),
                last_modified: file.last_modified,
                doc: None,
            });
            stats.types += 1;

            // Contains edge: file → type
            graph.add_edge(Edge {
                source: fid.clone(),
                target: tid.clone(),
                edge_type: EdgeType::Contains,
                weight: EdgeType::Contains.default_weight(),
            });
            stats.edges_contains += 1;

            // Register in symbol_index for inheritance resolution
            symbol_index.insert(dm.name.clone(), tid.clone());
            name_index.entry(dm.name.clone()).or_default().push(tid);
        }
    }

    // --- Pass 2: Resolve references into edges ---

    for file in files {
        for reference in &file.references {
            let source_id = resolve_symbol(
                &reference.source_symbol,
                &reference.source_file,
                &symbol_index,
                &name_index,
            );

            let target_id = match &reference.target_file {
                Some(tf) => {
                    resolve_symbol(&reference.target_symbol, tf, &symbol_index, &name_index)
                }
                None => resolve_by_name(&reference.target_symbol, &symbol_index, &name_index),
            };

            if let (Some(src), Some(tgt)) = (source_id, target_id) {
                if src == tgt {
                    continue; // skip self-references
                }

                let (edge_type, weight) = match reference.kind {
                    ReferenceKindDto::Call => {
                        stats.edges_calls += 1;
                        (EdgeType::Calls, EdgeType::Calls.default_weight())
                    }
                    ReferenceKindDto::Extends | ReferenceKindDto::Implements => {
                        stats.edges_inherits += 1;
                        (EdgeType::Inherits, EdgeType::Inherits.default_weight())
                    }
                    ReferenceKindDto::TypeUsage => {
                        stats.edges_type_depends += 1;
                        (
                            EdgeType::TypeDepends,
                            EdgeType::TypeDepends.default_weight(),
                        )
                    }
                    ReferenceKindDto::Import => {
                        stats.edges_imports += 1;
                        (EdgeType::Imports, EdgeType::Imports.default_weight())
                    }
                };

                graph.add_edge(Edge {
                    source: src,
                    target: tgt,
                    edge_type,
                    weight,
                });
            }
        }
    }

    // --- Pass 3: Infer Tests edges ---
    // Test nodes that reference non-test symbols get a Tests edge.
    // We look at test nodes' references (Calls) to non-test symbols.

    for file in files {
        for sym in &file.symbols {
            if !sym.is_test {
                continue;
            }
            let test_id = test_node_id(&file.path, &sym.qualified_name);

            // Find all Call references from this test function
            for reference in &file.references {
                if reference.source_symbol != sym.qualified_name {
                    continue;
                }
                if reference.kind != ReferenceKindDto::Call {
                    continue;
                }

                let target_id = match &reference.target_file {
                    Some(tf) => {
                        resolve_symbol(&reference.target_symbol, tf, &symbol_index, &name_index)
                    }
                    None => resolve_by_name(&reference.target_symbol, &symbol_index, &name_index),
                };

                if let Some(tgt) = target_id {
                    // Only add Tests edge if target is a regular symbol (not a test)
                    if let Some(node) = graph.get_node(&tgt) {
                        if matches!(node.node_type, NodeType::Symbol) {
                            graph.add_edge(Edge {
                                source: test_id.clone(),
                                target: tgt,
                                edge_type: EdgeType::Tests,
                                weight: EdgeType::Tests.default_weight(),
                            });
                            stats.edges_tests += 1;
                        }
                    }
                }
            }
        }
    }

    // --- Pass 4: Inheritance edges from data models ---

    for file in files {
        for dm in &file.data_models {
            let dm_id = type_node_id(&file.path, &dm.name);

            if let Some(parent) = &dm.parent_type {
                if let Some(parent_id) = resolve_by_name(parent, &symbol_index, &name_index) {
                    graph.add_edge(Edge {
                        source: dm_id.clone(),
                        target: parent_id,
                        edge_type: EdgeType::Inherits,
                        weight: EdgeType::Inherits.default_weight(),
                    });
                    stats.edges_inherits += 1;
                }
            }

            for iface in &dm.implemented_interfaces {
                if let Some(iface_id) = resolve_by_name(iface, &symbol_index, &name_index) {
                    graph.add_edge(Edge {
                        source: dm_id.clone(),
                        target: iface_id,
                        edge_type: EdgeType::Inherits,
                        weight: EdgeType::Inherits.default_weight(),
                    });
                    stats.edges_inherits += 1;
                }
            }
        }
    }

    (graph, stats)
}

// ---------------------------------------------------------------------------
// Symbol resolution helpers
// ---------------------------------------------------------------------------

/// Try to resolve a symbol by qualified name in a specific file, then by name globally.
fn resolve_symbol(
    qualified_name: &str,
    file_path: &str,
    symbol_index: &HashMap<String, String>,
    name_index: &HashMap<String, Vec<String>>,
) -> Option<String> {
    // Try exact qualified name
    if let Some(id) = symbol_index.get(qualified_name) {
        return Some(id.clone());
    }

    // Try file-scoped: sym:{file}:{name}
    let file_scoped = symbol_node_id(file_path, qualified_name);
    if symbol_index.values().any(|v| v == &file_scoped) {
        return Some(file_scoped);
    }

    // Fall back to name-only resolution
    resolve_by_name(qualified_name, symbol_index, name_index)
}

/// Resolve a symbol by short name. If unique, return it. If ambiguous, return None.
fn resolve_by_name(
    name: &str,
    symbol_index: &HashMap<String, String>,
    name_index: &HashMap<String, Vec<String>>,
) -> Option<String> {
    // Try exact match in symbol_index first
    if let Some(id) = symbol_index.get(name) {
        return Some(id.clone());
    }

    // Try name_index — only if unambiguous (single match)
    if let Some(ids) = name_index.get(name) {
        if ids.len() == 1 {
            return Some(ids[0].clone());
        }
    }

    // Extract short name from qualified name (last segment)
    let short = name.rsplit("::").next().unwrap_or(name);
    if short != name {
        if let Some(id) = symbol_index.get(short) {
            return Some(id.clone());
        }
        if let Some(ids) = name_index.get(short) {
            if ids.len() == 1 {
                return Some(ids[0].clone());
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Convenience: build graph from a directory using simple file walking
// ---------------------------------------------------------------------------

/// Walk a directory and create FileData entries for all source files.
/// This is a lightweight alternative when Intently is not available.
/// It only creates File nodes (no symbol extraction).
/// Directories always excluded from graph indexing.
const EXCLUDED_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "vendor",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    ".next",
    ".nuxt",
];

pub fn walk_files(repo_root: &Path) -> Vec<FileData> {
    let walker = ignore::WalkBuilder::new(repo_root)
        .hidden(true)
        .git_ignore(true)
        .filter_entry(|entry| {
            if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                let name = entry.file_name().to_str().unwrap_or("");
                return !EXCLUDED_DIRS.contains(&name);
            }
            true
        })
        .build();

    let mut files = Vec::new();
    let known_extensions = [
        "rs", "py", "ts", "tsx", "js", "jsx", "go", "java", "kt", "cs", "rb", "php", "c", "cpp",
        "h", "hpp", "swift", "scala",
    ];

    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !known_extensions.contains(&ext) {
            continue;
        }

        let rel_path = path
            .strip_prefix(repo_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let line_count = std::fs::read_to_string(path)
            .map(|s| s.lines().count())
            .unwrap_or(0);

        let last_modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64()
            })
            .unwrap_or(0.0);

        files.push(FileData {
            path: rel_path,
            language: ext.to_string(),
            line_count,
            last_modified,
            symbols: Vec::new(),
            imports: Vec::new(),
            references: Vec::new(),
            data_models: Vec::new(),
        });
    }

    files
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_files() -> Vec<FileData> {
        vec![
            FileData {
                path: "src/auth/jwt.rs".into(),
                language: "rs".into(),
                line_count: 100,
                last_modified: 1000.0,
                symbols: vec![
                    SymbolData {
                        qualified_name: "auth::jwt::verify_token".into(),
                        name: "verify_token".into(),
                        kind: SymbolKindDto::Function,
                        line_start: 10,
                        line_end: 30,
                        signature: Some("fn verify_token(token: &str) -> Result<Claims>".into()),
                        is_test: false,
                        parent: None,
                        doc: None,
                    },
                    SymbolData {
                        qualified_name: "auth::jwt::decode_header".into(),
                        name: "decode_header".into(),
                        kind: SymbolKindDto::Function,
                        line_start: 35,
                        line_end: 50,
                        signature: Some("fn decode_header(token: &str) -> Header".into()),
                        is_test: false,
                        parent: None,
                        doc: None,
                    },
                ],
                imports: vec![ImportData {
                    source: "crypto::hmac".into(),
                    specifiers: vec!["Hmac".into(), "verify".into()],
                    line: 1,
                }],
                references: vec![ReferenceData {
                    source_symbol: "auth::jwt::verify_token".into(),
                    source_file: "src/auth/jwt.rs".into(),
                    target_symbol: "auth::jwt::decode_header".into(),
                    target_file: Some("src/auth/jwt.rs".into()),
                    kind: ReferenceKindDto::Call,
                }],
                data_models: vec![],
            },
            FileData {
                path: "src/crypto/hmac.rs".into(),
                language: "rs".into(),
                line_count: 50,
                last_modified: 900.0,
                symbols: vec![SymbolData {
                    qualified_name: "crypto::hmac::verify".into(),
                    name: "verify".into(),
                    kind: SymbolKindDto::Function,
                    line_start: 5,
                    line_end: 20,
                    signature: Some("fn verify(key: &[u8], msg: &[u8]) -> bool".into()),
                    is_test: false,
                    parent: None,
                    doc: None,
                }],
                imports: vec![],
                references: vec![],
                data_models: vec![],
            },
            FileData {
                path: "tests/test_jwt.rs".into(),
                language: "rs".into(),
                line_count: 30,
                last_modified: 1100.0,
                symbols: vec![SymbolData {
                    qualified_name: "test_jwt::test_verify_valid".into(),
                    name: "test_verify_valid".into(),
                    kind: SymbolKindDto::Function,
                    line_start: 5,
                    line_end: 20,
                    signature: Some("fn test_verify_valid()".into()),
                    is_test: true,
                    parent: None,
                    doc: None,
                }],
                imports: vec![],
                references: vec![ReferenceData {
                    source_symbol: "test_jwt::test_verify_valid".into(),
                    source_file: "tests/test_jwt.rs".into(),
                    target_symbol: "auth::jwt::verify_token".into(),
                    target_file: Some("src/auth/jwt.rs".into()),
                    kind: ReferenceKindDto::Call,
                }],
                data_models: vec![],
            },
        ]
    }

    #[test]
    fn test_build_graph_creates_file_nodes() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.files, 3);
        assert!(graph.get_node("file:src/auth/jwt.rs").is_some());
        assert!(graph.get_node("file:src/crypto/hmac.rs").is_some());
        assert!(graph.get_node("file:tests/test_jwt.rs").is_some());
    }

    #[test]
    fn test_build_graph_creates_symbol_nodes() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.symbols, 3); // verify_token, decode_header, verify
        let sym = graph
            .get_node("sym:src/auth/jwt.rs:auth::jwt::verify_token")
            .unwrap();
        assert_eq!(sym.node_type, NodeType::Symbol);
        assert_eq!(sym.name, "verify_token");
    }

    #[test]
    fn test_build_graph_creates_test_nodes() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.tests, 1);
        let test = graph
            .get_node("test:tests/test_jwt.rs:test_jwt::test_verify_valid")
            .unwrap();
        assert_eq!(test.node_type, NodeType::Test);
    }

    #[test]
    fn test_build_graph_creates_contains_edges() {
        let files = make_test_files();
        let (_, stats) = build_graph(&files);

        // 3 files: jwt(2 syms), hmac(1 sym), test(1 test) + 1 import node
        assert_eq!(stats.edges_contains, 4);
    }

    #[test]
    fn test_build_graph_resolves_call_edges() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert!(stats.edges_calls > 0);
        // verify_token calls decode_header
        let caller = "sym:src/auth/jwt.rs:auth::jwt::verify_token";
        let callee = "sym:src/auth/jwt.rs:auth::jwt::decode_header";
        let call_edges = graph.edges_between(caller, callee);
        assert!(
            call_edges.iter().any(|e| e.edge_type == EdgeType::Calls),
            "Expected Calls edge from verify_token to decode_header"
        );
    }

    #[test]
    fn test_build_graph_creates_tests_edges() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert!(stats.edges_tests > 0);
        let test_id = "test:tests/test_jwt.rs:test_jwt::test_verify_valid";
        let target = "sym:src/auth/jwt.rs:auth::jwt::verify_token";
        let test_edges = graph.edges_between(test_id, target);
        assert!(
            test_edges.iter().any(|e| e.edge_type == EdgeType::Tests),
            "Expected Tests edge from test to verify_token"
        );
    }

    #[test]
    fn test_build_graph_creates_import_nodes() {
        let files = make_test_files();
        let (_, stats) = build_graph(&files);

        assert_eq!(stats.imports, 1);
        assert_eq!(stats.edges_imports, 1);
    }

    #[test]
    fn test_build_graph_stats_totals() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.total_nodes(), graph.node_count());
        assert!(stats.total_edges() > 0);
    }

    #[test]
    fn test_file_node_id_format() {
        assert_eq!(file_node_id("src/main.rs"), "file:src/main.rs");
    }

    #[test]
    fn test_empty_files_produces_empty_graph() {
        let (graph, stats) = build_graph(&[]);
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(stats.total_nodes(), 0);
    }

    #[test]
    fn test_data_model_creates_type_node_and_inheritance() {
        let files = vec![FileData {
            path: "src/model.rs".into(),
            language: "rs".into(),
            line_count: 50,
            last_modified: 1000.0,
            symbols: vec![],
            imports: vec![],
            references: vec![],
            data_models: vec![
                DataModelData {
                    name: "BaseEntity".into(),
                    file_path: "src/model.rs".into(),
                    line_start: 1,
                    line_end: 10,
                    parent_type: None,
                    implemented_interfaces: vec![],
                },
                DataModelData {
                    name: "User".into(),
                    file_path: "src/model.rs".into(),
                    line_start: 15,
                    line_end: 30,
                    parent_type: Some("BaseEntity".into()),
                    implemented_interfaces: vec![],
                },
            ],
        }];

        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.types, 2);
        assert!(graph.get_node("type:src/model.rs:User").is_some());
        assert!(graph.get_node("type:src/model.rs:BaseEntity").is_some());

        // User inherits BaseEntity
        assert!(stats.edges_inherits > 0);
    }
}
