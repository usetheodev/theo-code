//! Single-purpose slice extracted from `data_models.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::path::Path;

use tree_sitter::Node;

use crate::types::*;

use super::*;
use super::super::common::{anchor_from_node, node_text};
use super::rust::child_by_field;

pub fn find_child_by_kind<'a>(node: &'a Node<'a>, kind: &str) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == kind {
                return Some(child);
            }
    }
    None
}

/// Extract the text of a type annotation node, stripping a leading `:` if present.
///
/// In TypeScript, type annotations are often `type_annotation` nodes that
/// include the colon, e.g., `: string`. We strip the colon for clean output.
pub fn extract_type_annotation_text(node: &Node, source: &str) -> String {
    let text = node_text(node, source);
    let trimmed = text.trim();
    if let Some(stripped) = trimmed.strip_prefix(':') {
        stripped.trim().to_string()
    } else {
        trimmed.to_string()
    }
}
