//! Single-purpose slice extracted from `wiki/generator.rs` (T4.1 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType, SymbolKind};

use crate::wiki::model::*;

use super::*;

pub fn compute_graph_hash(graph: &CodeGraph) -> u64 {
    use std::collections::BTreeMap;
    use std::hash::{Hash, Hasher};

    let mut file_info: BTreeMap<String, u64> = BTreeMap::new();
    for node in graph.file_nodes() {
        let path = node.file_path.as_deref().unwrap_or(&node.name);
        file_info.insert(path.to_string(), node.last_modified.to_bits());
    }

    let mut hasher = std::hash::DefaultHasher::new();
    for (path, mtime) in &file_info {
        path.hash(&mut hasher);
        mtime.hash(&mut hasher);
    }
    hasher.finish()
}

/// Compute hash for a single community's files (for incremental generation).
///
/// Uses canonical path prefix (common path of member files) as stable key,
/// independent of Leiden's non-deterministic node_ids ordering.
pub fn compute_community_hash(community: &Community, graph: &CodeGraph) -> u64 {
    use std::collections::BTreeMap;
    use std::hash::{Hash, Hasher};

    let mut file_info: BTreeMap<String, u64> = BTreeMap::new();
    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id)
            && node.node_type == NodeType::File {
                let path = node.file_path.as_deref().unwrap_or(&node.name);
                file_info.insert(path.to_string(), node.last_modified.to_bits());
            }
    }

    let mut hasher = std::hash::DefaultHasher::new();
    for (path, mtime) in &file_info {
        path.hash(&mut hasher);
        mtime.hash(&mut hasher);
    }
    hasher.finish()
}

/// Canonical key for a community — the community slug.
///
/// Used as the key in page_hashes for incremental generation.
/// Uses slugify(community.name) which is deterministic for the same community.
pub fn community_canonical_key(community: &Community, _graph: &CodeGraph) -> String {
    slugify(&community.name)
}
