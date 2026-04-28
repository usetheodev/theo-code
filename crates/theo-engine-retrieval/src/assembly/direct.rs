//! Single-purpose slice extracted from `assembly.rs` (T4.3 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashSet;
use std::path::Path;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::ScoredCommunity;

use super::*;

pub fn assemble_files_direct(
    file_scores: &std::collections::HashMap<String, f64>,
    graph: &CodeGraph,
    communities: &[Community],
    budget_tokens: usize,
) -> ContextPayload {
    assemble_files_direct_with_inline_skip(file_scores, graph, communities, budget_tokens, &[])
}

/// PLAN_CONTEXT_WIRING Task 3.3 — same as `assemble_files_direct` but
/// suppresses the reverse-dependency boost for files that already
/// appear as the focal of an inline slice. Avoids double counting
/// (the inline slice already brings caller context inline).
///
/// `inline_focal_files` is the list of `InlineSlice.focal_file` paths
/// from the current `FileRetrievalResult.inline_slices`.
pub fn assemble_files_direct_with_inline_skip(
    file_scores: &std::collections::HashMap<String, f64>,
    graph: &CodeGraph,
    communities: &[Community],
    budget_tokens: usize,
    inline_focal_files: &[String],
) -> ContextPayload {
    if file_scores.is_empty() || budget_tokens == 0 {
        return ContextPayload {
            items: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: String::new(),
        };
    }
    let inline_skip: HashSet<&str> = inline_focal_files.iter().map(|s| s.as_str()).collect();

    // Build file → community lookup
    let mut file_to_community: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::new();
    for comm in communities {
        for nid in &comm.node_ids {
            if let Some(node) = graph.get_node(nid)
                && let Some(fp) = node.file_path.as_deref() {
                    file_to_community.entry(fp).or_insert(&comm.name);
                }
        }
    }

    // Apply penalties and boosts before ranking.
    let mut adjusted_scores: std::collections::HashMap<&str, f64> = file_scores
        .iter()
        .map(|(path, &score)| {
            let p = path.as_str();
            let mut s = score;

            // Penalty: test/benchmark/example files get 1/10 score (Zoekt pattern).
            // These files mention symbols (because they test them) but aren't source.
            let lp = p.to_lowercase();
            let is_test = lp.contains("/tests/")
                || lp.contains("/test_")
                || lp.contains("_test.")
                || lp.contains(".test.")
                || lp.contains("_spec.")
                || lp.contains(".spec.")
                || lp.starts_with("tests/");
            let is_benchmark = lp.contains("/examples/")
                || lp.contains("/benchmark")
                || lp.contains("benchmark.")
                || lp.contains("theo-benchmark")
                || lp.contains("/benches/");
            let is_eval = lp.contains("eval_suite") || lp.contains("eval_");

            if is_test || is_benchmark || is_eval {
                s *= 0.1; // 1/10 penalty
            }

            (p, s)
        })
        .collect();

    // Reverse Dependency Boost (LocAgent ACL 2025 pattern):
    // After BM25 finds file #1 (the definer), find files that CALL symbols
    // defined in file #1. This answers "who USES this?" — the exact gap
    // in slots 2-5.
    //
    // Key difference from failed forward expansion: we follow REVERSE edges
    // from the seed's symbols to their callers. Forward expansion (file→imports)
    // hits lib.rs (imported by everything). Reverse expansion (symbol→callers)
    // is targeted and sparse.
    //
    // Filters: only Function/Method symbols (types/traits have too many implementers),
    // skip hub files (lib.rs, mod.rs, main.rs), cap boost at 0.6.
    use theo_engine_graph::model::SymbolKind;

    const REVERSE_BOOST_PER_CALLER: f64 = 0.20;
    const MAX_REVERSE_BOOST: f64 = 0.6;
    const HUB_SUFFIXES: &[&str] = &["/lib.rs", "/mod.rs", "/main.rs"];

    let is_hub_file = |p: &str| HUB_SUFFIXES.iter().any(|s| p.ends_with(s));

    // Only expand from top-3 BM25 seeds
    let mut seeds_for_reverse: Vec<(&str, f64)> =
        adjusted_scores.iter().map(|(&p, &s)| (p, s)).collect();
    seeds_for_reverse.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut reverse_boost: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();

    for (seed_path, _seed_score) in seeds_for_reverse.iter().take(3) {
        // Phase 3 mutual exclusion: when this seed already appears as
        // an inline slice's focal file, the slice will inject caller
        // context as a high-priority block elsewhere — skipping the
        // reverse boost here prevents double counting.
        if inline_skip.contains(seed_path) {
            continue;
        }
        let file_id = format!("file:{}", seed_path);

        // For each symbol DEFINED in this seed file
        for sym_id in graph.contains_children(&file_id) {
            let Some(sym) = graph.get_node(sym_id) else {
                continue;
            };

            // Only functions/methods — types and traits have too many implementers
            let is_fn = matches!(
                sym.kind,
                Some(SymbolKind::Function) | Some(SymbolKind::Method)
            );
            if !is_fn {
                continue;
            }

            // Find CALLERS of this symbol (reverse edge traversal)
            for caller_id in graph.reverse_neighbors(sym_id) {
                let Some(caller) = graph.get_node(caller_id) else {
                    continue;
                };
                let Some(caller_file) = caller.file_path.as_deref() else {
                    continue;
                };

                // Skip self-references and hub files
                if caller_file == *seed_path {
                    continue;
                }
                if is_hub_file(caller_file) {
                    continue;
                }

                *reverse_boost.entry(caller_file).or_insert(0.0) += REVERSE_BOOST_PER_CALLER;
            }
        }

        // Also check FILE-level reverse edges (added by SCIP merge).
        // SCIP adds precise file:A → file:B edges for cross-file references.
        for rev_id in graph.reverse_neighbors(&file_id) {
            if let Some(rev_node) = graph.get_node(rev_id)
                && rev_node.node_type == NodeType::File
                    && let Some(fp) = rev_node.file_path.as_deref()
                        && fp != *seed_path && !is_hub_file(fp) {
                            *reverse_boost.entry(fp).or_insert(0.0) += REVERSE_BOOST_PER_CALLER;
                        }
        }
    }

    // Apply capped boost
    for (&path, &boost) in &reverse_boost {
        let capped = boost.min(MAX_REVERSE_BOOST);
        adjusted_scores
            .entry(path)
            .and_modify(|s| *s += capped)
            .or_insert(capped);
    }

    // Rank files by adjusted score descending
    let mut ranked_files: Vec<(&str, f64)> = adjusted_scores.into_iter().collect();
    ranked_files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Build items: one per file, with signatures
    let mut items: Vec<ContextItem> = Vec::new();
    let mut total_tokens = 0;
    let mut seen_files: HashSet<&str> = HashSet::new();

    for (file_path, score) in &ranked_files {
        if !seen_files.insert(file_path) {
            continue;
        }

        // Find file node ID
        let file_id = format!("file:{}", file_path);
        let mut lines: Vec<String> = Vec::new();

        // Community annotation (1 line)
        if let Some(comm_name) = file_to_community.get(file_path) {
            lines.push(format!("# {} [{}]", file_path, comm_name));
        } else {
            lines.push(format!("# {}", file_path));
        }

        // File header with ## for eval detection
        lines.push(format!("## {}", file_path));

        // Symbol signatures from children
        let children = graph.contains_children(&file_id);
        let mut seen_sigs: HashSet<String> = HashSet::new();
        for child_id in children {
            if let Some(child) = graph.get_node(child_id) {
                let text = child.signature.as_deref().unwrap_or(&child.name);
                if seen_sigs.insert(text.to_string()) {
                    lines.push(text.to_string());
                }
            }
        }

        let content = lines.join("\n");
        let token_count = estimate_tokens(&content).max(1);

        if total_tokens + token_count > budget_tokens {
            break; // Budget exhausted
        }

        total_tokens += token_count;
        items.push(ContextItem {
            community_id: format!("file:{}", file_path),
            content,
            token_count,
            score: *score,
        });

        if items.len() >= 20 {
            break; // Cap at 20 files
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
// Tests
// ---------------------------------------------------------------------------
