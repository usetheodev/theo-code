//! Deterministic wiki generator: CodeGraph → Vec<WikiDoc>.
//!
//! Zero LLM cost. All data derived from graph structure.
//! Provenance tracked for every claim.

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType, SymbolKind};

use super::model::*;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate complete wiki from communities + graph.
pub fn generate_wiki(
    communities: &[Community],
    graph: &CodeGraph,
    project_name: &str,
) -> Wiki {
    // Build file → community slug reverse map
    let file_to_community = build_file_community_map(communities, graph);

    let mut docs: Vec<WikiDoc> = communities
        .iter()
        .filter(|c| !c.node_ids.is_empty())
        .map(|c| generate_doc(c, graph, communities, &file_to_community))
        .collect();

    // Sort by file count descending (largest modules first)
    docs.sort_by(|a, b| b.file_count.cmp(&a.file_count));

    let now = chrono_now();
    let graph_hash = compute_graph_hash(graph);

    Wiki {
        manifest: WikiManifest {
            schema_version: WikiManifest::SCHEMA_VERSION,
            generator_version: WikiManifest::GENERATOR_VERSION.to_string(),
            graph_hash,
            generated_at: now,
            page_count: docs.len(),
        },
        docs,
    }
}

// ---------------------------------------------------------------------------
// Per-document generation
// ---------------------------------------------------------------------------

fn generate_doc(
    community: &Community,
    graph: &CodeGraph,
    _all_communities: &[Community],
    file_to_community: &HashMap<String, String>,
) -> WikiDoc {
    let slug = slugify(&community.name);
    let member_ids: HashSet<&str> = community.node_ids.iter().map(|s| s.as_str()).collect();

    // Collect files and symbols
    let mut files = Vec::new();
    let mut all_symbols: Vec<ApiEntry> = Vec::new();
    let mut source_refs = Vec::new();
    let mut lang_counts: HashMap<String, usize> = HashMap::new();

    for node_id in &community.node_ids {
        let Some(node) = graph.get_node(node_id) else { continue };
        if node.node_type != NodeType::File { continue; }

        let file_path = node.file_path.as_deref().unwrap_or(&node.name);
        let sr = SourceRef::file(file_path);
        source_refs.push(sr.clone());

        // Detect language from extension
        let ext = file_path.rsplit('.').next().unwrap_or("unknown");
        *lang_counts.entry(ext.to_string()).or_default() += 1;

        // Collect symbols from this file
        let children = graph.contains_children(node_id);
        let mut file_symbols = 0;

        for child_id in &children {
            let Some(child) = graph.get_node(child_id) else { continue };
            if child.node_type != NodeType::Symbol { continue; }
            file_symbols += 1;

            let kind = match &child.kind {
                Some(SymbolKind::Function) => "Function",
                Some(SymbolKind::Method) => "Method",
                Some(SymbolKind::Struct) => "Struct",
                Some(SymbolKind::Trait) | Some(SymbolKind::Interface) => "Trait",
                Some(SymbolKind::Enum) => "Enum",
                Some(SymbolKind::Class) => "Class",
                Some(SymbolKind::Constant) => "Constant",
                Some(SymbolKind::Module) => "Module",
                _ => "Symbol",
            };

            if let Some(sig) = &child.signature {
                let api = ApiEntry {
                    name: child.name.clone(),
                    signature: sig.clone(),
                    doc: child.doc.as_ref().and_then(|d| d.lines().next().map(|l| l.to_string())),
                    kind: kind.to_string(),
                    source_ref: SourceRef::symbol(
                        file_path,
                        &child.name,
                        child.line_start,
                        child.line_end,
                    ),
                };
                all_symbols.push(api);
            }
        }

        files.push(FileEntry {
            path: file_path.to_string(),
            symbol_count: file_symbols,
            source_ref: sr,
        });
    }

    // Primary language
    let primary_language = lang_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(lang, _)| lang)
        .unwrap_or_else(|| "unknown".to_string());

    // Entry points: symbols with no incoming Calls from within the community
    let entry_points = find_entry_points(&member_ids, &all_symbols, graph);

    // Public API: top symbols by signature presence (limit 15)
    let public_api: Vec<ApiEntry> = all_symbols.iter().take(15).cloned().collect();

    // Cross-community dependencies
    let dependencies = find_cross_deps(&member_ids, graph, file_to_community);

    // Call flow (2-hop BFS over Calls edges)
    let call_flow = find_call_flow(&member_ids, graph, 2);

    // Test coverage
    let test_coverage = compute_test_coverage(&member_ids, graph);

    WikiDoc {
        slug,
        title: community.name.clone(),
        community_id: community.id.clone(),
        file_count: files.len(),
        symbol_count: all_symbols.len(),
        primary_language,
        files,
        entry_points,
        public_api,
        dependencies,
        call_flow,
        test_coverage,
        source_refs,
        generated_at: chrono_now(),
        enriched: false,
    }
}

