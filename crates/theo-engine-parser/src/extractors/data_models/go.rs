//! Single-purpose slice extracted from `data_models.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::path::Path;

use tree_sitter::Node;

use crate::types::*;

use super::*;
use super::helpers::*;
use super::rust::child_by_field;
use super::super::common::{anchor_from_node, node_text};

pub fn try_extract_go_model(node: &Node, source: &str, file_path: &Path, models: &mut Vec<DataModel>) {
    if node.kind() != "type_declaration" {
        return;
    }

    for i in 0..node.child_count() {
        if let Some(type_spec) = node.child(i as u32) {
            if type_spec.kind() != "type_spec" {
                continue;
            }

            let name = child_by_field(&type_spec, "name")
                .map(|n| node_text(&n, source))
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }

            let type_node = match child_by_field(&type_spec, "type") {
                Some(n) => n,
                None => continue,
            };

            if type_node.kind() == "struct_type" {
                let mut fields = Vec::new();
                extract_go_struct_fields(&type_node, source, &mut fields);

                models.push(DataModel {
                    name,
                    model_kind: DataModelKind::Struct,
                    fields,
                    anchor: anchor_from_node(node, file_path),
                    parent_type: None,
                    implemented_interfaces: Vec::new(),
                });
            } else if type_node.kind() == "interface_type" {
                models.push(DataModel {
                    name,
                    model_kind: DataModelKind::Interface,
                    fields: Vec::new(),
                    anchor: anchor_from_node(node, file_path),
                    parent_type: None,
                    implemented_interfaces: Vec::new(),
                });
            }
        }
    }
}

/// Extract fields from a Go struct type.
pub fn extract_go_struct_fields(struct_node: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    // struct_type → field_declaration_list → field_declaration
    if let Some(field_list) = find_child_by_kind(struct_node, "field_declaration_list") {
        let count = field_list.child_count();
        for i in 0..count {
            if let Some(field) = field_list.child(i as u32)
                && field.kind() == "field_declaration" {
                    let field_name = child_by_field(&field, "name")
                        .map(|n| node_text(&n, source))
                        .unwrap_or_default();
                    if field_name.is_empty() {
                        continue;
                    }
                    let field_type = child_by_field(&field, "type").map(|n| node_text(&n, source));

                    // Go: capitalized → public, lowercase → private
                    let visibility = field_name.chars().next().map(|c| {
                        if c.is_uppercase() {
                            Visibility::Public
                        } else {
                            Visibility::Private
                        }
                    });

                    fields.push(FieldInfo {
                        name: field_name,
                        field_type,
                        line: field.start_position().row + 1,
                        visibility,
                    });
                }
        }
    }
}
