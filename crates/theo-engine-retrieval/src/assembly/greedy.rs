//! Single-purpose slice extracted from `assembly.rs` (T4.3 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashSet;
use std::path::Path;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::ScoredCommunity;

use super::*;

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
    }

    // Minimum community size to include in output (filter noise from singletons).
    const MIN_COMMUNITY_SIZE: usize = 2;

    let mut candidates: Vec<Candidate> = scored
        .iter()
        .filter(|s| s.community.node_ids.len() >= MIN_COMMUNITY_SIZE) // Q1.2: skip singletons
        .map(|s| {
            let content = community_content(&s.community, graph);
            let token_count = estimate_tokens(&content).max(1); // floor at 1 to avoid div/0
            Candidate {
                community_id: s.community.id.clone(),
                community_name: s.community.name.clone(),
                content,
                token_count,
                score: s.score,
            }
        })
        .collect();

    // Sort descending by SCORE (relevance first, not token-efficiency).
    // Previous: sorted by density (score/tokens) which caused small irrelevant
    // communities to beat large relevant ones. Now: most relevant first.
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

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
            Candidate {
                community_id: s.community.id.clone(),
                community_name: s.community.name.clone(),
                content,
                token_count,
                score: s.score,
            }
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

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
