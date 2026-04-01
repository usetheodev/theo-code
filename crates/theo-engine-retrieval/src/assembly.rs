/// Greedy knapsack context assembly.
///
/// Converts scored communities into a `ContextPayload` that fits within a
/// token budget. Items are ranked by value density (score / token_count) and
/// filled greedily until the budget is exhausted.

use std::collections::HashSet;
use std::path::Path;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::ScoredCommunity;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A context item ready to be sent to the LLM.
pub struct ContextItem {
    pub community_id: String,
    pub content: String,
    pub token_count: usize,
    pub score: f64,
}

/// The assembled context payload.
pub struct ContextPayload {
    pub items: Vec<ContextItem>,
    pub total_tokens: usize,
    pub budget_tokens: usize,
    /// Comma-separated names of excluded communities (exploration hints).
    pub exploration_hints: String,
}

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

/// Rough token count: word_count * 1.3, ceiling.
fn estimate_tokens(text: &str) -> usize {
    let words = text.split_whitespace().count();
    ((words as f64) * 1.3).ceil() as usize
}

// ---------------------------------------------------------------------------
// Content generation
// ---------------------------------------------------------------------------

/// Build the text representation for a community from its node signatures.
fn community_content(community: &Community, graph: &CodeGraph) -> String {
    let mut lines: Vec<String> = vec![format!("# {}", community.name)];
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            if let Some(sig) = &node.signature {
                lines.push(sig.clone());
            } else {
                lines.push(node.name.clone());
            }
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------------------

/// Greedy knapsack: sort by score/tokens (value density), fill until budget.
pub fn assemble_greedy(
    scored: &[ScoredCommunity],
    graph: &CodeGraph,
    budget_tokens: usize,
) -> ContextPayload {
    if budget_tokens == 0 || scored.is_empty() {
        // Collect exclusion hints for all communities.
        let hints: Vec<String> = scored.iter().map(|s| s.community.name.clone()).collect();
        return ContextPayload {
            items: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: hints.join(", "),
        };
    }

    // Build candidate items with content and token counts.
    struct Candidate {
        community_id: String,
        community_name: String,
        content: String,
        token_count: usize,
        score: f64,
        density: f64,
    }

    let mut candidates: Vec<Candidate> = scored
        .iter()
        .map(|s| {
            let content = community_content(&s.community, graph);
            let token_count = estimate_tokens(&content).max(1); // floor at 1 to avoid div/0
            let density = s.score / token_count as f64;
            Candidate {
                community_id: s.community.id.clone(),
                community_name: s.community.name.clone(),
                content,
                token_count,
                score: s.score,
                density,
            }
        })
        .collect();

    // Sort descending by density (value density = score / cost).
    candidates.sort_by(|a, b| b.density.partial_cmp(&a.density).unwrap_or(std::cmp::Ordering::Equal));

    let mut items: Vec<ContextItem> = Vec::new();
    let mut total_tokens = 0usize;
    let mut excluded_names: Vec<String> = Vec::new();

    for candidate in candidates {
        if total_tokens + candidate.token_count <= budget_tokens {
            total_tokens += candidate.token_count;
            items.push(ContextItem {
                community_id: candidate.community_id,
                content: candidate.content,
                token_count: candidate.token_count,
                score: candidate.score,
            });
        } else {
            excluded_names.push(candidate.community_name);
        }
    }

    let exploration_hints = excluded_names.join(", ");

    ContextPayload {
        items,
        total_tokens,
        budget_tokens,
        exploration_hints,
    }
}

/// Assemble context using pre-generated summaries instead of raw symbol dumps.
///
/// This produces human-readable, contextualised text that LLMs understand immediately.
pub fn assemble_with_summaries(
    scored: &[ScoredCommunity],
    summaries: &std::collections::HashMap<String, crate::summary::CommunitySummary>,
    budget_tokens: usize,
) -> ContextPayload {
    if budget_tokens == 0 || scored.is_empty() {
        let hints: Vec<String> = scored.iter().map(|s| s.community.name.clone()).collect();
        return ContextPayload {
            items: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: hints.join(", "),
        };
    }

    struct Candidate {
        community_id: String,
        community_name: String,
        content: String,
        token_count: usize,
        score: f64,
        density: f64,
    }

    let mut candidates: Vec<Candidate> = scored
        .iter()
        .map(|s| {
            let (content, token_count) = if let Some(summary) = summaries.get(&s.community.id) {
                (summary.text.clone(), summary.token_count.max(1))
            } else {
                let fallback = format!("## {} (no summary available)", s.community.name);
                let tc = estimate_tokens(&fallback).max(1);
                (fallback, tc)
            };
            let density = s.score / token_count as f64;
            Candidate {
                community_id: s.community.id.clone(),
                community_name: s.community.name.clone(),
                content,
                token_count,
                score: s.score,
                density,
            }
        })
        .collect();

    candidates.sort_by(|a, b| b.density.partial_cmp(&a.density).unwrap_or(std::cmp::Ordering::Equal));

    let mut items: Vec<ContextItem> = Vec::new();
    let mut total_tokens = 0usize;
    let mut excluded_names: Vec<String> = Vec::new();

    for candidate in candidates {
        if total_tokens + candidate.token_count <= budget_tokens {
            total_tokens += candidate.token_count;
            items.push(ContextItem {
                community_id: candidate.community_id,
                content: candidate.content,
                token_count: candidate.token_count,
                score: candidate.score,
            });
        } else {
            excluded_names.push(candidate.community_name);
        }
    }

    ContextPayload {
        items,
        total_tokens,
        budget_tokens,
        exploration_hints: excluded_names.join(", "),
    }
}

// ---------------------------------------------------------------------------
// Code-augmented assembly
// ---------------------------------------------------------------------------

/// Maximum lines for including a full file without truncation.
const FULL_FILE_LINE_THRESHOLD: usize = 100;

/// Represents a symbol's location within a file for code extraction.
struct SymbolRange {
    name: String,
    line_start: usize,
    line_end: usize,
}

/// Infer the language identifier for fenced code blocks from a file extension.
fn language_from_path(path: &str) -> &str {
    if let Some(ext) = path.rsplit('.').next() {
        match ext {
            "rs" => "rust",
            "py" => "python",
            "ts" => "typescript",
            "tsx" => "tsx",
            "js" => "javascript",
            "jsx" => "jsx",
            "go" => "go",
            "java" => "java",
            "rb" => "ruby",
            "c" | "h" => "c",
            "cpp" | "hpp" | "cc" | "cxx" => "cpp",
            "cs" => "csharp",
            "sh" | "bash" | "zsh" => "bash",
            "yaml" | "yml" => "yaml",
            "toml" => "toml",
            "json" => "json",
            "sql" => "sql",
            _ => ext,
        }
    } else {
        ""
    }
}

/// Collect unique file paths and their community-relevant symbol ranges from a community.
///
/// Returns a map of file_path -> Vec<SymbolRange> (sorted by line_start).
fn collect_file_symbols(
    community: &Community,
    graph: &CodeGraph,
) -> std::collections::HashMap<String, Vec<SymbolRange>> {
    let mut file_symbols: std::collections::HashMap<String, Vec<SymbolRange>> =
        std::collections::HashMap::new();

    // Collect file paths from file nodes in the community
    let mut community_files: HashSet<String> = HashSet::new();
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            if let Some(fp) = &node.file_path {
                community_files.insert(fp.clone());
            }
        }
    }

    // Also find files via Contains edges from file nodes
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            if node.node_type == NodeType::File {
                if let Some(fp) = &node.file_path {
                    community_files.insert(fp.clone());
                }
            }
        }
    }

    // Collect symbol ranges per file.
    // For file-level communities, members are File nodes — follow CONTAINS edges
    // to find the Symbol nodes within each file.
    use theo_engine_graph::model::EdgeType;
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            // Direct symbol/test nodes (symbol-level communities)
            if node.node_type == NodeType::Symbol || node.node_type == NodeType::Test {
                if let (Some(fp), Some(ls), Some(le)) =
                    (&node.file_path, node.line_start, node.line_end)
                {
                    file_symbols
                        .entry(fp.clone())
                        .or_default()
                        .push(SymbolRange {
                            name: node.name.clone(),
                            line_start: ls,
                            line_end: le,
                        });
                }
            }
            // File nodes — follow CONTAINS edges to get their symbols
            if node.node_type == NodeType::File {
                for edge in graph.all_edges() {
                    if edge.edge_type == EdgeType::Contains && edge.source == *node_id {
                        if let Some(child) = graph.get_node(&edge.target) {
                            if let (Some(fp), Some(ls), Some(le)) =
                                (&child.file_path, child.line_start, child.line_end)
                            {
                                file_symbols
                                    .entry(fp.clone())
                                    .or_default()
                                    .push(SymbolRange {
                                        name: child.name.clone(),
                                        line_start: ls,
                                        line_end: le,
                                    });
                            }
                        }
                    }
                }
            }
        }
    }

    // Ensure all community files appear even without explicit symbol ranges
    for fp in &community_files {
        file_symbols.entry(fp.clone()).or_default();
    }

    // Sort ranges by line_start for each file
    for ranges in file_symbols.values_mut() {
        ranges.sort_by_key(|r| r.line_start);
    }

    file_symbols
}

