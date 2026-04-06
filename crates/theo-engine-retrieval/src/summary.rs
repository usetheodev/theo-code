/// Deterministic community summary generator.
///
/// Produces human-readable summaries per community using graph metadata:
/// WHERE (files, lines), HOW (call flow, dependencies), WHY (git messages, docstrings).
///
/// These summaries replace raw symbol dumps in context assembly, making the
/// output immediately useful to both LLMs and humans.
use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Structured data derived from the graph — machine-readable, no LLM needed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityStructuredData {
    /// Top symbols by in-degree (most called/referenced)
    pub top_functions: Vec<String>,
    /// Edge types present within this community
    pub edge_types_present: Vec<String>,
    /// Files outside this community that members import/call (max 5)
    pub cross_community_deps: Vec<String>,
    /// Number of unique files
    pub file_count: usize,
    /// Dominant language by file extension
    pub primary_language: String,
}

/// A generated summary for a community.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunitySummary {
    /// Community ID
    pub community_id: String,
    /// Human-readable name (e.g., "auth/jwt")
    pub name: String,
    /// The full summary text (for LLM consumption)
    pub text: String,
    /// Estimated token count (of text only — structured not counted)
    pub token_count: usize,
    /// Machine-readable structured data (for runtime/tracing, not in budget)
    pub structured: CommunityStructuredData,
}

/// Git commit info per file (passed in from the git module).
#[derive(Debug, Clone, Default)]
pub struct FileGitInfo {
    pub last_commit_message: String,
    pub recent_messages: Vec<String>,
}

// ---------------------------------------------------------------------------
// Summary generation
// ---------------------------------------------------------------------------

/// Generate summaries for all communities.
///
/// * `communities` — detected communities from clustering
/// * `graph` — the code graph with nodes and edges
/// * `git_info` — optional git commit messages per file path
pub fn generate_summaries(
    communities: &[Community],
    graph: &CodeGraph,
    git_info: &HashMap<String, FileGitInfo>,
) -> Vec<CommunitySummary> {
    communities
        .iter()
        .map(|comm| generate_one(comm, graph, git_info))
        .collect()
}

