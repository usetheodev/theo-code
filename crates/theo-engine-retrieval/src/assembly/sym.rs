//! Single-purpose slice extracted from `assembly.rs` (T4.3 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashSet;
use std::path::Path;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::ScoredCommunity;

use super::*;

pub fn assemble_by_symbol(
    symbol_query: &str,
    graph: &CodeGraph,
    budget_tokens: usize,
) -> ContextPayload {
    if symbol_query.is_empty() || budget_tokens == 0 {
        return ContextPayload {
            items: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: String::new(),
        };
    }

    let query_lower = symbol_query.to_lowercase();
    let query_tokens: Vec<&str> = query_lower.split_whitespace().collect();

    // Collect candidate symbols: exact match on any query token, or substring match.
    let mut candidates: Vec<(&str, &str, f64)> = Vec::new(); // (node_id, file_path, score)

    for node in graph.symbol_nodes() {
        let name_lower = node.name.to_lowercase();
        let file_path = node.file_path.as_deref().unwrap_or("");

        // Score: exact token match > substring > no match
        let mut score = 0.0;
        for token in &query_tokens {
            if name_lower == *token {
                score += 2.0; // exact match
            } else if name_lower.contains(token) {
                score += 1.0; // substring
            } else if token.len() >= 3 && name_lower.contains(token) {
                score += 0.5;
            }
        }

        if score > 0.0 {
            // Boost by in-degree (number of callers — hubs are more important)
            let in_degree = graph.reverse_neighbors(&node.id).len() as f64;
            score += in_degree.min(5.0) * 0.1; // cap at 0.5 bonus
            candidates.push((&node.id, file_path, score));
        }
    }

    if candidates.is_empty() {
        return ContextPayload {
            items: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: format!("No symbols matching '{}' found. Try grep.", symbol_query),
        };
    }

    // Sort by score descending
    candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // Deduplicate by file — one entry per file, highest scored symbol wins
    let mut seen_files: HashSet<&str> = HashSet::new();
    let mut items: Vec<ContextItem> = Vec::new();
    let mut total_tokens = 0;

    for (node_id, file_path, score) in &candidates {
        if file_path.is_empty() || !seen_files.insert(file_path) {
            continue;
        }

        // Build content: file header + all symbol signatures in this file
        let mut lines = vec![format!("## {}", file_path)];
        let children = graph.contains_children(&format!("file:{}", file_path));
        if children.is_empty() {
            // No contains relationship — just show the matching symbol
            if let Some(node) = graph.get_node(node_id) {
                lines.push(node.signature.as_deref().unwrap_or(&node.name).to_string());
            }
        } else {
            for child_id in children {
                if let Some(child) = graph.get_node(child_id) {
                    let text = child.signature.as_deref().unwrap_or(&child.name);
                    lines.push(text.to_string());
                }
            }
        }

        let content = lines.join("\n");
        let token_count = estimate_tokens(&content).max(1);

        if total_tokens + token_count > budget_tokens {
            break;
        }

        total_tokens += token_count;
        items.push(ContextItem {
            community_id: format!("symbol:{}", file_path),
            content,
            token_count,
            score: *score,
        });

        if items.len() >= 10 {
            break; // Cap at 10 files for symbol-first
        }
    }

    ContextPayload {
        items,
        total_tokens,
        budget_tokens,
        exploration_hints: String::new(),
    }
}

// ---------------------------------------------------------------------------
// File-direct assembly (FAANG pattern: rank files, annotate with community)