/// Read a file and extract relevant code content.
///
/// - If the file is shorter than `FULL_FILE_LINE_THRESHOLD` lines, include everything.
/// - Otherwise, include only the symbol ranges with `// ... (N lines omitted)` markers.
fn read_file_content(
    file_path: &str,
    repo_root: &Path,
    symbol_ranges: &[SymbolRange],
) -> Option<String> {
    read_file_content_filtered(file_path, repo_root, symbol_ranges, &HashSet::new())
}

/// Read a file and extract relevant code content, prioritizing functions
/// whose names match the query terms.
///
/// For large files (>100 lines), only includes the most relevant symbol ranges.
/// Relevance = number of query terms that appear in the symbol name.
fn read_file_content_filtered(
    file_path: &str,
    repo_root: &Path,
    symbol_ranges: &[SymbolRange],
    query_tokens: &HashSet<String>,
) -> Option<String> {
    let full_path = repo_root.join(file_path);
    let source = std::fs::read_to_string(&full_path).ok()?;
    let lines: Vec<&str> = source.lines().collect();

    if lines.len() < FULL_FILE_LINE_THRESHOLD || symbol_ranges.is_empty() {
        return Some(source);
    }

    // Score each symbol range by query match relevance.
    // Production functions (not test_*) get a +10 boost to be prioritized over tests.
    let mut scored_ranges: Vec<(usize, &SymbolRange)> = symbol_ranges
        .iter()
        .map(|r| {
            let name_lower = r.name.to_lowercase();
            let is_test = name_lower.starts_with("test_")
                || name_lower.starts_with("test ")
                || name_lower.contains("_test_");
            let base_score = if is_test { 0 } else { 10 }; // production code first

            if query_tokens.is_empty() {
                return (base_score, r);
            }
            let name_tokens: HashSet<String> =
                crate::search::tokenise(&r.name).into_iter().collect();
            let matches = query_tokens
                .iter()
                .filter(|qt| name_tokens.contains(*qt))
                .count();
            (base_score + matches, r)
        })
        .collect();

    // Sort by score descending, then by line_start (earlier first)
    scored_ranges.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.line_start.cmp(&b.1.line_start)));

    // Take top 8 most relevant ranges — enough to cover key functions
    let max_ranges = 8;
    let selected: Vec<&SymbolRange> = scored_ranges
        .iter()
        .take(max_ranges)
        .map(|(_, r)| *r)
        .collect();

    // Sort selected by line_start for coherent output
    let mut sorted_selected = selected;
    sorted_selected.sort_by_key(|r| r.line_start);

    let skipped_count = symbol_ranges.len().saturating_sub(max_ranges);

    let mut output_lines: Vec<String> = Vec::new();
    let mut last_end: usize = 0;

    for range in &sorted_selected {
        let start = range.line_start.saturating_sub(1).min(lines.len());
        let end = range.line_end.min(lines.len());

        if start > last_end {
            let omitted = start - last_end;
            output_lines.push(format!("// ... ({} lines omitted)", omitted));
        }

        for line in &lines[start..end] {
            output_lines.push(line.to_string());
        }

        last_end = end;
    }

    if last_end < lines.len() {
        let omitted = lines.len() - last_end;
        output_lines.push(format!("// ... ({} lines omitted)", omitted));
    }

    if skipped_count > 0 {
        output_lines.push(format!("// ... and {} more functions", skipped_count));
    }

    Some(output_lines.join("\n"))
}

