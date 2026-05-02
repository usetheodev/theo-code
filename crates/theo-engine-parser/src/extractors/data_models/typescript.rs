//! Single-purpose slice extracted from `data_models.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::path::Path;

use tree_sitter::Node;

use crate::types::*;

use super::*;
use super::helpers::*;
use super::rust::child_by_field;
use super::super::common::{anchor_from_node, node_text};

pub fn try_extract_ts_model(node: &Node, source: &str, file_path: &Path, models: &mut Vec<DataModel>) {
    match node.kind() {
        "class_declaration" => {
            let name = child_by_field(node, "name")
                .map(|n| node_text(&n, source))
                .unwrap_or_default();
            if name.is_empty() {
                return;
            }

            let parent_type = find_ts_extends(node, source);
            let implemented = find_ts_implements(node, source);

            let mut fields = Vec::new();
            if let Some(body) = child_by_field(node, "body") {
                extract_ts_class_fields(&body, source, &mut fields);
            }

            models.push(DataModel {
                name,
                model_kind: DataModelKind::Class,
                fields,
                anchor: anchor_from_node(node, file_path),
                parent_type,
                implemented_interfaces: implemented,
            });
        }
        "interface_declaration" => {
            let name = child_by_field(node, "name")
                .map(|n| node_text(&n, source))
                .unwrap_or_default();
            if name.is_empty() {
                return;
            }

            let parent_type = find_ts_interface_extends(node, source);

            let mut fields = Vec::new();
            if let Some(body) = child_by_field(node, "body") {
                extract_ts_interface_fields(&body, source, &mut fields);
            }

            models.push(DataModel {
                name,
                model_kind: DataModelKind::Interface,
                fields,
                anchor: anchor_from_node(node, file_path),
                parent_type,
                implemented_interfaces: Vec::new(),
            });
        }
        _ => {}
    }
}

/// Extract fields from a TS/JS class body.
pub fn extract_ts_class_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32) {
            let kind = child.kind();
            // TS uses `public_field_definition` for uninitialized class fields
            // and `property_declaration` for fields, depending on grammar version.
            if kind == "public_field_definition" || kind == "property_declaration" {
                let name = child_by_field(&child, "name")
                    .map(|n| node_text(&n, source))
                    .unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let field_type = child_by_field(&child, "type")
                    .map(|n| extract_type_annotation_text(&n, source));

                let visibility = detect_ts_field_visibility(&child, source);

                fields.push(FieldInfo {
                    name,
                    field_type,
                    line: child.start_position().row + 1,
                    visibility,
                });
            }
        }
    }
}

/// Detect visibility of a TS class field from accessibility modifier.
pub fn detect_ts_field_visibility(node: &Node, source: &str) -> Option<Visibility> {
    let text = node_text(node, source);
    if text.starts_with("private ") || text.starts_with("private\t") {
        Some(Visibility::Private)
    } else if text.starts_with("protected ") || text.starts_with("protected\t") {
        Some(Visibility::Protected)
    } else if text.starts_with("public ") || text.starts_with("public\t") {
        Some(Visibility::Public)
    } else {
        None
    }
}

/// Extract fields from a TS interface body.
pub fn extract_ts_interface_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32)
            && child.kind() == "property_signature" {
                let name = child_by_field(&child, "name")
                    .map(|n| node_text(&n, source))
                    .unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let field_type = child_by_field(&child, "type")
                    .map(|n| extract_type_annotation_text(&n, source));

                fields.push(FieldInfo {
                    name,
                    field_type,
                    line: child.start_position().row + 1,
                    visibility: None,
                });
            }
    }
}

/// Find the `extends` clause in a TS class declaration.
pub fn find_ts_extends(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "class_heritage" {
                // Walk children looking for `extends_clause`
                for j in 0..child.child_count() {
                    if let Some(clause) = child.child(j as u32)
                        && clause.kind() == "extends_clause" {
                            // The type follows the `extends` keyword
                            return extract_first_type_from_clause(&clause, source);
                        }
                }
            }
            // Some grammars put extends_clause directly as a child
            if child.kind() == "extends_clause" {
                return extract_first_type_from_clause(&child, source);
            }
        }
    }
    None
}

/// Find `implements` clause types in a TS class declaration.
pub fn find_ts_implements(node: &Node, source: &str) -> Vec<String> {
    let mut result = Vec::new();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "class_heritage" {
                for j in 0..child.child_count() {
                    if let Some(clause) = child.child(j as u32)
                        && clause.kind() == "implements_clause" {
                            collect_types_from_clause(&clause, source, &mut result);
                        }
                }
            }
            if child.kind() == "implements_clause" {
                collect_types_from_clause(&child, source, &mut result);
            }
        }
    }
    result
}

/// Find `extends` parent for TS interfaces.
pub fn find_ts_interface_extends(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && (child.kind() == "extends_type_clause" || child.kind() == "extends_clause") {
                return extract_first_type_from_clause(&child, source);
            }
    }
    None
}

/// Extract the first type identifier from an extends/implements clause.
pub fn extract_first_type_from_clause(clause: &Node, source: &str) -> Option<String> {
    for i in 0..clause.child_count() {
        if let Some(child) = clause.child(i as u32) {
            let kind = child.kind();
            if kind == "type_identifier" || kind == "identifier" || kind == "generic_type" {
                let text = node_text(&child, source);
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }
    None
}

/// Collect all type identifiers from an implements clause.
pub fn collect_types_from_clause(clause: &Node, source: &str, result: &mut Vec<String>) {
    for i in 0..clause.child_count() {
        if let Some(child) = clause.child(i as u32) {
            let kind = child.kind();
            if kind == "type_identifier" || kind == "identifier" || kind == "generic_type" {
                let text = node_text(&child, source);
                if !text.is_empty() {
                    result.push(text);
                }
            }
        }
    }
}
