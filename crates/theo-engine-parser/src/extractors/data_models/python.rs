//! Single-purpose slice extracted from `data_models.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::path::Path;

use tree_sitter::Node;

use crate::types::*;

use super::*;
use super::helpers::*;
use super::rust::child_by_field;
use super::super::common::{anchor_from_node, node_text};

pub fn try_extract_python_model(
    node: &Node,
    source: &str,
    file_path: &Path,
    models: &mut Vec<DataModel>,
) {
    if node.kind() != "class_definition" {
        return;
    }

    let name = child_by_field(node, "name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }

    let parent_type = find_python_superclass(node, source);

    let mut fields = Vec::new();
    if let Some(body) = child_by_field(node, "body") {
        extract_python_fields(&body, source, &mut fields);
    }

    models.push(DataModel {
        name,
        model_kind: DataModelKind::Class,
        fields,
        anchor: anchor_from_node(node, file_path),
        parent_type,
        implemented_interfaces: Vec::new(),
    });
}

/// Find the first superclass from `class Foo(Bar):` syntax.
pub fn find_python_superclass(node: &Node, source: &str) -> Option<String> {
    if let Some(superclasses) = child_by_field(node, "superclasses") {
        // argument_list containing identifiers
        for i in 0..superclasses.child_count() {
            if let Some(child) = superclasses.child(i as u32)
                && (child.kind() == "identifier" || child.kind() == "attribute") {
                    let text = node_text(&child, source);
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
        }
    }
    None
}

/// Extract fields from Python class body by looking at `__init__` assignments.
///
/// Looks for patterns like:
/// - `self.name = value`
/// - `self.name: type = value`
pub fn extract_python_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    // Walk body looking for `__init__` function_definition
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32) {
            if child.kind() == "function_definition" {
                let fn_name = child_by_field(&child, "name")
                    .map(|n| node_text(&n, source))
                    .unwrap_or_default();
                if fn_name == "__init__"
                    && let Some(fn_body) = child_by_field(&child, "body") {
                        extract_python_init_fields(&fn_body, source, fields);
                    }
            }
            // Also check class-level typed assignments (e.g., dataclass fields)
            if child.kind() == "expression_statement"
                && let Some(inner) = child.child(0_u32) {
                    try_extract_python_class_level_field(&inner, source, fields);
                }
        }
    }
}

/// Extract `self.field = ...` and `self.field: type = ...` from `__init__` body.
pub fn extract_python_init_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32)
            && child.kind() == "expression_statement"
                && let Some(inner) = child.child(0_u32)
                    && inner.kind() == "assignment" {
                        try_extract_self_assignment(&inner, source, fields);
                    }
    }
}

/// Extract field from `self.field = value` or `self.field: type = value`.
pub fn try_extract_self_assignment(node: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let left = match child_by_field(node, "left") {
        Some(n) => n,
        None => return,
    };

    let left_text = node_text(&left, source);
    if !left_text.starts_with("self.") {
        return;
    }
    let field_name = &left_text[5..]; // strip "self."
    if field_name.is_empty() {
        return;
    }

    // Check for type annotation
    let field_type = child_by_field(node, "type").map(|n| node_text(&n, source));

    let visibility = if field_name.starts_with('_') {
        Some(Visibility::Private)
    } else {
        Some(Visibility::Public)
    };

    fields.push(FieldInfo {
        name: field_name.to_string(),
        field_type,
        line: node.start_position().row + 1,
        visibility,
    });
}

/// Extract class-level typed fields (e.g., dataclass style: `name: str`).
pub fn try_extract_python_class_level_field(node: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    if node.kind() == "type" {
        // This is a type alias, not a field
        return;
    }
    // Look for `name: type` patterns (typed assignment or annotation)
    let text = node_text(node, source);
    if text.contains(": ") && !text.starts_with("self.") {
        // Could be a class-level annotated variable like `name: str = "default"`
        if let Some(colon_pos) = text.find(':') {
            let field_name = text[..colon_pos].trim();
            if !field_name.is_empty() && field_name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                let type_part = text[colon_pos + 1..].trim();
                let field_type = type_part
                    .split('=')
                    .next()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());

                fields.push(FieldInfo {
                    name: field_name.to_string(),
                    field_type,
                    line: node.start_position().row + 1,
                    visibility: if field_name.starts_with('_') {
                        Some(Visibility::Private)
                    } else {
                        Some(Visibility::Public)
                    },
                });
            }
        }
    }
}