/// Build a code-augmented content string for a community.
///
/// Format:
/// ```text
/// ## {community_name} -- {file_count} files
/// {summary_line}
///
/// ### {file_path}
/// ```{language}
/// {code}
/// ```
/// ```
fn build_code_content(
    community: &Community,
    summary_text: &str,
    graph: &CodeGraph,
    repo_root: &Path,
) -> String {
    let file_symbols = collect_file_symbols(community, graph);
    let file_count = file_symbols.len();

    let mut lines: Vec<String> = Vec::new();

    // Header with summary
    lines.push(format!("## {} -- {} files", community.name, file_count));

    // Include the first line of the summary (the flow/dependency info) as a brief header
    let summary_first_lines: Vec<&str> = summary_text
        .lines()
        .filter(|l| {
            let trimmed = l.trim();
            !trimmed.is_empty() && !trimmed.starts_with("## ")
        })
        .take(3)
        .collect();
    if !summary_first_lines.is_empty() {
        lines.push(summary_first_lines.join("\n"));
    }

    // Sort files for deterministic output
    let mut sorted_files: Vec<(&String, &Vec<SymbolRange>)> = file_symbols.iter().collect();
    sorted_files.sort_by_key(|(path, _)| path.as_str());

    for (file_path, ranges) in sorted_files {
        if let Some(code) = read_file_content(file_path, repo_root, ranges) {
            let lang = language_from_path(file_path);
            lines.push(String::new());
            lines.push(format!("### {}", file_path));
            lines.push(format!("```{}", lang));
            lines.push(code);
            lines.push("```".to_string());
        }
    }

    lines.join("\n")
}

