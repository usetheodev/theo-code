//! Greedy knapsack context assembly.
//!
//! Converts scored communities into a `ContextPayload` that fits within a
//! token budget. Items are ranked by value density (score / token_count) and
//! filled greedily until the budget is exhausted.

#![allow(unused_imports, dead_code)]

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

/// Token estimation using unified domain function.
pub fn estimate_tokens(text: &str) -> usize {
    theo_domain::tokens::estimate_tokens(text)
}

// ---------------------------------------------------------------------------
// Content generation
