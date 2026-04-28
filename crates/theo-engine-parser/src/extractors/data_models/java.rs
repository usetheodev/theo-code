//! Single-purpose slice extracted from `data_models.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::path::Path;

use tree_sitter::Node;

use crate::types::*;

use super::*;
use super::helpers::*;
use super::rust::child_by_field;
use super::super::common::{anchor_from_node, node_text};

pub fn try_extract_java_model(
    node: &Node,
    source: &str,
    file_path: &Path,
    models: &mut Vec<DataModel>,
) {
    let kind = node.kind();
    let model_kind = match kind {
        "class_declaration" => DataModelKind::Class,
        "interface_declaration" => DataModelKind::Interface,
        _ => return,
    };

    let name = child_by_field(node, "name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }

    let parent_type = find_java_superclass(node, source);
    let implemented = find_java_interfaces(node, source);

    let mut fields = Vec::new();
    if let Some(body) = child_by_field(node, "body") {
        extract_java_fields(&body, source, &mut fields);
    }

    models.push(DataModel {
        name,
        model_kind,
        fields,
        anchor: anchor_from_node(node, file_path),
        parent_type,
        implemented_interfaces: implemented,
    });
}

/// Find Java superclass from `extends Foo` clause.
pub fn find_java_superclass(node: &Node, source: &str) -> Option<String> {
    if let Some(super_node) = child_by_field(node, "superclass") {
        let text = node_text(&super_node, source);
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// Find Java implemented interfaces from `implements` clause.
pub fn find_java_interfaces(node: &Node, source: &str) -> Vec<String> {
    let mut result = Vec::new();
    if let Some(interfaces_node) = child_by_field(node, "interfaces") {
        for i in 0..interfaces_node.child_count() {
            if let Some(child) = interfaces_node.child(i as u32) {
                let kind = child.kind();
                if kind == "type_identifier" || kind == "generic_type" || kind == "type_list" {
                    if kind == "type_list" {
                        // type_list contains multiple type identifiers
                        for j in 0..child.child_count() {
                            if let Some(typ) = child.child(j as u32)
                                && (typ.kind() == "type_identifier" || typ.kind() == "generic_type") {
                                    let text = node_text(&typ, source);
                                    if !text.is_empty() {
                                        result.push(text);
                                    }
                                }
                        }
                    } else {
                        let text = node_text(&child, source);
                        if !text.is_empty() {
                            result.push(text);
                        }
                    }
                }
            }
        }
    }
    result
}

/// Extract fields from a Java class body.
pub fn extract_java_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32)
            && child.kind() == "field_declaration" {
                let field_type = child_by_field(&child, "type").map(|n| node_text(&n, source));

                let visibility = detect_java_visibility(&child, source);

                // The variable name is inside a `variable_declarator`
                if let Some(declarator) = child_by_field(&child, "declarator") {
                    let name = child_by_field(&declarator, "name")
                        .map(|n| node_text(&n, source))
                        .unwrap_or_default();
                    if !name.is_empty() {
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
}

/// Detect Java visibility from modifier keywords.
pub fn detect_java_visibility(node: &Node, source: &str) -> Option<Visibility> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "modifiers" {
                let text = node_text(&child, source);
                if text.contains("public") {
                    return Some(Visibility::Public);
                } else if text.contains("private") {
                    return Some(Visibility::Private);
                } else if text.contains("protected") {
                    return Some(Visibility::Protected);
                }
            }
    }
    None
}
