//! Single-purpose slice extracted from `data_models.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::path::Path;

use tree_sitter::Node;

use crate::types::*;

use super::*;
use super::helpers::*;
use super::rust::child_by_field;
use super::super::common::{anchor_from_node, node_text};

pub fn try_extract_csharp_model(
    node: &Node,
    source: &str,
    file_path: &Path,
    models: &mut Vec<DataModel>,
) {
    let model_kind = match node.kind() {
        "class_declaration" => DataModelKind::Class,
        "struct_declaration" => DataModelKind::Struct,
        "interface_declaration" => DataModelKind::Interface,
        _ => return,
    };

    let name = child_by_field(node, "name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }

    let parent_type = find_csharp_base_type(node, source);

    let mut fields = Vec::new();
    if let Some(body) = child_by_field(node, "body") {
        extract_csharp_fields(&body, source, &mut fields);
    }

    models.push(DataModel {
        name,
        model_kind,
        fields,
        anchor: anchor_from_node(node, file_path),
        parent_type,
        implemented_interfaces: Vec::new(),
    });
}

/// Find C# base type from `: BaseClass` syntax.
pub fn find_csharp_base_type(node: &Node, source: &str) -> Option<String> {
    if let Some(bases) = child_by_field(node, "bases") {
        // base_list contains base_type nodes
        for i in 0..bases.child_count() {
            if let Some(child) = bases.child(i as u32) {
                let kind = child.kind();
                if kind == "identifier" || kind == "generic_name" || kind == "qualified_name" {
                    let text = node_text(&child, source);
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
                // Recurse one level for nested node types
                for j in 0..child.child_count() {
                    if let Some(inner) = child.child(j as u32) {
                        let inner_kind = inner.kind();
                        if inner_kind == "identifier"
                            || inner_kind == "generic_name"
                            || inner_kind == "qualified_name"
                        {
                            let text = node_text(&inner, source);
                            if !text.is_empty() {
                                return Some(text);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Extract fields from a C# class/struct body.
pub fn extract_csharp_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32)
            && child.kind() == "field_declaration" {
                let field_type = child_by_field(&child, "type").map(|n| node_text(&n, source));

                let visibility = detect_csharp_visibility(&child, source);

                // variable_declaration → variable_declarator → identifier
                if let Some(var_decl) = child_by_field(&child, "declaration") {
                    for j in 0..var_decl.child_count() {
                        if let Some(declarator) = var_decl.child(j as u32)
                            && declarator.kind() == "variable_declarator" {
                                let name = child_by_field(&declarator, "name")
                                    .or_else(|| declarator.child(0_u32))
                                    .map(|n| node_text(&n, source))
                                    .unwrap_or_default();
                                if !name.is_empty() {
                                    fields.push(FieldInfo {
                                        name,
                                        field_type: field_type.clone(),
                                        line: child.start_position().row + 1,
                                        visibility,
                                    });
                                }
                            }
                    }
                }
            }
    }
}

/// Detect C# visibility from modifier nodes.
pub fn detect_csharp_visibility(node: &Node, source: &str) -> Option<Visibility> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "modifier" {
                let text = node_text(&child, source);
                match text.as_str() {
                    "public" => return Some(Visibility::Public),
                    "private" => return Some(Visibility::Private),
                    "protected" => return Some(Visibility::Protected),
                    "internal" => return Some(Visibility::Internal),
                    _ => {}
                }
            }
    }
    None
}
