//! Single-purpose slice extracted from `assembly.rs` (T4.3 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashSet;
use std::path::Path;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::ScoredCommunity;

use super::*;

pub fn community_content(community: &Community, graph: &CodeGraph) -> String {
    let mut lines: Vec<String> = vec![format!("# {}", community.name)];
    let mut seen_signatures: std::collections::HashSet<String> = std::collections::HashSet::new(); // Q1.3: dedup

    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            match node.node_type {
                NodeType::File => {
                    // Emit child signatures (symbols contained in this file).
                    let children = graph.contains_children(node_id);
                    // Always emit ## prefix so file paths are detectable by consumers.
                    lines.push(format!("## {}", node.name));
                    if !children.is_empty() {
                        for child_id in children {
                            if let Some(child) = graph.get_node(child_id) {
                                let text = child.signature.as_deref().unwrap_or(&child.name);
                                if seen_signatures.insert(text.to_string()) {
                                    lines.push(text.to_string());
                                }
                            }
                        }
                    }
                }
                _ => {
                    let text = node.signature.as_deref().unwrap_or(&node.name);
                    if seen_signatures.insert(text.to_string()) {
                        lines.push(text.to_string());
                    }
                }
            }
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Assembly