fn generate_one(
    community: &Community,
    graph: &CodeGraph,
    git_info: &HashMap<String, FileGitInfo>,
) -> CommunitySummary {
    let mut lines: Vec<String> = Vec::new();

    // Collect node info
    let mut files: HashSet<String> = HashSet::new();
    let mut symbols: Vec<SymbolInfo> = Vec::new();
    let mut test_count = 0;
    let mut total_lines = 0;

    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            if let Some(fp) = &node.file_path {
                files.insert(fp.clone());
            }

            match node.node_type {
                NodeType::Symbol => {
                    let line_span = match (node.line_start, node.line_end) {
                        (Some(s), Some(e)) => e.saturating_sub(s) + 1,
                        _ => 0,
                    };
                    total_lines += line_span;

                    symbols.push(SymbolInfo {
                        id: node_id.clone(),
                        name: node.name.clone(),
                        signature: node.signature.clone(),
                        doc: node.doc.clone(),
                        file: node.file_path.clone().unwrap_or_default(),
                        has_test: false, // filled below
                    });
                }
                NodeType::Test => {
                    test_count += 1;
                }
                _ => {}
            }
        }
    }

    // Determine which symbols are tested
    let tested_symbols: HashSet<String> = graph
        .all_edges()
        .iter()
        .filter(|e| e.edge_type == EdgeType::Tests)
        .filter(|e| community.node_ids.contains(&e.source))
        .map(|e| e.target.clone())
        .collect();

    for sym in &mut symbols {
        sym.has_test = tested_symbols.contains(&sym.id);
    }

    let tested_count = symbols.iter().filter(|s| s.has_test).count();
    let sym_count = symbols.len();

    // --- WHERE ---
    let file_list: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    let file_display = if file_list.len() <= 3 {
        file_list.join(", ")
    } else {
        format!(
            "{}, {} e +{} arquivos",
            file_list[0],
            file_list[1],
            file_list.len() - 2
        )
    };

    lines.push(format!(
        "## {} ({} funções, {} linhas, {})",
        community.name, sym_count, total_lines, file_display
    ));

    // --- HOW: call flow ---
    let call_flow = build_call_flow(&symbols, graph);
    if !call_flow.is_empty() {
        lines.push(String::new());
        lines.push(format!("Fluxo: {}", call_flow));
    }

    // --- Signatures (top 5) ---
    let top_sigs: Vec<String> = symbols
        .iter()
        .filter_map(|s| s.signature.as_ref().map(|sig| format!("  {}", sig)))
        .take(5)
        .collect();
    if !top_sigs.is_empty() {
        lines.push(String::new());
        lines.push("Funções principais:".into());
        lines.extend(top_sigs);
        if sym_count > 5 {
            lines.push(format!("  ... e +{} funções", sym_count - 5));
        }
    }

    // --- Docstrings (first available) ---
    let first_doc = symbols.iter().find_map(|s| s.doc.as_ref());
    if let Some(doc) = first_doc {
        let trimmed = doc.lines().next().unwrap_or("").trim();
        if !trimmed.is_empty() {
            lines.push(String::new());
            lines.push(format!("Descrição: {}", trimmed));
        }
    }

    // --- Dependencies (outgoing edges to other communities) ---
    let deps = find_dependencies(community, graph);
    if !deps.is_empty() {
        let dep_display = deps.join(", ");
        lines.push(format!("Depende de: {}", dep_display));
    }

    // --- Co-change partners ---
    let cochanges = find_cochanges(community, graph);
    if !cochanges.is_empty() {
        let cc_display: Vec<String> = cochanges
            .iter()
            .take(3)
            .map(|(name, weight)| format!("{} ({:.0}%)", name, weight * 100.0))
            .collect();
        lines.push(format!("Co-muda com: {}", cc_display.join(", ")));
    }

    // --- Tests ---
    if test_count > 0 || sym_count > 0 {
        let coverage = if sym_count > 0 {
            (tested_count as f64 / sym_count as f64 * 100.0) as usize
        } else {
            0
        };
        lines.push(format!(
            "Testes: {} testes, {}/{} funções cobertas ({}%)",
            test_count, tested_count, sym_count, coverage
        ));

        // Flag untested symbols
        let untested: Vec<&str> = symbols
            .iter()
            .filter(|s| !s.has_test)
            .take(3)
            .map(|s| s.name.as_str())
            .collect();
        if !untested.is_empty() && tested_count < sym_count {
            lines.push(format!("  Sem teste: {}", untested.join(", ")));
        }
    }

    // --- WHY: git messages ---
    let git_messages = collect_git_messages(&files, git_info);
    if !git_messages.is_empty() {
        lines.push(String::new());
        lines.push("Mudanças recentes:".into());
        for msg in git_messages.iter().take(3) {
            lines.push(format!("  - {}", msg));
        }
    }

    let text = lines.join("\n");
    let token_count = estimate_tokens(&text);

    // Build structured data (machine-readable, no LLM needed).
    let member_set: HashSet<&str> = community.node_ids.iter().map(String::as_str).collect();

    // Top functions: symbols with highest in-degree (most called)
    let mut top_functions: Vec<(String, usize)> = symbols.iter()
        .map(|s| {
            let in_degree = graph.reverse_neighbors(&s.id).len();
            (s.name.clone(), in_degree)
        })
        .collect();
    top_functions.sort_by(|a, b| b.1.cmp(&a.1));
    let top_functions: Vec<String> = top_functions.into_iter().take(5).map(|(name, _)| name).collect();

    // Edge types present within this community
    let mut edge_types: HashSet<String> = HashSet::new();
    for edge in graph.all_edges() {
        if member_set.contains(edge.source.as_str()) && member_set.contains(edge.target.as_str()) {
            edge_types.insert(format!("{:?}", edge.edge_type));
        }
    }
    let edge_types_present: Vec<String> = edge_types.into_iter().collect();

    // Cross-community deps: external files that members import/call (max 5)
    let mut external_deps: HashSet<String> = HashSet::new();
    for edge in graph.all_edges() {
        if member_set.contains(edge.source.as_str()) && !member_set.contains(edge.target.as_str()) {
            if let Some(node) = graph.get_node(&edge.target) {
                if let Some(fp) = &node.file_path {
                    external_deps.insert(fp.clone());
                }
            }
        }
    }
    let cross_community_deps: Vec<String> = external_deps.into_iter().take(5).collect();

    // Primary language by extension
    let mut ext_counts: HashMap<String, usize> = HashMap::new();
    for f in &files {
        if let Some(ext) = std::path::Path::new(f).extension().and_then(|e| e.to_str()) {
            *ext_counts.entry(ext.to_string()).or_default() += 1;
        }
    }
    let primary_language = ext_counts.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(ext, _)| ext)
        .unwrap_or_default();

    CommunitySummary {
        community_id: community.id.clone(),
        name: community.name.clone(),
        text,
        token_count,
        structured: CommunityStructuredData {
            top_functions,
            edge_types_present,
            cross_community_deps,
            file_count: files.len(),
            primary_language,
        },
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct SymbolInfo {
    id: String,
    name: String,
    signature: Option<String>,
    doc: Option<String>,
    #[allow(dead_code)]
    file: String,
    has_test: bool,
}

/// Build a call flow string: "main → run_server → handle_request"
fn build_call_flow(symbols: &[SymbolInfo], graph: &CodeGraph) -> String {
    let sym_ids: HashSet<&str> = symbols.iter().map(|s| s.id.as_str()).collect();
    let id_to_name: HashMap<&str, &str> = symbols
        .iter()
        .map(|s| (s.id.as_str(), s.name.as_str()))
        .collect();

    // Find internal call edges (both endpoints in this community)
    let mut calls: Vec<(&str, &str)> = Vec::new();
    for edge in graph.all_edges() {
        if edge.edge_type == EdgeType::Calls
            && sym_ids.contains(edge.source.as_str())
            && sym_ids.contains(edge.target.as_str())
        {
            if let (Some(src), Some(tgt)) =
                (id_to_name.get(edge.source.as_str()), id_to_name.get(edge.target.as_str()))
            {
                calls.push((src, tgt));
            }
        }
    }

    if calls.is_empty() {
        return String::new();
    }

    // Build a simple chain from calls (first 4 steps)
    let mut chain: Vec<&str> = Vec::new();
    if let Some((first_src, first_tgt)) = calls.first() {
        chain.push(first_src);
        chain.push(first_tgt);

        // Follow the chain
        for _ in 0..3 {
            let last = *chain.last().unwrap();
            if let Some((_, next)) = calls.iter().find(|(s, _)| *s == last) {
                if !chain.contains(next) {
                    chain.push(next);
                }
            }
        }
    }

    chain.join(" → ")
}

/// Find file paths of dependencies (outgoing Calls/Imports edges to nodes outside community).
fn find_dependencies(community: &Community, graph: &CodeGraph) -> Vec<String> {
    let member_ids: HashSet<&str> = community.node_ids.iter().map(|s| s.as_str()).collect();
    let mut dep_files: HashSet<String> = HashSet::new();

    for edge in graph.all_edges() {
        if (edge.edge_type == EdgeType::Calls || edge.edge_type == EdgeType::Imports)
            && member_ids.contains(edge.source.as_str())
            && !member_ids.contains(edge.target.as_str())
        {
            if let Some(node) = graph.get_node(&edge.target) {
                if let Some(fp) = &node.file_path {
                    dep_files.insert(fp.clone());
                }
            }
        }
    }

    let mut deps: Vec<String> = dep_files.into_iter().collect();
    deps.sort();
    deps.truncate(5);
    deps
}

/// Find co-change partners (files with CO_CHANGES edges to community files).
fn find_cochanges(community: &Community, graph: &CodeGraph) -> Vec<(String, f64)> {
    let member_files: HashSet<String> = community
        .node_ids
        .iter()
        .filter_map(|id| graph.get_node(id))
        .filter_map(|n| n.file_path.clone())
        .collect();

    let mut partner_weights: HashMap<String, f64> = HashMap::new();

    for edge in graph.all_edges() {
        if edge.edge_type != EdgeType::CoChanges {
            continue;
        }
        let src_file = graph
            .get_node(&edge.source)
            .and_then(|n| n.file_path.clone());
        let tgt_file = graph
            .get_node(&edge.target)
            .and_then(|n| n.file_path.clone());

        if let (Some(sf), Some(tf)) = (src_file, tgt_file) {
            if member_files.contains(&sf) && !member_files.contains(&tf) {
                let entry = partner_weights.entry(tf).or_insert(0.0);
                if edge.weight > *entry {
                    *entry = edge.weight;
                }
            }
        }
    }

    let mut partners: Vec<(String, f64)> = partner_weights.into_iter().collect();
    partners.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    partners.truncate(3);
    partners
}

/// Collect unique git messages across all files in the community.
fn collect_git_messages(
    files: &HashSet<String>,
    git_info: &HashMap<String, FileGitInfo>,
) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut messages: Vec<String> = Vec::new();

    for file in files {
        if let Some(info) = git_info.get(file.as_str()) {
            if !info.last_commit_message.is_empty() && seen.insert(info.last_commit_message.clone())
            {
                messages.push(info.last_commit_message.clone());
            }
            for msg in &info.recent_messages {
                if !msg.is_empty() && seen.insert(msg.clone()) {
                    messages.push(msg.clone());
                }
            }
        }
    }

    messages.truncate(5);
    messages
}

