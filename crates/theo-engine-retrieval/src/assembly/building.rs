//! Single-purpose slice extracted from `assembly.rs` (T4.3 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashSet;
use std::path::Path;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::ScoredCommunity;

use super::*;

pub fn build_code_content(
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
pub fn build_code_content_filtered(
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
                if let Some(node) = graph.get_node(node_id)
                    && node.file_path.as_deref() == Some(path.as_str()) {
                        // Check CONTAINS children
                        for edge in graph.all_edges() {
                            if edge.edge_type == EdgeType::Contains && edge.source == *node_id
                                && let Some(child) = graph.get_node(&edge.target) {
                                    let name_tokens: HashSet<String> =
                                        crate::search::tokenise(&child.name).into_iter().collect();
                                    match_count += query_tokens
                                        .iter()
                                        .filter(|qt| name_tokens.contains(*qt))
                                        .count();
                                }
                        }
                    }
            }
            // Also check the file name itself
            let path_tokens: HashSet<String> = crate::search::tokenise(path).into_iter().collect();
            match_count += query_tokens
                .iter()
                .filter(|qt| path_tokens.contains(*qt))
                .count();

            (path, ranges, match_count)
        })
        .collect();

    // Sort by match count descending, take top 3 files
    file_scores.sort_by_key(|item| std::cmp::Reverse(item.2));
    let top_files: Vec<_> = file_scores
        .into_iter()
        .filter(|(_, _, score)| *score > 0)
        .take(3)
        .collect();

    if top_files.is_empty() {
        // Fallback: return summary only
        return format!(
            "## {} -- {} files\n{}",
            community.name,
            file_symbols.len(),
            summary_text
        );
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "## {} -- {} relevant files (of {})",
        community.name,
        top_files.len(),
        file_symbols.len()
    ));

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
pub fn build_signature_content(community: &Community, graph: &CodeGraph) -> String {
    let file_symbols = collect_file_symbols(community, graph);
    let file_count = file_symbols.len();

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("## {} -- {} files", community.name, file_count));

    // Collect unique signatures from community nodes
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id)
            && (node.node_type == NodeType::Symbol || node.node_type == NodeType::Test)
                && let Some(sig) = &node.signature {
                    lines.push(sig.clone());
                }
    }

    lines.join("\n")
}

/// Build a compressed semantic representation for a community.
///
/// Uses intent-aware compression to produce ~8-line summaries per symbol
/// that capture calls, errors, test coverage, and co-changes. Sits between
/// full code (rank 0-1) and signature-only (rank 4+) in information density.
pub fn build_compressed_content(community: &Community, graph: &CodeGraph) -> String {
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