// ---------------------------------------------------------------------------
// Section generators
// ---------------------------------------------------------------------------

/// Entry points: symbols with no incoming Calls from within the community.
/// These are the "doors" into the module — the most important public API.
fn find_entry_points(
    member_ids: &HashSet<&str>,
    all_symbols: &[ApiEntry],
    graph: &CodeGraph,
) -> Vec<ApiEntry> {
    let mut entry_points = Vec::new();

    for sym in all_symbols {
        // Find the node ID for this symbol
        let candidates = graph.nodes_by_name(&sym.name);
        for candidate_id in candidates {
            let reverse = graph.reverse_neighbors(candidate_id);
            // Entry point = no callers from WITHIN the community
            let internal_callers = reverse.iter().filter(|r| {
                if let Some(node) = graph.get_node(r) {
                    // Check if caller is in this community AND it's a Calls edge
                    member_ids.contains(node.id.as_str())
                        && graph.edges_between(r, candidate_id).iter().any(|e| e.edge_type == EdgeType::Calls)
                } else {
                    false
                }
            }).count();

            if internal_callers == 0 {
                entry_points.push(sym.clone());
                break;
            }
        }
    }

    // Limit to top 5 entry points
    entry_points.truncate(5);
    entry_points
}

/// Call flow: BFS over Calls edges, limited to max_depth hops.
fn find_call_flow(
    member_ids: &HashSet<&str>,
    graph: &CodeGraph,
    max_depth: usize,
) -> Vec<FlowStep> {
    let mut steps = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();

    for node_id in member_ids {
        if visited.contains(*node_id) { continue; }
        let Some(node) = graph.get_node(node_id) else { continue; };
        if node.node_type != NodeType::Symbol { continue; }

        // BFS from this symbol
        let mut queue = vec![(node_id.to_string(), 0usize)];
        while let Some((current, depth)) = queue.pop() {
            if depth >= max_depth { continue; }
            if !visited.insert(current.clone()) { continue; }

            for neighbor_id in graph.neighbors(&current) {
                let Some(neighbor) = graph.get_node(neighbor_id) else { continue };
                if neighbor.node_type != NodeType::Symbol { continue; }

                // Only follow Calls edges
                let has_call = graph.edges_between(&current, neighbor_id)
                    .iter()
                    .any(|e| e.edge_type == EdgeType::Calls);

                if has_call {
                    let from_name = graph.get_node(&current)
                        .map(|n| n.name.clone())
                        .unwrap_or_default();

                    steps.push(FlowStep {
                        from_symbol: from_name,
                        to_symbol: neighbor.name.clone(),
                        edge_type: "Calls".to_string(),
                        source_ref: SourceRef::symbol(
                            neighbor.file_path.as_deref().unwrap_or(""),
                            &neighbor.name,
                            neighbor.line_start,
                            neighbor.line_end,
                        ),
                    });

                    if member_ids.contains(neighbor_id) {
                        queue.push((neighbor_id.to_string(), depth + 1));
                    }
                }
            }
        }

        // Limit steps per page
        if steps.len() > 20 { break; }
    }

    steps.truncate(20);
    steps
}