/// Like `build_code_content` but only includes files whose symbols match the query.
/// For large communities, this prevents returning 20 irrelevant files when only 1 matches.
fn build_code_content_filtered(
    community: &Community,
    summary_text: &str,
    graph: &CodeGraph,
    repo_root: &Path,
    query_tokens: &HashSet<String>,
) -> String {
    use theo_engine_graph::model::EdgeType;

    let file_symbols = collect_file_symbols(community, graph);

    // Score each file by how many query terms appear in its symbols
    let mut file_scores: Vec<(&String, &Vec<SymbolRange>, usize)> = file_symbols
        .iter()
        .map(|(path, ranges)| {
            // Get symbol names for this file
            let mut match_count = 0usize;
            for node_id in &community.node_ids {
                if let Some(node) = graph.get_node(node_id) {
                    if node.file_path.as_deref() == Some(path.as_str()) {
                        // Check CONTAINS children
                        for edge in graph.all_edges() {
                            if edge.edge_type == EdgeType::Contains && edge.source == *node_id {
                                if let Some(child) = graph.get_node(&edge.target) {
                                    let name_tokens: HashSet<String> = crate::search::tokenise(&child.name).into_iter().collect();
                                    match_count += query_tokens.iter().filter(|qt| name_tokens.contains(*qt)).count();
                                }
                            }
                        }
                    }
                }
            }
            // Also check the file name itself
            let path_tokens: HashSet<String> = crate::search::tokenise(path).into_iter().collect();
            match_count += query_tokens.iter().filter(|qt| path_tokens.contains(*qt)).count();

            (path, ranges, match_count)
        })
        .collect();

    // Sort by match count descending, take top 3 files
    file_scores.sort_by(|a, b| b.2.cmp(&a.2));
    let top_files: Vec<_> = file_scores.into_iter().filter(|(_, _, score)| *score > 0).take(3).collect();

    if top_files.is_empty() {
        // Fallback: return summary only
        return format!("## {} -- {} files\n{}", community.name, file_symbols.len(), summary_text);
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("## {} -- {} relevant files (of {})", community.name, top_files.len(), file_symbols.len()));

    let summary_first_lines: Vec<&str> = summary_text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with("## "))
        .take(2)
        .collect();
    if !summary_first_lines.is_empty() {
        lines.push(summary_first_lines.join("\n"));
    }

    for (file_path, ranges, _) in &top_files {
        if let Some(code) = read_file_content_filtered(file_path, repo_root, ranges, query_tokens) {
            let lang = language_from_path(file_path);
            lines.push(String::new());
            lines.push(format!("### {}", file_path));
            lines.push(format!("```{}", lang));
            lines.push(code);
            lines.push("```".to_string());
        }
    }

    lines.join("\n")
}

