//! Deterministic wiki generator: CodeGraph → Vec<WikiDoc>.
//!
//! Zero LLM cost. All data derived from graph structure.
//! Provenance tracked for every claim.

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType, SymbolKind};

use crate::wiki::model::*;

use super::*;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate complete wiki from communities + graph.
///
/// `repo_root` is needed to scan source files for test coverage detection.
pub fn generate_wiki(communities: &[Community], graph: &CodeGraph, project_name: &str) -> Wiki {
    generate_wiki_with_root(communities, graph, project_name, None)
}

/// Generate wiki with explicit repo root for test file scanning.
pub fn generate_wiki_with_root(
    communities: &[Community],
    graph: &CodeGraph,
    _project_name: &str,
    repo_root: Option<&std::path::Path>,
) -> Wiki {
    // Build file → community slug reverse map
    let file_to_community = build_file_community_map(communities, graph);

    let mut docs: Vec<WikiDoc> = communities
        .iter()
        .filter(|c| !c.node_ids.is_empty())
        .map(|c| generate_doc(c, graph, communities, &file_to_community, repo_root))
        .filter(|d| d.file_count >= 2) // Filter single-file noise communities
        .collect();

    // Layer 1: Enrich with author metadata (Cargo.toml, //! docs, README)
    let project_dir = repo_root.unwrap_or_else(|| std::path::Path::new("."));
    let crate_meta = extract_crate_metadata(project_dir);

    for doc in &mut docs {
        if let Some(crate_name) = find_crate_for_doc(doc, &crate_meta)
            && let Some(meta) = crate_meta.get(&crate_name) {
                // Title: crate name (clean, no community ID)
                if let Some(ref name) = meta.name {
                    doc.title = name.clone();
                }
                // Summary: Cargo.toml description (highest authority)
                if let Some(ref desc) = meta.description {
                    doc.summary = desc.clone();
                    doc.crate_description = Some(desc.clone());
                }
                // Module doc: //! comments from lib.rs
                let crate_dir = if meta.crate_dir.is_empty() {
                    &crate_name
                } else {
                    &meta.crate_dir
                };
                if let Some(module_doc) = extract_module_doc(project_dir, crate_dir) {
                    doc.module_doc = Some(module_doc);
                }
                // README fallback for summary if no Cargo.toml description
                if doc.crate_description.is_none()
                    && let Some(readme) = extract_readme_summary(project_dir, crate_dir) {
                        doc.summary = readme.clone();
                        doc.crate_description = Some(readme);
                    }
            }
    }

    // Sort by file count descending (largest modules first)
    docs.sort_by_key(|doc| std::cmp::Reverse(doc.file_count));

    let now = chrono_now();
    let graph_hash = compute_graph_hash(graph);

    Wiki {
        manifest: WikiManifest {
            schema_version: WikiManifest::SCHEMA_VERSION,
            generator_version: WikiManifest::GENERATOR_VERSION.to_string(),
            graph_hash,
            generated_at: now,
            page_count: docs.len(),
            page_hashes: std::collections::HashMap::new(),
        },
        docs,
    }
}

// ---------------------------------------------------------------------------
// Per-document generation
// ---------------------------------------------------------------------------