/// Cross-community dependencies via Imports/Calls/TypeDepends edges.
fn find_cross_deps(
    member_ids: &HashSet<&str>,
    graph: &CodeGraph,
    file_to_community: &HashMap<String, String>,
) -> Vec<DepEntry> {
    let mut deps: HashMap<String, (String, String)> = HashMap::new(); // slug → (name, edge_type)

    for edge in graph.all_edges() {
        if !member_ids.contains(edge.source.as_str()) { continue; }
        if member_ids.contains(edge.target.as_str()) { continue; }

        let edge_type = match edge.edge_type {
            EdgeType::Imports => "Imports",
            EdgeType::Calls => "Calls",
            EdgeType::TypeDepends => "TypeDepends",
            _ => continue,
        };

        // Find which community the target belongs to
        if let Some(target_node) = graph.get_node(&edge.target) {
            let target_path = target_node.file_path.as_deref().unwrap_or(&target_node.name);
            if let Some(target_slug) = file_to_community.get(target_path) {
                deps.entry(target_slug.clone())
                    .or_insert_with(|| (target_slug.clone(), edge_type.to_string()));
            }
        }
    }

    deps.into_iter()
        .map(|(slug, (name, edge_type))| DepEntry {
            target_slug: slug,
            target_name: name,
            edge_type,
        })
        .collect()
}

/// Test coverage: count symbols covered by Tests edges.
fn compute_test_coverage(
    member_ids: &HashSet<&str>,
    graph: &CodeGraph,
) -> TestCoverage {
    let mut total = 0;
    let mut tested_set: HashSet<String> = HashSet::new();
    let mut all_symbols: Vec<String> = Vec::new();

    for node_id in member_ids {
        let Some(node) = graph.get_node(node_id) else { continue };
        if node.node_type != NodeType::Symbol { continue; }
        if !matches!(node.kind, Some(SymbolKind::Function) | Some(SymbolKind::Method)) {
            continue;
        }

        total += 1;
        all_symbols.push(node.name.clone());

        // Check if any Tests edge targets this symbol
        for rev_id in graph.reverse_neighbors(node_id) {
            let has_test = graph.edges_between(rev_id, node_id)
                .iter()
                .any(|e| e.edge_type == EdgeType::Tests);
            if has_test {
                tested_set.insert(node.name.clone());
                break;
            }
        }
    }

    let tested = tested_set.len();
    let percentage = if total > 0 { (tested as f64 / total as f64) * 100.0 } else { 0.0 };
    let untested: Vec<String> = all_symbols
        .into_iter()
        .filter(|name| !tested_set.contains(name))
        .take(10) // Limit to 10 untested for readability
        .collect();

    TestCoverage { tested, total, percentage, untested }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build file_path → community_slug map.
fn build_file_community_map(
    communities: &[Community],
    graph: &CodeGraph,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for community in communities {
        let slug = slugify(&community.name);
        for node_id in &community.node_ids {
            if let Some(node) = graph.get_node(node_id) {
                if let Some(fp) = &node.file_path {
                    map.insert(fp.clone(), slug.clone());
                }
            }
        }
    }
    map
}

/// Slugify a community name for use as filename.
pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "-")
        .replace("--", "-")
        .trim_matches('-')
        .to_string()
}

/// Compute graph hash for cache invalidation.
pub fn compute_graph_hash(graph: &CodeGraph) -> u64 {
    use std::collections::BTreeMap;
    use std::hash::{Hash, Hasher};

    let mut file_info: BTreeMap<String, u64> = BTreeMap::new();
    for node in graph.file_nodes() {
        let path = node.file_path.as_deref().unwrap_or(&node.name);
        file_info.insert(path.to_string(), node.last_modified.to_bits());
    }

    let mut hasher = std::hash::DefaultHasher::new();
    for (path, mtime) in &file_info {
        path.hash(&mut hasher);
        mtime.hash(&mut hasher);
    }
    hasher.finish()
}

