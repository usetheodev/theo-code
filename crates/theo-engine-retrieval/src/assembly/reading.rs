//! Single-purpose slice extracted from `assembly.rs` (T4.3 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashSet;
use std::path::Path;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::ScoredCommunity;

use super::*;

const FULL_FILE_LINE_THRESHOLD: usize = 100;

pub struct SymbolRange {
    pub name: String,
    pub line_start: usize,
    pub line_end: usize,
}


pub fn language_from_path(path: &str) -> &str {
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
pub fn collect_file_symbols(
    community: &Community,
    graph: &CodeGraph,
) -> std::collections::HashMap<String, Vec<SymbolRange>> {
    let mut file_symbols: std::collections::HashMap<String, Vec<SymbolRange>> =
        std::collections::HashMap::new();

    // Collect file paths from file nodes in the community
    let mut community_files: HashSet<String> = HashSet::new();
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id)
            && let Some(fp) = &node.file_path {
                community_files.insert(fp.clone());
            }
    }

    // Also find files via Contains edges from file nodes
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id)
            && node.node_type == NodeType::File
                && let Some(fp) = &node.file_path {
                    community_files.insert(fp.clone());
                }
    }

    // Collect symbol ranges per file.
    // For file-level communities, members are File nodes — follow CONTAINS edges
    // to find the Symbol nodes within each file.
    use theo_engine_graph::model::EdgeType;
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            // Direct symbol/test nodes (symbol-level communities)
            if (node.node_type == NodeType::Symbol || node.node_type == NodeType::Test)
                && let (Some(fp), Some(ls), Some(le)) =
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
            // File nodes — follow CONTAINS edges to get their symbols
            if node.node_type == NodeType::File {
                for edge in graph.all_edges() {
                    if edge.edge_type == EdgeType::Contains && edge.source == *node_id
                        && let Some(child) = graph.get_node(&edge.target)
                            && let (Some(fp), Some(ls), Some(le)) =
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
pub fn read_file_content(
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
pub fn read_file_content_filtered(
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