/// Rough token estimate: words * 1.3
fn estimate_tokens(text: &str) -> usize {
    let words = text.split_whitespace().count();
    ((words as f64) * 1.3).ceil() as usize
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::model::{Edge, Node, SymbolKind};

    fn make_test_graph() -> (CodeGraph, Vec<Community>) {
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
            signature: Some("fn verify_token(token: &str) -> Result<Claims>".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(30),
            last_modified: 1000.0,
            doc: Some("Verifies a JWT token and returns decoded claims.".into()),
        });

        // Symbol: decode_header
        graph.add_node(Node {
            id: "sym:decode_header".into(),
            node_type: NodeType::Symbol,
            name: "decode_header".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn decode_header(token: &str) -> Header".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(35),
            line_end: Some(50),
            last_modified: 1000.0,
            doc: None,
        });

        // Test node
        graph.add_node(Node {
            id: "test:test_verify".into(),
            node_type: NodeType::Test,
            name: "test_verify".into(),
            file_path: Some("tests/test_auth.rs".into()),
            signature: Some("fn test_verify()".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(10),
            last_modified: 1000.0,
            doc: None,
        });

        // Edges
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
            source: "sym:verify_token".into(),
            target: "sym:decode_header".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "test:test_verify".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Tests,
            weight: 0.7,
        });

        let communities = vec![Community {
            id: "comm_auth".into(),
            name: "auth/jwt".into(),
            level: 0,
            node_ids: vec![
                "sym:verify_token".into(),
                "sym:decode_header".into(),
                "test:test_verify".into(),
            ],
            parent_id: None,
            version: 1,
        }];

        (graph, communities)
    }

    #[test]
    fn test_summary_contains_name_and_function_count() {
        let (graph, communities) = make_test_graph();
        let summaries = generate_summaries(&communities, &graph, &HashMap::new());

        assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        assert!(s.text.contains("auth/jwt"), "should contain community name");
        assert!(s.text.contains("2 funções"), "should show function count");
    }

    #[test]
    fn test_summary_contains_call_flow() {
        let (graph, communities) = make_test_graph();
        let summaries = generate_summaries(&communities, &graph, &HashMap::new());

        let s = &summaries[0];
        assert!(
            s.text.contains("verify_token") && s.text.contains("decode_header"),
            "should show call flow"
        );
    }

    #[test]
    fn test_summary_contains_signatures() {
        let (graph, communities) = make_test_graph();
        let summaries = generate_summaries(&communities, &graph, &HashMap::new());

        let s = &summaries[0];
        assert!(
            s.text.contains("fn verify_token"),
            "should include signature"
        );
    }

    #[test]
    fn test_summary_contains_docstring() {
        let (graph, communities) = make_test_graph();
        let summaries = generate_summaries(&communities, &graph, &HashMap::new());

        let s = &summaries[0];
        assert!(
            s.text.contains("Verifies a JWT token"),
            "should include docstring"
        );
    }

    #[test]
    fn test_summary_contains_test_coverage() {
        let (graph, communities) = make_test_graph();
        let summaries = generate_summaries(&communities, &graph, &HashMap::new());

        let s = &summaries[0];
        assert!(s.text.contains("Testes:"), "should show test info");
        assert!(s.text.contains("1/2"), "should show 1 of 2 covered");
        assert!(
            s.text.contains("decode_header"),
            "should flag untested symbol"
        );
    }

    #[test]
    fn test_summary_contains_git_messages() {
        let (graph, communities) = make_test_graph();
        let mut git_info = HashMap::new();
        git_info.insert(
            "src/auth.rs".to_string(),
            FileGitInfo {
                last_commit_message: "fix: token expiry not checked".into(),
                recent_messages: vec!["feat: add JWT validation".into()],
            },
        );

        let summaries = generate_summaries(&communities, &graph, &git_info);
        let s = &summaries[0];
        assert!(
            s.text.contains("token expiry"),
            "should include git message"
        );
    }

    #[test]
    fn test_summary_token_count_positive() {
        let (graph, communities) = make_test_graph();
        let summaries = generate_summaries(&communities, &graph, &HashMap::new());

        assert!(summaries[0].token_count > 0);
    }

    #[test]
    fn test_empty_community_produces_minimal_summary() {
        let graph = CodeGraph::new();
        let communities = vec![Community {
            id: "empty".into(),
            name: "empty".into(),
            level: 0,
            node_ids: vec![],
            parent_id: None,
            version: 1,
        }];

        let summaries = generate_summaries(&communities, &graph, &HashMap::new());
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].text.contains("empty"));
    }
}