/// Build a compact signature-only representation for a community.
///
/// Returns just the community name, file count, and function signatures — no full source code.
/// Used for items ranked 3+ to drastically reduce token usage.
fn build_signature_content(community: &Community, graph: &CodeGraph) -> String {
    let file_symbols = collect_file_symbols(community, graph);
    let file_count = file_symbols.len();

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("## {} -- {} files", community.name, file_count));

    // Collect unique signatures from community nodes
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            if node.node_type == NodeType::Symbol || node.node_type == NodeType::Test {
                if let Some(sig) = &node.signature {
                    lines.push(sig.clone());
                }
            }
        }
    }

    lines.join("\n")
}

/// Build a compressed semantic representation for a community.
///
/// Uses intent-aware compression to produce ~8-line summaries per symbol
/// that capture calls, errors, test coverage, and co-changes. Sits between
/// full code (rank 0-1) and signature-only (rank 4+) in information density.
fn build_compressed_content(community: &Community, graph: &CodeGraph) -> String {
    let file_symbols = collect_file_symbols(community, graph);
    let file_count = file_symbols.len();

    let compressed = crate::compress::compress_community(community, graph);

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "## {} -- {} files (compressed)",
        community.name, file_count
    ));

    for sym in &compressed {
        lines.push(String::new());
        lines.push(crate::compress::format_compressed(sym));
    }

    lines.join("\n")
}

