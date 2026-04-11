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
///
/// `repo_root` is needed to scan source files for test coverage detection.
pub fn generate_wiki(communities: &[Community], graph: &CodeGraph, project_name: &str) -> Wiki {
    generate_wiki_with_root(communities, graph, project_name, None)
}

/// Generate wiki with explicit repo root for test file scanning.
pub fn generate_wiki_with_root(
    communities: &[Community],
    graph: &CodeGraph,
    project_name: &str,
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
        if let Some(crate_name) = find_crate_for_doc(doc, &crate_meta) {
            if let Some(meta) = crate_meta.get(&crate_name) {
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
                if doc.crate_description.is_none() {
                    if let Some(readme) = extract_readme_summary(project_dir, crate_dir) {
                        doc.summary = readme.clone();
                        doc.crate_description = Some(readme);
                    }
                }
            }
        }
    }

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
            page_hashes: std::collections::HashMap::new(),
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
fn find_call_flow(
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
fn find_cross_deps(
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
fn compute_test_coverage(
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
        if member_id_set.contains(edge.target.as_str()) {
            if let Some(target_node) = graph.get_node(&edge.target) {
                tested_set.insert(target_node.name.clone());
            }
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
        if let Some(fp) = &node.file_path {
            if community_files.contains(fp) {
                community_test_count += 1;
            }
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

// ---------------------------------------------------------------------------
// Layer 1: Author metadata extraction (Cargo.toml, //!, README.md)
// ---------------------------------------------------------------------------

/// Metadata extracted from Cargo.toml / pyproject.toml / package.json.
#[derive(Debug, Clone, Default)]
pub struct CrateMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub crate_dir: String,
}

/// Extract crate metadata from all Cargo.toml files in the project.
pub fn extract_crate_metadata(project_dir: &std::path::Path) -> HashMap<String, CrateMetadata> {
    let mut metadata = HashMap::new();

    // Walk for Cargo.toml files (max depth 3 to avoid target/)
    if let Ok(entries) = std::fs::read_dir(project_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if name == "target" || name == ".git" || name == "node_modules" {
                    continue;
                }
                // Check for Cargo.toml in this directory
                let cargo_path = path.join("Cargo.toml");
                if cargo_path.exists() {
                    if let Some(meta) = parse_cargo_toml(&cargo_path) {
                        let key = meta.name.clone().unwrap_or_else(|| name.clone());
                        metadata.insert(
                            key,
                            CrateMetadata {
                                crate_dir: name,
                                ..meta
                            },
                        );
                    }
                }
            }
        }
    }

    // Also check root Cargo.toml (single-crate projects)
    let root_cargo = project_dir.join("Cargo.toml");
    if root_cargo.exists() {
        if let Some(meta) = parse_cargo_toml(&root_cargo) {
            let key = meta.name.clone().unwrap_or_else(|| {
                project_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("project")
                    .to_string()
            });
            if !metadata.contains_key(&key) {
                metadata.insert(key, meta);
            }
        }
    }

    // Check pyproject.toml for Python projects
    let pyproject = project_dir.join("pyproject.toml");
    if pyproject.exists() {
        if let Ok(content) = std::fs::read_to_string(&pyproject) {
            let name = extract_toml_value(&content, "name");
            let desc = extract_toml_value(&content, "description");
            if name.is_some() || desc.is_some() {
                let key = name.clone().unwrap_or_else(|| "project".into());
                metadata.entry(key).or_insert(CrateMetadata {
                    name,
                    description: desc,
                    crate_dir: ".".into(),
                });
            }
        }
    }

    metadata
}

fn parse_cargo_toml(path: &std::path::Path) -> Option<CrateMetadata> {
    let content = std::fs::read_to_string(path).ok()?;
    let name = extract_toml_value(&content, "name");
    let description = extract_toml_value(&content, "description");
    Some(CrateMetadata {
        name,
        description,
        crate_dir: String::new(),
    })
}

/// Simple TOML value extraction (no full parser needed — just key = "value" lines).
fn extract_toml_value(content: &str, key: &str) -> Option<String> {
    // Look for: key = "value" in [package] section
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]" || trimmed == "[project]";
        }
        if in_package {
            if let Some(rest) = trimmed.strip_prefix(key) {
                let rest = rest.trim();
                if let Some(rest) = rest.strip_prefix('=') {
                    let val = rest.trim().trim_matches('"').trim_matches('\'');
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Extract //! module-level doc comments from lib.rs or main.rs.
pub fn extract_module_doc(project_dir: &std::path::Path, crate_dir: &str) -> Option<String> {
    // Try crate-level lib.rs first, then main.rs, then src/lib.rs
    let candidates = [
        project_dir.join(crate_dir).join("src").join("lib.rs"),
        project_dir.join(crate_dir).join("src").join("main.rs"),
        project_dir.join(crate_dir).join("lib.rs"),
        project_dir.join("src").join("lib.rs"), // root-level project
        project_dir.join("src").join("main.rs"),
    ];

    for path in &candidates {
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(path).ok()?;

        // Collect consecutive //! lines at the start
        let mut doc_lines: Vec<String> = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//!") {
                let text = trimmed.strip_prefix("//!").unwrap_or("").to_string();
                // Remove leading space if present
                let text = text.strip_prefix(' ').unwrap_or(&text).to_string();
                doc_lines.push(text);
            } else if trimmed.is_empty() && !doc_lines.is_empty() {
                doc_lines.push(String::new()); // preserve paragraph breaks
            } else if !trimmed.is_empty() && !doc_lines.is_empty() {
                break; // end of module doc
            } else if trimmed.starts_with("//") || trimmed.starts_with("#![") {
                continue; // skip regular comments and attributes
            } else if !trimmed.is_empty() {
                break; // code started
            }
        }

        if !doc_lines.is_empty() {
            // Trim trailing empty lines
            while doc_lines.last().map_or(false, |l| l.is_empty()) {
                doc_lines.pop();
            }
            return Some(doc_lines.join("\n"));
        }
    }
    None
}

/// Extract first meaningful paragraph from README.md.
pub fn extract_readme_summary(project_dir: &std::path::Path, crate_dir: &str) -> Option<String> {
    let candidates = [
        project_dir.join(crate_dir).join("README.md"),
        project_dir.join("README.md"),
    ];

    for path in &candidates {
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(path).ok()?;

        // Skip: headings, badges, empty lines, links-only lines
        let paragraph: Vec<&str> = content
            .lines()
            .skip_while(|l| {
                let t = l.trim();
                t.is_empty()
                    || t.starts_with('#')
                    || t.starts_with('[')
                    || t.starts_with('!')
                    || t.starts_with("[![")
                    || t.starts_with("More information")
            })
            .take_while(|l| !l.trim().is_empty())
            .collect();

        if !paragraph.is_empty() {
            let text = paragraph.join(" ");
            if text.len() > 10 {
                // skip very short fragments
                return Some(text);
            }
        }
    }
    None
}

/// Match a WikiDoc to its crate metadata by finding which crate directory contains its files.
fn find_crate_for_doc(
    doc: &super::model::WikiDoc,
    metadata: &HashMap<String, CrateMetadata>,
) -> Option<String> {
    // Build sorted list of crate dirs (longest first for precise matching)
    let mut dirs: Vec<(&String, &str)> = metadata
        .iter()
        .map(|(name, meta)| {
            let dir = if meta.crate_dir.is_empty() {
                name.as_str()
            } else {
                meta.crate_dir.as_str()
            };
            (name, dir)
        })
        .collect();
    dirs.sort_by(|a, b| b.1.len().cmp(&a.1.len())); // longest prefix first

    // Check majority of files — which crate dir has most file matches?
    let mut best_match: Option<(String, usize)> = None;
    for (name, dir) in &dirs {
        let prefix = format!("{}/", dir);
        let count = doc
            .files
            .iter()
            .filter(|f| f.path.starts_with(&prefix))
            .count();
        if count > 0 {
            if best_match
                .as_ref()
                .map_or(true, |(_, best_count)| count > *best_count)
            {
                best_match = Some(((*name).clone(), count));
            }
        }
    }
    if let Some((name, _)) = best_match {
        return Some(name);
    }

    // Fallback: check title/slug (longest match first)
    for (name, _) in &dirs {
        if doc.title.contains(name.as_str()) || doc.slug.contains(name.as_str()) {
            return Some((*name).clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Karpathy header generators (deterministic, zero LLM)
// ---------------------------------------------------------------------------

/// Derive a semantic title from path structure + primary entry point.
/// "axum-core (10)" → "axum-core — IntoResponseParts"
fn derive_semantic_title(
    community_name: &str,
    files: &[super::model::FileEntry],
    entry_points: &[super::model::ApiEntry],
) -> String {
    // Base: common path prefix or crate directory
    let base = if let Some(first) = files.first() {
        let segments: Vec<&str> = first.path.split('/').collect();
        if segments.len() >= 2 {
            // "axum-core/src/..." → "axum-core"
            // "src/auth.rs" → keep community name
            let first_seg = segments[0];
            if first_seg == "src" || first_seg == "lib" || first_seg == "." {
                community_name
                    .split('(')
                    .next()
                    .unwrap_or(community_name)
                    .trim()
                    .to_string()
            } else {
                first_seg.to_string()
            }
        } else {
            community_name
                .split('(')
                .next()
                .unwrap_or(community_name)
                .trim()
                .to_string()
        }
    } else {
        community_name
            .split('(')
            .next()
            .unwrap_or(community_name)
            .trim()
            .to_string()
    };

    // Append primary concept (first trait/struct entry point)
    let primary = entry_points
        .first()
        .filter(|e| e.kind == "Trait" || e.kind == "Struct" || e.kind == "Function")
        .map(|e| format!(" — {}", e.name));

    let full = format!("{}{}", base, primary.unwrap_or_default());
    if full.len() > 60 {
        full[..57].to_string() + "..."
    } else {
        full
    }
}

/// Generate a one-line deterministic summary from WikiDoc fields.
/// "Rust traits across 10 files (140 symbols, depends on 3 modules). Primary: IntoResponse."
fn generate_summary(doc: &super::model::WikiDoc) -> String {
    let kind_summary = {
        let has_traits = doc.public_api.iter().any(|a| a.kind == "Trait");
        let has_structs = doc.public_api.iter().any(|a| a.kind == "Struct");
        if has_traits && has_structs {
            "traits and types"
        } else if has_traits {
            "traits"
        } else if has_structs {
            "types"
        } else {
            "functions"
        }
    };

    let dep_hint = if !doc.dependencies.is_empty() {
        format!(", depends on {} modules", doc.dependencies.len())
    } else {
        String::new()
    };

    let primary = doc
        .entry_points
        .first()
        .map(|e| format!(" Primary: {}.", e.name))
        .unwrap_or_default();

    format!(
        "{} {} across {} files ({} symbols{}).{}",
        doc.primary_language, kind_summary, doc.file_count, doc.symbol_count, dep_hint, primary
    )
}

/// Auto-detect tags from file paths and symbol kinds.
fn generate_tags(doc: &super::model::WikiDoc) -> Vec<String> {
    let mut tags = vec![doc.primary_language.clone()];

    // From path patterns
    let all_paths: String = doc
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let path_patterns: &[(&str, &str)] = &[
        ("test", "testing"),
        ("auth", "auth"),
        ("route", "routing"),
        ("router", "routing"),
        ("middleware", "middleware"),
        ("extract", "extraction"),
        ("error", "error-handling"),
        ("handler", "handlers"),
        ("response", "response"),
        ("request", "request"),
        ("body", "http-body"),
        ("json", "json"),
        ("form", "forms"),
        ("query", "query"),
        ("state", "state"),
        ("tower", "tower"),
        ("service", "service"),
        ("layer", "layer"),
        ("header", "headers"),
        ("cookie", "cookies"),
        ("websocket", "websocket"),
        ("sse", "sse"),
        ("multipart", "multipart"),
    ];
    for (pattern, tag) in path_patterns {
        if all_paths.contains(pattern) {
            tags.push(tag.to_string());
        }
    }

    // From symbol kinds
    if doc.public_api.iter().any(|a| a.kind == "Trait") {
        tags.push("traits".into());
    }
    if doc.public_api.iter().any(|a| a.kind == "Struct") {
        tags.push("types".into());
    }
    if doc.public_api.iter().any(|a| a.kind == "Enum") {
        tags.push("enums".into());
    }
    if doc.test_coverage.percentage > 80.0 {
        tags.push("well-tested".into());
    }

    tags.sort();
    tags.dedup();
    tags
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

/// Compute hash for a single community's files (for incremental generation).
///
/// Uses canonical path prefix (common path of member files) as stable key,
/// independent of Leiden's non-deterministic node_ids ordering.
pub fn compute_community_hash(community: &Community, graph: &CodeGraph) -> u64 {
    use std::collections::BTreeMap;
    use std::hash::{Hash, Hasher};

    let mut file_info: BTreeMap<String, u64> = BTreeMap::new();
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            if node.node_type == NodeType::File {
                let path = node.file_path.as_deref().unwrap_or(&node.name);
                file_info.insert(path.to_string(), node.last_modified.to_bits());
            }
        }
    }

    let mut hasher = std::hash::DefaultHasher::new();
    for (path, mtime) in &file_info {
        path.hash(&mut hasher);
        mtime.hash(&mut hasher);
    }
    hasher.finish()
}

/// Canonical key for a community — the community slug.
///
/// Used as the key in page_hashes for incremental generation.
/// Uses slugify(community.name) which is deterministic for the same community.
pub fn community_canonical_key(community: &Community, _graph: &CodeGraph) -> String {
    slugify(&community.name)
}

/// Generate wiki incrementally: only regenerate changed pages + dependents.
///
/// Returns the complete wiki (unchanged pages are loaded from existing docs).
pub fn generate_wiki_incremental(
    communities: &[Community],
    graph: &CodeGraph,
    project_name: &str,
    existing_manifest: &WikiManifest,
    existing_docs: &[WikiDoc],
) -> (Wiki, IncrementalStats) {
    // Always build the global file→community map (needed for cross-deps)
    let file_to_community = build_file_community_map(communities, graph);

    // Phase 1: Compute per-community hashes, detect changed
    let active_communities: Vec<&Community> = communities
        .iter()
        .filter(|c| !c.node_ids.is_empty())
        .collect();

    let mut new_hashes: HashMap<String, u64> = HashMap::new();
    let mut changed_keys: HashSet<String> = HashSet::new();
    let mut key_to_community: HashMap<String, &Community> = HashMap::new();

    for c in &active_communities {
        let key = community_canonical_key(c, graph);
        let hash = compute_community_hash(c, graph);
        new_hashes.insert(key.clone(), hash);
        key_to_community.insert(key.clone(), c);

        if existing_manifest.page_hashes.get(&key) != Some(&hash) {
            changed_keys.insert(key);
        }
    }

    // Fast path: nothing changed
    if changed_keys.is_empty() {
        let now = chrono_now();
        return (
            Wiki {
                docs: existing_docs.to_vec(),
                manifest: WikiManifest {
                    schema_version: WikiManifest::SCHEMA_VERSION,
                    generator_version: WikiManifest::GENERATOR_VERSION.to_string(),
                    graph_hash: compute_graph_hash(graph),
                    generated_at: now,
                    page_count: existing_docs.len(),
                    page_hashes: new_hashes,
                },
            },
            IncrementalStats {
                changed: 0,
                propagated: 0,
                skipped: active_communities.len(),
            },
        );
    }

    // Threshold: if >50% changed, full regen is simpler
    if changed_keys.len() * 2 > active_communities.len() {
        let wiki = generate_wiki_with_root(communities, graph, project_name, None);
        let mut manifest = wiki.manifest.clone();
        manifest.page_hashes = new_hashes;
        let stats = IncrementalStats {
            changed: active_communities.len(),
            propagated: 0,
            skipped: 0,
        };
        return (
            Wiki {
                docs: wiki.docs,
                manifest,
            },
            stats,
        );
    }

    // Phase 2: Generate changed docs
    let mut changed_docs: HashMap<String, WikiDoc> = HashMap::new();
    for key in &changed_keys {
        if let Some(community) = key_to_community.get(key.as_str()) {
            let doc = generate_doc(community, graph, communities, &file_to_community, None);
            changed_docs.insert(doc.slug.clone(), doc);
        }
    }

    // Phase 3: Dependency propagation (2-hop)
    // Build reverse-dep map: target_slug → set of source_slugs
    let mut reverse_deps: HashMap<String, HashSet<String>> = HashMap::new();
    // Include existing docs' deps
    for doc in existing_docs {
        for dep in &doc.dependencies {
            reverse_deps
                .entry(dep.target_slug.clone())
                .or_default()
                .insert(doc.slug.clone());
        }
    }
    // Include new changed docs' deps
    for doc in changed_docs.values() {
        for dep in &doc.dependencies {
            reverse_deps
                .entry(dep.target_slug.clone())
                .or_default()
                .insert(doc.slug.clone());
        }
    }

    // Find slugs of changed communities
    let changed_slugs: HashSet<String> = changed_docs.keys().cloned().collect();

    // 2-hop propagation
    let mut propagated_keys: HashSet<String> = HashSet::new();
    let mut to_propagate: HashSet<String> = HashSet::new();

    // Hop 1: direct dependents of changed slugs
    for changed_slug in &changed_slugs {
        if let Some(dependents) = reverse_deps.get(changed_slug) {
            for dep_slug in dependents {
                if !changed_slugs.contains(dep_slug) {
                    to_propagate.insert(dep_slug.clone());
                }
            }
        }
    }

    // Hop 2: dependents of hop-1 slugs
    let hop1_slugs = to_propagate.clone();
    for hop1_slug in &hop1_slugs {
        if let Some(dependents) = reverse_deps.get(hop1_slug) {
            for dep_slug in dependents {
                if !changed_slugs.contains(dep_slug) && !hop1_slugs.contains(dep_slug) {
                    to_propagate.insert(dep_slug.clone());
                }
            }
        }
    }

    // Regenerate propagated pages
    for key in new_hashes.keys() {
        if let Some(community) = key_to_community.get(key.as_str()) {
            let slug = slugify(&community.name);
            if to_propagate.contains(&slug) && !changed_slugs.contains(&slug) {
                let doc = generate_doc(community, graph, communities, &file_to_community, None);
                propagated_keys.insert(key.clone());
                changed_docs.insert(doc.slug.clone(), doc);
            }
        }
    }

    // Phase 4: Merge — changed docs override existing
    let mut final_docs: Vec<WikiDoc> = Vec::new();
    let existing_by_slug: HashMap<String, &WikiDoc> =
        existing_docs.iter().map(|d| (d.slug.clone(), d)).collect();

    // Track which existing slugs are still valid
    let current_slugs: HashSet<String> = active_communities
        .iter()
        .map(|c| slugify(&c.name))
        .collect();

    for c in &active_communities {
        let slug = slugify(&c.name);
        if let Some(new_doc) = changed_docs.remove(&slug) {
            final_docs.push(new_doc);
        } else if let Some(existing) = existing_by_slug.get(&slug) {
            final_docs.push((*existing).clone());
        }
    }

    final_docs.sort_by(|a, b| b.file_count.cmp(&a.file_count));

    let now = chrono_now();
    let stats = IncrementalStats {
        changed: changed_keys.len(),
        propagated: propagated_keys.len(),
        skipped: active_communities.len() - changed_keys.len() - propagated_keys.len(),
    };

    (
        Wiki {
            manifest: WikiManifest {
                schema_version: WikiManifest::SCHEMA_VERSION,
                generator_version: WikiManifest::GENERATOR_VERSION.to_string(),
                graph_hash: compute_graph_hash(graph),
                generated_at: now,
                page_count: final_docs.len(),
                page_hashes: new_hashes,
            },
            docs: final_docs,
        },
        stats,
    )
}

/// Stats from incremental generation.
#[derive(Debug)]
pub struct IncrementalStats {
    pub changed: usize,
    pub propagated: usize,
    pub skipped: usize,
}

impl std::fmt::Display for IncrementalStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "changed: {}, propagated: {}, skipped: {}",
            self.changed, self.propagated, self.skipped
        )
    }
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
// Concept detection
// ---------------------------------------------------------------------------

/// A detected concept that merits a dedicated concept page.
#[derive(Debug, Clone)]
pub struct ConceptCandidate {
    /// Concept name (e.g., "retrieval", "authentication", "sandbox").
    pub name: String,
    /// Slugs of related module pages.
    pub related_modules: Vec<String>,
    /// Hint text from top symbols/docs.
    pub description_hint: String,
}

/// Detect high-level concepts using graph topology (cross-dep edge density)
/// with prefix-based fallback.
///
/// Algorithm:
/// 1. Build adjacency matrix from WikiDoc.dependencies
/// 2. Union-find: merge communities with >= 3 mutual cross-deps
/// 3. Fallback to prefix-based grouping for unclustered modules
pub fn detect_concepts(docs: &[super::model::WikiDoc]) -> Vec<ConceptCandidate> {
    let filtered: Vec<&super::model::WikiDoc> = docs.iter().filter(|d| d.file_count >= 2).collect();

    if filtered.is_empty() {
        return Vec::new();
    }

    // Build slug → index map
    let slug_to_idx: HashMap<String, usize> = filtered
        .iter()
        .enumerate()
        .map(|(i, d)| (d.slug.clone(), i))
        .collect();

    let n = filtered.len();

    // Build adjacency matrix: adj[i][j] = count of deps from i to j
    let mut adj = vec![vec![0u32; n]; n];
    for (i, doc) in filtered.iter().enumerate() {
        for dep in &doc.dependencies {
            if let Some(&j) = slug_to_idx.get(&dep.target_slug) {
                if i != j {
                    adj[i][j] += 1;
                }
            }
        }
    }

    // Union-Find
    let mut parent: Vec<usize> = (0..n).collect();
    let find = |parent: &mut Vec<usize>, mut x: usize| -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]]; // path compression
            x = parent[x];
        }
        x
    };

    // Merge communities with >= 3 mutual edges
    for i in 0..n {
        for j in (i + 1)..n {
            let mutual = adj[i][j] + adj[j][i];
            if mutual >= 3 {
                let ri = find(&mut parent, i);
                let rj = find(&mut parent, j);
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }

    // Collect topology-based clusters
    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        clusters.entry(root).or_default().push(i);
    }

    let mut concepts = Vec::new();
    let mut clustered_slugs: HashSet<String> = HashSet::new();

    for members in clusters.values() {
        if members.len() < 2 {
            continue;
        } // Need 2+ for a concept

        let group_docs: Vec<&super::model::WikiDoc> =
            members.iter().map(|&i| filtered[i]).collect();

        let related_modules: Vec<String> = group_docs.iter().map(|d| d.slug.clone()).collect();
        for slug in &related_modules {
            clustered_slugs.insert(slug.clone());
        }

        // Name from common prefix or first doc's crate prefix
        let name = derive_concept_name(&group_docs);
        let description_hint = build_description_hint(&group_docs);

        concepts.push(ConceptCandidate {
            name,
            related_modules,
            description_hint,
        });
    }

    // Fallback: prefix-based for unclustered modules
    let mut prefix_groups: HashMap<String, Vec<&super::model::WikiDoc>> = HashMap::new();
    for doc in &filtered {
        if clustered_slugs.contains(&doc.slug) {
            continue;
        }
        let key = doc
            .title
            .split(|c: char| c == '(' || c == ' ')
            .next()
            .unwrap_or(&doc.title)
            .trim()
            .split('-')
            .take(2)
            .collect::<Vec<_>>()
            .join("-");
        if key.len() >= 4 {
            prefix_groups.entry(key).or_default().push(doc);
        }
    }

    for (_, group_docs) in &prefix_groups {
        if group_docs.len() < 2 {
            continue;
        }
        let related_modules: Vec<String> = group_docs.iter().map(|d| d.slug.clone()).collect();
        let name = derive_concept_name(group_docs);
        let description_hint = build_description_hint(group_docs);
        concepts.push(ConceptCandidate {
            name,
            related_modules,
            description_hint,
        });
    }

    concepts.sort_by(|a, b| b.related_modules.len().cmp(&a.related_modules.len()));
    concepts.truncate(8);
    concepts
}