pub(super) fn generate_doc(
    community: &Community,
    graph: &CodeGraph,
    _all_communities: &[Community],
    file_to_community: &HashMap<String, String>,
    repo_root: Option<&std::path::Path>,
) -> WikiDoc {
    let slug = slugify(&community.name);
    let member_ids: HashSet<&str> = community.node_ids.iter().map(|s| s.as_str()).collect();

    // Collect files and symbols
    let mut files = Vec::new();
    let mut all_symbols: Vec<ApiEntry> = Vec::new();
    let mut source_refs = Vec::new();
    let mut lang_counts: HashMap<String, usize> = HashMap::new();

    for node_id in &community.node_ids {
        let Some(node) = graph.get_node(node_id) else {
            continue;
        };
        if node.node_type != NodeType::File {
            continue;
        }

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
            let Some(child) = graph.get_node(child_id) else {
                continue;
            };
            if child.node_type != NodeType::Symbol {
                continue;
            }
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
                    doc: child.doc.clone(),
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
    let test_coverage = compute_test_coverage(&member_ids, graph, repo_root);

    // Semantic title: path-based + primary concept
    let semantic_title = derive_semantic_title(&community.name, &files, &entry_points);

    let mut doc = WikiDoc {
        slug,
        title: semantic_title,
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
        summary: String::new(),
        tags: Vec::new(),
        crate_description: None,
        module_doc: None,
        generated_at: chrono_now(),
        enriched: false,
    };

    // Generate after doc is constructed (needs all fields)
    doc.summary = generate_summary(&doc);
    doc.tags = generate_tags(&doc);
    doc
}

// ---------------------------------------------------------------------------
// Section generators
// ---------------------------------------------------------------------------

/// Entry points: symbols with no incoming Calls from within the community.
/// These are the "doors" into the module — the most important public API.
pub(super) fn find_entry_points(
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
            let internal_callers = reverse
                .iter()
                .filter(|r| {
                    if let Some(node) = graph.get_node(r) {
                        // Check if caller is in this community AND it's a Calls edge
                        member_ids.contains(node.id.as_str())
                            && graph
                                .edges_between(r, candidate_id)
                                .iter()
                                .any(|e| e.edge_type == EdgeType::Calls)
                    } else {
                        false
                    }
                })
                .count();

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
pub(super) fn find_call_flow(
    member_ids: &HashSet<&str>,
    graph: &CodeGraph,
    max_depth: usize,
) -> Vec<FlowStep> {
    let mut steps = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();

    for node_id in member_ids {
        if visited.contains(*node_id) {
            continue;
        }
        let Some(node) = graph.get_node(node_id) else {
            continue;
        };
        if node.node_type != NodeType::Symbol {
            continue;
        }

        // BFS from this symbol
        let mut queue = vec![(node_id.to_string(), 0usize)];
        while let Some((current, depth)) = queue.pop() {
            if depth >= max_depth {
                continue;
            }
            if !visited.insert(current.clone()) {
                continue;
            }

            for neighbor_id in graph.neighbors(&current) {
                let Some(neighbor) = graph.get_node(neighbor_id) else {
                    continue;
                };
                if neighbor.node_type != NodeType::Symbol {
                    continue;
                }

                // Only follow Calls edges
                let has_call = graph
                    .edges_between(&current, neighbor_id)
                    .iter()
                    .any(|e| e.edge_type == EdgeType::Calls);

                if has_call {
                    let from_name = graph
                        .get_node(&current)
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
        if steps.len() > 20 {
            break;
        }
    }

    steps.truncate(20);
    steps
}

/// Cross-community dependencies via Imports/Calls/TypeDepends edges.
pub(super) fn find_cross_deps(
    member_ids: &HashSet<&str>,
    graph: &CodeGraph,
    file_to_community: &HashMap<String, String>,
) -> Vec<DepEntry> {
    let mut deps: HashMap<String, (String, String)> = HashMap::new(); // slug → (name, edge_type)

    for edge in graph.all_edges() {
        if !member_ids.contains(edge.source.as_str()) {
            continue;
        }
        if member_ids.contains(edge.target.as_str()) {
            continue;
        }

        let edge_type = match edge.edge_type {
            EdgeType::Imports => "Imports",
            EdgeType::Calls => "Calls",
            EdgeType::TypeDepends => "TypeDepends",
            _ => continue,
        };

        // Find which community the target belongs to
        if let Some(target_node) = graph.get_node(&edge.target) {
            let target_path = target_node
                .file_path
                .as_deref()
                .unwrap_or(&target_node.name);
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

/// Test coverage: uses the graph's NodeType::Test nodes and EdgeType::Tests edges.
///
/// The parser already detects tests professionally across 9 languages:
/// Rust (#[test]), Python (def test_*), Go (func Test*), Java (@Test),
/// C# ([Fact]/[Theory]), PHP, Ruby, TypeScript, Kotlin.
///
/// The bridge creates NodeType::Test nodes and EdgeType::Tests edges.
/// We simply query the graph — no file scanning, no regex hacks.
pub(super) fn compute_test_coverage(
    member_ids: &HashSet<&str>,
    graph: &CodeGraph,
    _repo_root: Option<&std::path::Path>,
) -> TestCoverage {
    // Count production symbols (non-test) in this community
    let mut production_symbols: Vec<String> = Vec::new();
    for node_id in member_ids {
        let Some(node) = graph.get_node(node_id) else {
            continue;
        };
        if node.node_type == NodeType::Symbol {
            production_symbols.push(node.name.clone());
        }
    }

    // Find which production symbols are covered by Tests edges.
    // Strategy: scan ALL Tests edges in the graph, check if target is in our community.
    let member_id_set: HashSet<&str> = member_ids.iter().copied().collect();
    let mut tested_set: HashSet<String> = HashSet::new();

    // Scan all Tests edges (efficient: 430 edges in theo-code)
    for edge in graph.all_edges() {
        if edge.edge_type != EdgeType::Tests {
            continue;
        }
        // If the target (tested symbol) is in our community, mark it
        if member_id_set.contains(edge.target.as_str())
            && let Some(target_node) = graph.get_node(&edge.target) {
                tested_set.insert(target_node.name.clone());
            }
    }

    // Also count Test nodes that share the same file_path as our community files
    let community_files: HashSet<String> = member_ids
        .iter()
        .filter_map(|id| graph.get_node(id))
        .filter_map(|n| n.file_path.clone())
        .collect();

    let mut community_test_count = 0;
    for node_id in graph.node_ids() {
        let Some(node) = graph.get_node(node_id) else {
            continue;
        };
        if node.node_type != NodeType::Test {
            continue;
        }
        if let Some(fp) = &node.file_path
            && community_files.contains(fp) {
                community_test_count += 1;
            }
    }

    let total = production_symbols.len();
    let tested = tested_set.len();
    // If we have test nodes but no edge matches, at least show that tests exist
    let effective_tested = if tested == 0 && community_test_count > 0 {
        // Conservative: assume each test covers ~1 production symbol
        community_test_count.min(total)
    } else {
        tested
    };

    let percentage = if total > 0 {
        (effective_tested as f64 / total as f64) * 100.0
    } else if community_test_count > 0 {
        // Module has tests but no production symbols (pure test module)
        100.0
    } else {
        0.0
    };

    let untested: Vec<String> = production_symbols
        .into_iter()
        .filter(|name| !tested_set.contains(name))
        .take(10)
        .collect();

    TestCoverage {
        tested: effective_tested,
        total,
        percentage,
        untested,
    }
}
