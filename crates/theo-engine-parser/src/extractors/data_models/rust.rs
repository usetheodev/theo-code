//! Single-purpose slice extracted from `data_models.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::path::Path;

use tree_sitter::Node;

use crate::types::*;

use super::*;
use super::super::common::{anchor_from_node, node_text};

pub fn try_extract_rust_model(
    node: &Node,
    source: &str,
    file_path: &Path,
    models: &mut Vec<DataModel>,
) {
    if node.kind() != "struct_item" {
        return;
    }

    let name = child_by_field(node, "name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }

    let mut fields = Vec::new();
    if let Some(body) = child_by_field(node, "body")
        && body.kind() == "field_declaration_list" {
            extract_rust_fields(&body, source, &mut fields);
        }

    models.push(DataModel {
        name,
        model_kind: DataModelKind::Struct,
        fields,
        anchor: anchor_from_node(node, file_path),
        parent_type: None,
        implemented_interfaces: Vec::new(),
    });
}

/// Extract fields from a Rust field declaration list.
pub fn extract_rust_fields(list: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = list.child_count();
    for i in 0..count {
        if let Some(child) = list.child(i as u32)
            && child.kind() == "field_declaration" {
                let name = child_by_field(&child, "name")
                    .map(|n| node_text(&n, source))
                    .unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let field_type = child_by_field(&child, "type").map(|n| node_text(&n, source));

                // Check for `pub` visibility modifier
                let visibility = if has_visibility_modifier(&child) {
                    Some(Visibility::Public)
                } else {
                    Some(Visibility::Private)
                };

                fields.push(FieldInfo {
                    name,
                    field_type,
                    line: child.start_position().row + 1,
                    visibility,
                });
            }
    }
}

/// Check if a Rust node has a `visibility_modifier` child (i.e., `pub`).
pub fn has_visibility_modifier(node: &Node) -> bool {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "visibility_modifier" {
                return true;
            }
    }
    false
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Find a named child (field) by its tree-sitter field name.
pub fn child_by_field<'a>(node: &'a Node<'a>, field_name: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field_name)
}
