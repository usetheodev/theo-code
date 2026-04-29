//! Single-purpose slice extracted from `assembly.rs` (T4.3 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashSet;
use std::path::Path;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::ScoredCommunity;

use super::*;

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
            let content = build_candidate_content(
                rank,
                s,
                summaries,
                graph,
                repo_root,
                &query_tokens,
                symbol_count_threshold,
                file_count_threshold,
            );
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

/// Tiered content strategy by rank:
///   rank 0-1 → full source code (highest fidelity)
///   rank 2-3 → compressed if large, full if small
///   rank 4+  → signature-only
fn build_candidate_content(
    rank: usize,
    s: &&ScoredCommunity,
    summaries: &std::collections::HashMap<String, crate::summary::CommunitySummary>,
    graph: &CodeGraph,
    repo_root: &Path,
    query_tokens: &HashSet<String>,
    symbol_count_threshold: usize,
    file_count_threshold: usize,
) -> String {
    if rank < 2 {
        return full_code_content(s, summaries, graph, repo_root, query_tokens);
    }
    if rank < 4 {
        let symbol_count = s
            .community
            .node_ids
            .iter()
            .filter(|id| {
                graph
                    .get_node(id)
                    .is_some_and(|n| n.node_type == NodeType::Symbol)
            })
            .count();
        let file_count = collect_file_symbols(&s.community, graph).len();
        if symbol_count > symbol_count_threshold || file_count > file_count_threshold {
            return build_compressed_content(&s.community, graph);
        }
        return full_code_content(s, summaries, graph, repo_root, query_tokens);
    }
    build_signature_content(&s.community, graph)
}

/// Full code: filtered if community is large (>5 nodes), else complete.
fn full_code_content(
    s: &&ScoredCommunity,
    summaries: &std::collections::HashMap<String, crate::summary::CommunitySummary>,
    graph: &CodeGraph,
    repo_root: &Path,
    query_tokens: &HashSet<String>,
) -> String {
    let summary_text = summaries
        .get(&s.community.id)
        .map(|sum| sum.text.as_str())
        .unwrap_or("");
    if s.community.node_ids.len() > 5 {
        build_code_content_filtered(&s.community, summary_text, graph, repo_root, query_tokens)
    } else {
        build_code_content(&s.community, summary_text, graph, repo_root)
    }
}

// ---------------------------------------------------------------------------
// Symbol-first assembly