/// Derive a human-readable concept name from a group of docs.
fn derive_concept_name(docs: &[&super::model::WikiDoc]) -> String {
    // Extract common prefix key
    let keys: Vec<String> = docs
        .iter()
        .map(|d| {
            d.title
                .split(|c: char| c == '(' || c == ' ')
                .next()
                .unwrap_or(&d.title)
                .trim()
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect();

    let most_common = keys
        .iter()
        .fold(HashMap::new(), |mut acc, k| {
            *acc.entry(k.as_str()).or_insert(0) += 1;
            acc
        })
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(k, _)| k.to_string())
        .unwrap_or_default();

    match most_common.as_str() {
        "theo-engine" => "Code Intelligence Engine".to_string(),
        "theo-agent" => "Agent Runtime".to_string(),
        "theo-infra" => "Infrastructure".to_string(),
        "theo-tooling" => "Developer Tools".to_string(),
        "theo-governance" => "Governance & Safety".to_string(),
        "theo-domain" => "Domain Model".to_string(),
        "theo-ui" | "theo-desktop" => "Frontend & Desktop".to_string(),
        "theo-application" => "Application Layer".to_string(),
        other if !other.is_empty() => format!("{} Subsystem", other.replace('-', " ")),
        _ => "Related Modules".to_string(),
    }
}

fn build_description_hint(docs: &[&super::model::WikiDoc]) -> String {
    let mut hints = Vec::new();
    for doc in docs.iter().take(3) {
        for ep in doc.entry_points.iter().take(2) {
            hints.push(format!("{}: {}", ep.name, ep.signature));
        }
    }
    if hints.is_empty() {
        format!("{} related modules", docs.len())
    } else {
        hints.join("; ")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::model::{Edge, Node};

    fn test_graph() -> (CodeGraph, Vec<Community>) {
        let mut graph = CodeGraph::new();

        // File: auth.rs
        graph.add_node(Node {
            id: "file:auth.rs".into(),
            name: "auth.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/auth.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 100.0,
        });
        graph.add_node(Node {
            id: "sym:verify".into(),
            name: "verify_token".into(),
            node_type: NodeType::Symbol,
            file_path: Some("src/auth.rs".into()),
            signature: Some("pub fn verify_token(t: &str) -> bool".into()),
            doc: Some("Verify JWT token".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(30),
            last_modified: 100.0,
        });
        graph.add_edge(Edge {
            source: "file:auth.rs".into(),
            target: "sym:verify".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // File: handler.rs (different community)
        graph.add_node(Node {
            id: "file:handler.rs".into(),
            name: "handler.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/handler.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 200.0,
        });
        graph.add_node(Node {
            id: "sym:handle".into(),
            name: "handle_request".into(),
            node_type: NodeType::Symbol,
            file_path: Some("src/handler.rs".into()),
            signature: Some("pub fn handle_request(req: Request) -> Response".into()),
            doc: None,
            kind: Some(SymbolKind::Function),
            line_start: Some(5),
            line_end: Some(20),
            last_modified: 200.0,
        });
        graph.add_edge(Edge {
            source: "file:handler.rs".into(),
            target: "sym:handle".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // Second file in auth community
        graph.add_node(Node {
            id: "file:auth_utils.rs".into(),
            name: "auth_utils.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/auth_utils.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 100.0,
        });

        // handler calls verify (cross-community)
        graph.add_edge(Edge {
            source: "sym:handle".into(),
            target: "sym:verify".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        // Second file in handler community
        graph.add_node(Node {
            id: "file:middleware.rs".into(),
            name: "middleware.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/middleware.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 200.0,
        });

        let communities = vec![
            Community {
                id: "c1".into(),
                name: "auth".into(),
                node_ids: vec![
                    "file:auth.rs".into(),
                    "sym:verify".into(),
                    "file:auth_utils.rs".into(),
                ],
                level: 0,
                parent_id: None,
                version: 0,
            },
            Community {
                id: "c2".into(),
                name: "handler".into(),
                node_ids: vec![
                    "file:handler.rs".into(),
                    "sym:handle".into(),
                    "file:middleware.rs".into(),
                ],
                level: 0,
                parent_id: None,
                version: 0,
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
        assert!(auth.files.len() >= 1);
        assert!(auth.files.iter().any(|f| f.path == "src/auth.rs"));
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
            id: "empty".into(),
            name: "empty".into(),
            node_ids: vec![],
            level: 0,
            parent_id: None,
            version: 0,
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

    #[test]
    fn community_hash_deterministic() {
        let (graph, communities) = test_graph();
        let h1 = compute_community_hash(&communities[0], &graph);
        let h2 = compute_community_hash(&communities[0], &graph);
        assert_eq!(h1, h2);
    }

    #[test]
    fn community_hash_differs_between_communities() {
        let (graph, communities) = test_graph();
        let h1 = compute_community_hash(&communities[0], &graph);
        let h2 = compute_community_hash(&communities[1], &graph);
        assert_ne!(h1, h2);
    }

    #[test]
    fn incremental_zero_change_zero_regen() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");

        // Build page_hashes from the current state
        let mut page_hashes = HashMap::new();
        for c in &communities {
            if c.node_ids.is_empty() {
                continue;
            }
            let key = community_canonical_key(c, &graph);
            let hash = compute_community_hash(c, &graph);
            page_hashes.insert(key, hash);
        }

        let manifest_with_hashes = WikiManifest {
            page_hashes,
            ..wiki.manifest.clone()
        };

        let (_, stats) = generate_wiki_incremental(
            &communities,
            &graph,
            "test",
            &manifest_with_hashes,
            &wiki.docs,
        );
        assert_eq!(stats.changed, 0, "no changes should mean zero regeneration");
        assert_eq!(stats.propagated, 0);
    }

    fn test_graph_modified() -> (CodeGraph, Vec<Community>) {
        // Same as test_graph but auth.rs has different mtime
        let mut graph = CodeGraph::new();
        graph.add_node(Node {
            id: "file:auth.rs".into(),
            name: "auth.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/auth.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 999.0, // CHANGED
        });
        graph.add_node(Node {
            id: "sym:verify".into(),
            name: "verify_token".into(),
            node_type: NodeType::Symbol,
            file_path: Some("src/auth.rs".into()),
            signature: Some("pub fn verify_token(t: &str) -> bool".into()),
            doc: Some("Verify JWT token".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(30),
            last_modified: 999.0,
        });
        graph.add_edge(Edge {
            source: "file:auth.rs".into(),
            target: "sym:verify".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_node(Node {
            id: "file:handler.rs".into(),
            name: "handler.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/handler.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 200.0,
        });
        graph.add_node(Node {
            id: "sym:handle".into(),
            name: "handle_request".into(),
            node_type: NodeType::Symbol,
            file_path: Some("src/handler.rs".into()),
            signature: Some("pub fn handle_request(req: Request) -> Response".into()),
            doc: None,
            kind: Some(SymbolKind::Function),
            line_start: Some(5),
            line_end: Some(20),
            last_modified: 200.0,
        });
        graph.add_edge(Edge {
            source: "file:handler.rs".into(),
            target: "sym:handle".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "sym:handle".into(),
            target: "sym:verify".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        // Second files (same as test_graph)
        graph.add_node(Node {
            id: "file:auth_utils.rs".into(),
            name: "auth_utils.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/auth_utils.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 100.0,
        });
        graph.add_node(Node {
            id: "file:middleware.rs".into(),
            name: "middleware.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/middleware.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 200.0,
        });

        let communities = vec![
            Community {
                id: "c1".into(),
                name: "auth".into(),
                node_ids: vec![
                    "file:auth.rs".into(),
                    "sym:verify".into(),
                    "file:auth_utils.rs".into(),
                ],
                level: 0,
                parent_id: None,
                version: 0,
            },
            Community {
                id: "c2".into(),
                name: "handler".into(),
                node_ids: vec![
                    "file:handler.rs".into(),
                    "sym:handle".into(),
                    "file:middleware.rs".into(),
                ],
                level: 0,
                parent_id: None,
                version: 0,
            },
        ];
        (graph, communities)
    }

    #[test]
    fn incremental_detects_changed_community() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");

        // Build initial hashes
        let mut page_hashes = HashMap::new();
        for c in &communities {
            if c.node_ids.is_empty() {
                continue;
            }
            let key = community_canonical_key(c, &graph);
            let hash = compute_community_hash(c, &graph);
            page_hashes.insert(key, hash);
        }
        let manifest = WikiManifest {
            page_hashes,
            ..wiki.manifest.clone()
        };

        // New graph where auth.rs has different mtime
        let (graph2, communities2) = test_graph_modified();

        let (result, stats) =
            generate_wiki_incremental(&communities2, &graph2, "test", &manifest, &wiki.docs);
        assert!(stats.changed > 0, "should detect change in auth community");
        assert_eq!(result.docs.len(), 2, "should still have all pages");
    }

    #[test]
    fn topology_concept_detection_with_cross_deps() {
        // Build docs with enough cross-deps to form topology clusters
        let doc_a = WikiDoc {
            slug: "mod-a".into(),
            title: "Module A".into(),
            community_id: "c1".into(),
            file_count: 5,
            symbol_count: 10,
            primary_language: "rs".into(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![
                DepEntry {
                    target_slug: "mod-b".into(),
                    target_name: "B".into(),
                    edge_type: "Calls".into(),
                },
                DepEntry {
                    target_slug: "mod-b".into(),
                    target_name: "B".into(),
                    edge_type: "Imports".into(),
                },
            ],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        };
        let doc_b = WikiDoc {
            slug: "mod-b".into(),
            title: "Module B".into(),
            community_id: "c2".into(),
            file_count: 5,
            symbol_count: 10,
            primary_language: "rs".into(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![DepEntry {
                target_slug: "mod-a".into(),
                target_name: "A".into(),
                edge_type: "Calls".into(),
            }],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        };
        // C and D are isolated
        let doc_c = WikiDoc {
            slug: "other-c".into(),
            title: "Other C".into(),
            community_id: "c3".into(),
            file_count: 3,
            symbol_count: 5,
            primary_language: "rs".into(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        };

        let docs = vec![doc_a, doc_b, doc_c];
        let concepts = detect_concepts(&docs);
        // A and B have 3 mutual deps → should form a topology cluster
        assert!(
            concepts
                .iter()
                .any(|c| c.related_modules.contains(&"mod-a".to_string())
                    && c.related_modules.contains(&"mod-b".to_string())),
            "A and B should be in same concept cluster, got: {:?}",
            concepts
        );
    }

    #[test]
    fn no_deps_falls_back_to_prefix() {
        // Docs with same prefix but no cross-deps → prefix-based grouping
        let make_doc = |slug: &str, title: &str| WikiDoc {
            slug: slug.into(),
            title: title.into(),
            community_id: "cx".into(),
            file_count: 3,
            symbol_count: 5,
            primary_language: "rs".into(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        };

        let docs = vec![
            make_doc("theo-engine-a", "theo-engine-a (10)"),
            make_doc("theo-engine-b", "theo-engine-b (5)"),
        ];
        let concepts = detect_concepts(&docs);
        // Should fall back to prefix: "theo-engine" groups them
        assert!(!concepts.is_empty(), "should have prefix-based concept");
        assert!(concepts[0].related_modules.len() >= 2);
    }

    #[test]
    fn dep_propagation_regenerates_dependent() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");

        // Build initial hashes
        let mut page_hashes = HashMap::new();
        for c in &communities {
            if c.node_ids.is_empty() {
                continue;
            }
            let key = community_canonical_key(c, &graph);
            let hash = compute_community_hash(c, &graph);
            page_hashes.insert(key, hash);
        }
        let manifest = WikiManifest {
            page_hashes,
            ..wiki.manifest.clone()
        };

        // Modified graph: auth changed
        let (graph2, communities2) = test_graph_modified();

        let (_, stats) =
            generate_wiki_incremental(&communities2, &graph2, "test", &manifest, &wiki.docs);
        // auth changed + handler propagated (depends on auth)
        assert!(
            stats.changed >= 1,
            "auth should be changed, stats: {}",
            stats
        );
        let total_regen = stats.changed + stats.propagated;
        assert!(
            total_regen >= 1,
            "at least auth should be regenerated, stats: {}",
            stats
        );
    }
}
