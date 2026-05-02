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

pub fn community_document(community: &Community, graph: &CodeGraph) -> String {
    use theo_engine_graph::model::NodeType;

    let mut parts = vec![community.name.clone()];

    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            parts.push(node.name.clone());

            if let Some(sig) = &node.signature {
                parts.push(sig.clone());
            }
            if let Some(doc) = &node.doc
                && let Some(first_line) = doc.lines().next() {
                    parts.push(first_line.to_string());
                }

            // Follow CONTAINS edges to get child symbols
            if matches!(node.node_type, NodeType::File) {
                for child_id in graph.contains_children(node_id) {
                    if let Some(child) = graph.get_node(child_id) {
                        parts.push(child.name.clone());
                        if let Some(sig) = &child.signature {
                            parts.push(sig.clone());
                        }
                        if let Some(doc) = &child.doc
                            && let Some(first_line) = doc.lines().next() {
                                parts.push(first_line.to_string());
                            }
                    }
                }
            }
        }
    }

    parts.join(" ")
}

// ---------------------------------------------------------------------------
// Bm25Index implementation