/// Assemble context with real source code: use summaries for ranking,
/// but include actual source code in the output.
///
/// Flow:
/// 1. Score communities with BM25 (done by caller — `scored` is pre-ranked)
/// 2. For each top-scored community, collect its file paths from the graph
/// 3. Read the actual source files from disk
/// 4. Build context items that contain: summary header + actual code
/// 5. Pack into token budget using greedy knapsack
pub fn assemble_with_code(
    scored: &[ScoredCommunity],
    summaries: &std::collections::HashMap<String, crate::summary::CommunitySummary>,
    graph: &CodeGraph,
    repo_root: &Path,
    budget_tokens: usize,
    query: &str,
) -> ContextPayload {
    if budget_tokens == 0 || scored.is_empty() {
        let hints: Vec<String> = scored.iter().map(|s| s.community.name.clone()).collect();
        return ContextPayload {
            items: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: hints.join(", "),
        };
    }

    struct Candidate {
        community_id: String,
        community_name: String,
        content: String,
        token_count: usize,
        score: f64,
    }

    // Tokenize query for file-level filtering
    let query_tokens: HashSet<String> = crate::search::tokenise(query).into_iter().collect();

    // Sort scored communities by score descending to determine rank
    let mut scored_sorted: Vec<&ScoredCommunity> = scored.iter().collect();
    scored_sorted.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Build candidates with tiered content strategy:
    // - Rank 0-1: full source code (highest fidelity)
    // - Rank 2-3: compressed semantic representation for large communities,
    //             full code for small ones
    // - Rank 4+: signature-only (minimal tokens)
    let symbol_count_threshold = 100;
    let file_count_threshold = 10;

    let mut candidates: Vec<Candidate> = scored_sorted
        .iter()
        .enumerate()
        .map(|(rank, s)| {
            let content = if rank < 2 {
                // Top-2: always include FULL code
                let summary_text = summaries
                    .get(&s.community.id)
                    .map(|sum| sum.text.as_str())
                    .unwrap_or("");

                if s.community.node_ids.len() > 5 {
                    build_code_content_filtered(&s.community, summary_text, graph, repo_root, &query_tokens)
                } else {
                    build_code_content(&s.community, summary_text, graph, repo_root)
                }
            } else if rank < 4 {
                // Rank 2-3: use compressed representations for large communities
                let symbol_count = s.community.node_ids.iter().filter(|id| {
                    graph.get_node(id).map_or(false, |n| n.node_type == NodeType::Symbol)
                }).count();
                let file_count = collect_file_symbols(&s.community, graph).len();

                if symbol_count > symbol_count_threshold || file_count > file_count_threshold {
                    build_compressed_content(&s.community, graph)
                } else {
                    let summary_text = summaries
                        .get(&s.community.id)
                        .map(|sum| sum.text.as_str())
                        .unwrap_or("");
                    if s.community.node_ids.len() > 5 {
                        build_code_content_filtered(&s.community, summary_text, graph, repo_root, &query_tokens)
                    } else {
                        build_code_content(&s.community, summary_text, graph, repo_root)
                    }
                }
            } else {
                // Rank 4+: signature-only (no full source code)
                build_signature_content(&s.community, graph)
            };

            let token_count = estimate_tokens(&content).max(1);

            Candidate {
                community_id: s.community.id.clone(),
                community_name: s.community.name.clone(),
                content,
                token_count,
                score: s.score,
            }
        })
        .collect();

    // Two-phase selection:
    // Phase 1: Top-2 by absolute score (full code, ensures most relevant items come first)
    // Phase 2: Fill remaining budget by score (position-aware ordering)
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut items: Vec<ContextItem> = Vec::new();
    let mut total_tokens = 0usize;
    let mut excluded_names: Vec<String> = Vec::new();
    let mut selected_ids: HashSet<String> = HashSet::new();

    // Phase 1: Force-include top-3 by score with full code
    for candidate in candidates.iter().take(3) {
        if total_tokens + candidate.token_count <= budget_tokens {
            total_tokens += candidate.token_count;
            selected_ids.insert(candidate.community_id.clone());
            items.push(ContextItem {
                community_id: candidate.community_id.clone(),
                content: candidate.content.clone(),
                token_count: candidate.token_count,
                score: candidate.score,
            });
        }
    }

    // Phase 2: Fill remaining budget by score descending (not density),
    // ensuring the LLM sees the most relevant items first
    for candidate in &candidates {
        if selected_ids.contains(&candidate.community_id) {
            continue;
        }
        if total_tokens + candidate.token_count <= budget_tokens {
            total_tokens += candidate.token_count;
            items.push(ContextItem {
                community_id: candidate.community_id.clone(),
                content: candidate.content.clone(),
                token_count: candidate.token_count,
                score: candidate.score,
            });
        } else {
            excluded_names.push(candidate.community_name.clone());
        }
    }

    ContextPayload {
        items,
        total_tokens,
        budget_tokens,
        exploration_hints: excluded_names.join(", "),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::CommunitySummary;
    use std::collections::HashMap;
    use std::io::Write;
    use theo_engine_graph::cluster::Community;
    use theo_engine_graph::model::{Edge, EdgeType, Node, SymbolKind};

    /// Helper: create a graph with one community containing two files, a summary,
    /// and write the source files to a temp directory.
    fn setup_code_test() -> (
        Vec<ScoredCommunity>,
        HashMap<String, CommunitySummary>,
        CodeGraph,
        tempfile::TempDir,
    ) {
        let tmp_dir = tempfile::tempdir().unwrap();

        // Create source files on disk
        let src_dir = tmp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        let mut f1 = std::fs::File::create(src_dir.join("auth.rs")).unwrap();
        writeln!(
            f1,
            "fn verify_token(token: &str) -> bool {{\n    token.len() > 0\n}}\n\nfn decode(t: &str) -> String {{\n    t.to_string()\n}}"
        )
        .unwrap();

        let mut f2 = std::fs::File::create(src_dir.join("handler.rs")).unwrap();
        writeln!(f2, "fn handle(req: Request) -> Response {{\n    todo!()\n}}").unwrap();

        // Build graph
        let mut graph = CodeGraph::new();

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
        graph.add_node(Node {
            id: "sym:verify_token".into(),
            node_type: NodeType::Symbol,
            name: "verify_token".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn verify_token(token: &str) -> bool".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(3),
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:decode".into(),
            node_type: NodeType::Symbol,
            name: "decode".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn decode(t: &str) -> String".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(5),
            line_end: Some(7),
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "file:src/handler.rs".into(),
            node_type: NodeType::File,
            name: "src/handler.rs".into(),
            file_path: Some("src/handler.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:handle".into(),
            node_type: NodeType::Symbol,
            name: "handle".into(),
            file_path: Some("src/handler.rs".into()),
            signature: Some("fn handle(req: Request) -> Response".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(3),
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
            target: "sym:decode".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "file:src/handler.rs".into(),
            target: "sym:handle".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "sym:verify_token".into(),
            target: "sym:decode".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        let community = Community {
            id: "comm_auth".into(),
            name: "auth/jwt".into(),
            level: 0,
            node_ids: vec![
                "file:src/auth.rs".into(),
                "sym:verify_token".into(),
                "sym:decode".into(),
                "file:src/handler.rs".into(),
                "sym:handle".into(),
            ],
            parent_id: None,
            version: 1,
        };

        let scored = vec![ScoredCommunity {
            community: community.clone(),
            score: 5.0,
        }];

        let mut summaries = HashMap::new();
        summaries.insert(
            "comm_auth".into(),
            CommunitySummary {
                community_id: "comm_auth".into(),
                name: "auth/jwt".into(),
                text: "## auth/jwt (3 funções, 10 linhas, src/auth.rs, src/handler.rs)\n\nFluxo: verify_token → decode".into(),
                token_count: 20,
            },
        );

        (scored, summaries, graph, tmp_dir)
    }

    #[test]
    fn test_assemble_with_code_includes_source() {
        let (scored, summaries, graph, tmp_dir) = setup_code_test();

        let payload = assemble_with_code(&scored, &summaries, &graph, tmp_dir.path(), 50_000, "test query");

        assert!(!payload.items.is_empty(), "should produce at least one item");

        let content = &payload.items[0].content;

        // Should contain actual source code, not just summaries
        assert!(
            content.contains("fn verify_token(token: &str) -> bool"),
            "should contain actual source code from auth.rs, got: {}",
            content
        );
        assert!(
            content.contains("fn handle(req: Request) -> Response"),
            "should contain actual source code from handler.rs, got: {}",
            content
        );

        // Should contain fenced code blocks
        assert!(
            content.contains("```rust"),
            "should have rust fenced code blocks, got: {}",
            content
        );

        // Should contain file path headers
        assert!(
            content.contains("### src/auth.rs"),
            "should have file path header, got: {}",
            content
        );

        // Should contain the community header
        assert!(
            content.contains("## auth/jwt -- 2 files"),
            "should have community header, got: {}",
            content
        );
    }

    #[test]
    fn test_assemble_with_code_respects_budget() {
        let (scored, summaries, graph, tmp_dir) = setup_code_test();

        // Use a very small budget — should cap the total tokens
        let tiny_budget = 10;
        let payload = assemble_with_code(&scored, &summaries, &graph, tmp_dir.path(), tiny_budget, "test query");

        assert!(
            payload.total_tokens <= tiny_budget,
            "total_tokens ({}) should be <= budget ({})",
            payload.total_tokens,
            tiny_budget
        );
        assert_eq!(payload.budget_tokens, tiny_budget);
    }

    #[test]
    fn test_assemble_with_code_large_file_truncation() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let src_dir = tmp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Create a file with 300 lines
        let mut f = std::fs::File::create(src_dir.join("big.rs")).unwrap();
        for i in 1..=300 {
            writeln!(f, "// line {}", i).unwrap();
        }

        let mut graph = CodeGraph::new();
        graph.add_node(Node {
            id: "file:src/big.rs".into(),
            node_type: NodeType::File,
            name: "src/big.rs".into(),
            file_path: Some("src/big.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:func_a".into(),
            node_type: NodeType::Symbol,
            name: "func_a".into(),
            file_path: Some("src/big.rs".into()),
            signature: Some("fn func_a()".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(20),
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:func_b".into(),
            node_type: NodeType::Symbol,
            name: "func_b".into(),
            file_path: Some("src/big.rs".into()),
            signature: Some("fn func_b()".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(250),
            line_end: Some(260),
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_edge(Edge {
            source: "file:src/big.rs".into(),
            target: "sym:func_a".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "file:src/big.rs".into(),
            target: "sym:func_b".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        let community = Community {
            id: "comm_big".into(),
            name: "big/module".into(),
            level: 0,
            node_ids: vec![
                "file:src/big.rs".into(),
                "sym:func_a".into(),
                "sym:func_b".into(),
            ],
            parent_id: None,
            version: 1,
        };

        let scored = vec![ScoredCommunity {
            community: community,
            score: 5.0,
        }];

        let mut summaries = HashMap::new();
        summaries.insert(
            "comm_big".into(),
            CommunitySummary {
                community_id: "comm_big".into(),
                name: "big/module".into(),
                text: "## big/module".into(),
                token_count: 5,
            },
        );

        let payload = assemble_with_code(&scored, &summaries, &graph, tmp_dir.path(), 50_000, "test query");

        assert!(!payload.items.is_empty());
        let content = &payload.items[0].content;

        // Should have omission markers since file is > 100 lines
        assert!(
            content.contains("lines omitted"),
            "should contain omission markers for large file, got: {}",
            content
        );

        // Should still contain the symbol ranges
        assert!(
            content.contains("// line 10"),
            "should contain lines from func_a range"
        );
        assert!(
            content.contains("// line 250"),
            "should contain lines from func_b range"
        );
    }
}