/// Current timestamp as ISO 8601.
fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", now)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::model::{Node, Edge};

    fn test_graph() -> (CodeGraph, Vec<Community>) {
        let mut graph = CodeGraph::new();

        // File: auth.rs
        graph.add_node(Node {
            id: "file:auth.rs".into(), name: "auth.rs".into(),
            node_type: NodeType::File, file_path: Some("src/auth.rs".into()),
            signature: None, doc: None, kind: None,
            line_start: None, line_end: None, last_modified: 100.0,
        });
        graph.add_node(Node {
            id: "sym:verify".into(), name: "verify_token".into(),
            node_type: NodeType::Symbol, file_path: Some("src/auth.rs".into()),
            signature: Some("pub fn verify_token(t: &str) -> bool".into()),
            doc: Some("Verify JWT token".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10), line_end: Some(30), last_modified: 100.0,
        });
        graph.add_edge(Edge {
            source: "file:auth.rs".into(), target: "sym:verify".into(),
            edge_type: EdgeType::Contains, weight: 1.0,
        });

        // File: handler.rs (different community)
        graph.add_node(Node {
            id: "file:handler.rs".into(), name: "handler.rs".into(),
            node_type: NodeType::File, file_path: Some("src/handler.rs".into()),
            signature: None, doc: None, kind: None,
            line_start: None, line_end: None, last_modified: 200.0,
        });
        graph.add_node(Node {
            id: "sym:handle".into(), name: "handle_request".into(),
            node_type: NodeType::Symbol, file_path: Some("src/handler.rs".into()),
            signature: Some("pub fn handle_request(req: Request) -> Response".into()),
            doc: None, kind: Some(SymbolKind::Function),
            line_start: Some(5), line_end: Some(20), last_modified: 200.0,
        });
        graph.add_edge(Edge {
            source: "file:handler.rs".into(), target: "sym:handle".into(),
            edge_type: EdgeType::Contains, weight: 1.0,
        });

        // handler calls verify (cross-community)
        graph.add_edge(Edge {
            source: "sym:handle".into(), target: "sym:verify".into(),
            edge_type: EdgeType::Calls, weight: 1.0,
        });

        let communities = vec![
            Community {
                id: "c1".into(), name: "auth".into(),
                node_ids: vec!["file:auth.rs".into(), "sym:verify".into()],
                level: 0, parent_id: None, version: 0,
            },
            Community {
                id: "c2".into(), name: "handler".into(),
                node_ids: vec!["file:handler.rs".into(), "sym:handle".into()],
                level: 0, parent_id: None, version: 0,
            },
        ];

        (graph, communities)
    }

    #[test]
    fn generate_wiki_produces_pages() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test-project");
        assert_eq!(wiki.docs.len(), 2);
        assert_eq!(wiki.manifest.page_count, 2);
    }

    #[test]
    fn wiki_doc_has_provenance() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");
        let auth = wiki.docs.iter().find(|d| d.slug == "auth").unwrap();
        assert!(!auth.source_refs.is_empty());
        assert_eq!(auth.source_refs[0].file_path, "src/auth.rs");
    }

    #[test]
    fn wiki_doc_has_files() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");
        let auth = wiki.docs.iter().find(|d| d.slug == "auth").unwrap();
        assert_eq!(auth.files.len(), 1);
        assert_eq!(auth.files[0].path, "src/auth.rs");
    }

    #[test]
    fn wiki_doc_has_public_api() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");
        let auth = wiki.docs.iter().find(|d| d.slug == "auth").unwrap();
        assert!(!auth.public_api.is_empty());
        assert!(auth.public_api[0].signature.contains("verify_token"));
    }

    #[test]
    fn cross_community_deps_detected() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");
        let handler = wiki.docs.iter().find(|d| d.slug == "handler").unwrap();
        // handler calls verify_token in auth → dependency on auth
        assert!(handler.dependencies.iter().any(|d| d.target_slug == "auth"));
    }

    #[test]
    fn slugify_works() {
        assert_eq!(slugify("My Module"), "my-module");
        assert_eq!(slugify("auth/jwt"), "auth-jwt");
        assert_eq!(slugify("theo-engine-graph (42)"), "theo-engine-graph-42");
    }

    #[test]
    fn empty_community() {
        let graph = CodeGraph::new();
        let communities = vec![Community {
            id: "empty".into(), name: "empty".into(),
            node_ids: vec![], level: 0, parent_id: None, version: 0,
        }];
        let wiki = generate_wiki(&communities, &graph, "test");
        assert_eq!(wiki.docs.len(), 0); // Empty community filtered out
    }

    #[test]
    fn graph_hash_deterministic() {
        let (graph, _) = test_graph();
        let h1 = compute_graph_hash(&graph);
        let h2 = compute_graph_hash(&graph);
        assert_eq!(h1, h2);
    }
}
