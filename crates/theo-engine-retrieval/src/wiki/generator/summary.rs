//! Single-purpose slice extracted from `wiki/generator.rs` (T4.1 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType, SymbolKind};

use crate::wiki::model::*;

use super::*;

pub(super) fn generate_summary(doc: &crate::wiki::model::WikiDoc) -> String {
    let kind_summary = {
        let has_traits = doc.public_api.iter().any(|a| a.kind == "Trait");
        let has_structs = doc.public_api.iter().any(|a| a.kind == "Struct");
        if has_traits && has_structs {
            "traits and types"
        } else if has_traits {
            "traits"
        } else if has_structs {
            "types"
        } else {
            "functions"
        }
    };

    let dep_hint = if !doc.dependencies.is_empty() {
        format!(", depends on {} modules", doc.dependencies.len())
    } else {
        String::new()
    };

    let primary = doc
        .entry_points
        .first()
        .map(|e| format!(" Primary: {}.", e.name))
        .unwrap_or_default();

    format!(
        "{} {} across {} files ({} symbols{}).{}",
        doc.primary_language, kind_summary, doc.file_count, doc.symbol_count, dep_hint, primary
    )
}

/// Auto-detect tags from file paths and symbol kinds.
pub(super) fn generate_tags(doc: &crate::wiki::model::WikiDoc) -> Vec<String> {
    let mut tags = vec![doc.primary_language.clone()];

    // From path patterns
    let all_paths: String = doc
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let path_patterns: &[(&str, &str)] = &[
        ("test", "testing"),
        ("auth", "auth"),
        ("route", "routing"),
        ("router", "routing"),
        ("middleware", "middleware"),
        ("extract", "extraction"),
        ("error", "error-handling"),
        ("handler", "handlers"),
        ("response", "response"),
        ("request", "request"),
        ("body", "http-body"),
        ("json", "json"),
        ("form", "forms"),
        ("query", "query"),
        ("state", "state"),
        ("tower", "tower"),
        ("service", "service"),
        ("layer", "layer"),
        ("header", "headers"),
        ("cookie", "cookies"),
        ("websocket", "websocket"),
        ("sse", "sse"),
        ("multipart", "multipart"),
    ];
    for (pattern, tag) in path_patterns {
        if all_paths.contains(pattern) {
            tags.push(tag.to_string());
        }
    }

    // From symbol kinds
    if doc.public_api.iter().any(|a| a.kind == "Trait") {
        tags.push("traits".into());
    }
    if doc.public_api.iter().any(|a| a.kind == "Struct") {
        tags.push("types".into());
    }
    if doc.public_api.iter().any(|a| a.kind == "Enum") {
        tags.push("enums".into());
    }
    if doc.test_coverage.percentage > 80.0 {
        tags.push("well-tested".into());
    }

    tags.sort();
    tags.dedup();
    tags
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build file_path → community_slug map.
pub(super) fn build_file_community_map(
    communities: &[Community],
    graph: &CodeGraph,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for community in communities {
        let slug = slugify(&community.name);
        for node_id in &community.node_ids {
            if let Some(node) = graph.get_node(node_id)
                && let Some(fp) = &node.file_path {
                    map.insert(fp.clone(), slug.clone());
                }
        }
    }
    map
}

/// Slugify a community name for use as filename.
pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "-")
        .replace("--", "-")
        .trim_matches('-')
        .to_string()
}
