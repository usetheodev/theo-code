//! Single-purpose slice extracted from `search.rs` (T4.3 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashMap;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::graph_attention::propagate_attention;
use crate::neural::NeuralEmbedder;
use crate::tfidf::{TfidfConfig, TfidfModel};
use crate::turboquant::{QuantizedVector, TurboQuantizer};

use super::*;

pub fn stem(word: &str) -> String {
    let w = word.to_lowercase();
    if w.len() < 4 {
        return w;
    }
    // ies → y (communities → community)
    if w.ends_with("ies") {
        return format!("{}y", &w[..w.len() - 3]);
    }
    // ing → (running → run, but keep "ring")
    if w.ends_with("ing") && w.len() > 5 {
        return w[..w.len() - 3].to_string();
    }
    // tion → t (detection → detect)
    if w.ends_with("tion") {
        return w[..w.len() - 3].to_string();
    }
    // ment → (refinement → refine)
    if w.ends_with("ment") && w.len() > 6 {
        return w[..w.len() - 4].to_string();
    }
    // es → (phases → phase)
    if w.ends_with("es") && w.len() > 4 {
        return w[..w.len() - 2].to_string();
    }
    // s → (clusters → cluster)
    if w.ends_with('s') && !w.ends_with("ss") {
        return w[..w.len() - 1].to_string();
    }
    w
}

/// Tokenise with identifier splitting and basic stemming.
///
/// Handles camelCase, PascalCase, snake_case, SCREAMING_CASE, and mixed:
///   "verifyJwtToken"     → ["verify", "jwt", "token"]
///   "parse_auth_header"  → ["parse", "auth", "header"]
///   "HTMLParser"         → ["html", "parser"]
///   "getHTTPResponse"    → ["get", "http", "response"]
///   "communities"        → ["community"] (stemmed)
pub(crate) fn tokenise(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    // First split on non-alphanumeric (handles snake_case, spaces, punctuation)
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        // Also index the unsplit form (lowercased) for substring-like matching.
        // "OpenAIAuth" → tokens: ["open", "ai", "auth", "openaiauth"]
        // This helps "oauth" match files containing "oauth_client" etc.
        let lower = word.to_lowercase();
        if lower.len() >= 3 {
            tokens.push(lower);
        }
        // Then split camelCase/PascalCase
        split_identifier(word, &mut tokens);
    }
    tokens
}

/// Split a single identifier on camelCase/PascalCase boundaries.
///
/// "verifyJwtToken" → ["verify", "jwt", "token"]
/// "HTMLParser"     → ["html", "parser"]
/// "getHTTPResponse" → ["get", "http", "response"]
pub fn split_identifier(word: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = word.chars().collect();
    if chars.is_empty() {
        return;
    }

    let mut start = 0;
    let len = chars.len();

    for i in 1..len {
        let prev = chars[i - 1];
        let curr = chars[i];
        let split = if prev.is_lowercase() && curr.is_uppercase() {
            true
        } else { prev.is_uppercase()
            && curr.is_uppercase()
            && i + 1 < len && chars[i + 1].is_lowercase() };

        if split {
            let part: String = chars[start..i].iter().collect();
            if !part.is_empty() {
                out.push(stem(&part));
            }
            start = i;
        }
    }

    let part: String = chars[start..].iter().collect();
    if !part.is_empty() {
        out.push(stem(&part));
    }
}

// ---------------------------------------------------------------------------
// BM25 helpers
