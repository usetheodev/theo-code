//! Data model extraction from tree-sitter CSTs.
//!
//! Extracts classes, structs, and interfaces along with their field
//! definitions from source code across multiple languages.  Each
//! language uses different CST node names for the same concepts,
//! so this module dispatches to language-specific walkers.
//!
//! Extracted information:
//! - Model name, kind (class/struct/interface), location
//! - Fields with name, type annotation, line, visibility
//! - Parent type (extends/inherits) when detectable
//! - Implemented interfaces when detectable

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::tree_sitter::SupportedLanguage;
use crate::types::{DataModel, DataModelKind, FieldInfo, Visibility};

use super::common::{anchor_from_node, node_text};

/// Extract data models (classes, structs, interfaces with fields) from a CST.
///
/// Dispatches to language-specific extraction logic based on the
/// `SupportedLanguage`. Languages without dedicated support return
/// an empty vector.
pub fn extract_data_models(
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
    file_path: &Path,
) -> Vec<DataModel> {
    let root = tree.root_node();
    let mut models = Vec::new();

    collect_models(&root, source, language, file_path, &mut models);

    models
}

/// Recursively walk the CST collecting data models.
fn collect_models(
    node: &Node,
    source: &str,
    language: SupportedLanguage,
    file_path: &Path,
    models: &mut Vec<DataModel>,
) {
    match language {
        SupportedLanguage::TypeScript
        | SupportedLanguage::Tsx
        | SupportedLanguage::JavaScript
        | SupportedLanguage::Jsx => {
            try_extract_ts_model(node, source, file_path, models);
        }
        SupportedLanguage::Python => {
            try_extract_python_model(node, source, file_path, models);
        }
        SupportedLanguage::Java | SupportedLanguage::Kotlin => {
            try_extract_java_model(node, source, file_path, models);
        }
        SupportedLanguage::CSharp => {
            try_extract_csharp_model(node, source, file_path, models);
        }
        SupportedLanguage::Go => {
            try_extract_go_model(node, source, file_path, models);
        }
        SupportedLanguage::Rust => {
            try_extract_rust_model(node, source, file_path, models);
        }
        _ => {}
    }

    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i as u32) {
            collect_models(&child, source, language, file_path, models);
        }
    }
}

// ---------------------------------------------------------------------------
// TypeScript / JavaScript
// ---------------------------------------------------------------------------

/// Extract TS/JS class or interface declarations.
///
/// CST shapes:
/// - `class_declaration` with optional `extends_clause`
///   - body: `class_body` containing `public_field_definition` / `property_declaration`
/// - `interface_declaration` with optional `extends_type_clause`
///   - body: `interface_body` / `object_type` containing `property_signature`
fn try_extract_ts_model(node: &Node, source: &str, file_path: &Path, models: &mut Vec<DataModel>) {
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
fn extract_ts_class_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
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
fn detect_ts_field_visibility(node: &Node, source: &str) -> Option<Visibility> {
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
fn extract_ts_interface_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32) {
            if child.kind() == "property_signature" {
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
}

/// Find the `extends` clause in a TS class declaration.
fn find_ts_extends(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "class_heritage" {
                // Walk children looking for `extends_clause`
                for j in 0..child.child_count() {
                    if let Some(clause) = child.child(j as u32) {
                        if clause.kind() == "extends_clause" {
                            // The type follows the `extends` keyword
                            return extract_first_type_from_clause(&clause, source);
                        }
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
fn find_ts_implements(node: &Node, source: &str) -> Vec<String> {
    let mut result = Vec::new();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "class_heritage" {
                for j in 0..child.child_count() {
                    if let Some(clause) = child.child(j as u32) {
                        if clause.kind() == "implements_clause" {
                            collect_types_from_clause(&clause, source, &mut result);
                        }
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
fn find_ts_interface_extends(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "extends_type_clause" || child.kind() == "extends_clause" {
                return extract_first_type_from_clause(&child, source);
            }
        }
    }
    None
}

/// Extract the first type identifier from an extends/implements clause.
fn extract_first_type_from_clause(clause: &Node, source: &str) -> Option<String> {
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
fn collect_types_from_clause(clause: &Node, source: &str, result: &mut Vec<String>) {
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

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

/// Extract Python class definitions with `__init__` field assignments.
///
/// CST shape:
/// - `class_definition` with `name` and `body` (`block`)
///   - `__init__` method body contains `self.field = ...` assignments
///   - Type annotations: `self.field: type = ...` in the `__init__` body
fn try_extract_python_model(
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
fn find_python_superclass(node: &Node, source: &str) -> Option<String> {
    if let Some(superclasses) = child_by_field(node, "superclasses") {
        // argument_list containing identifiers
        for i in 0..superclasses.child_count() {
            if let Some(child) = superclasses.child(i as u32) {
                if child.kind() == "identifier" || child.kind() == "attribute" {
                    let text = node_text(&child, source);
                    if !text.is_empty() {
                        return Some(text);
                    }
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
fn extract_python_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    // Walk body looking for `__init__` function_definition
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32) {
            if child.kind() == "function_definition" {
                let fn_name = child_by_field(&child, "name")
                    .map(|n| node_text(&n, source))
                    .unwrap_or_default();
                if fn_name == "__init__" {
                    if let Some(fn_body) = child_by_field(&child, "body") {
                        extract_python_init_fields(&fn_body, source, fields);
                    }
                }
            }
            // Also check class-level typed assignments (e.g., dataclass fields)
            if child.kind() == "expression_statement" {
                if let Some(inner) = child.child(0_u32) {
                    try_extract_python_class_level_field(&inner, source, fields);
                }
            }
        }
    }
}

/// Extract `self.field = ...` and `self.field: type = ...` from `__init__` body.
fn extract_python_init_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32) {
            if child.kind() == "expression_statement" {
                if let Some(inner) = child.child(0_u32) {
                    if inner.kind() == "assignment" {
                        try_extract_self_assignment(&inner, source, fields);
                    }
                }
            }
        }
    }
}

/// Extract field from `self.field = value` or `self.field: type = value`.
fn try_extract_self_assignment(node: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
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
fn try_extract_python_class_level_field(node: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
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

// ---------------------------------------------------------------------------
// Java / Kotlin
// ---------------------------------------------------------------------------

/// Extract Java/Kotlin class declarations with field declarations.
///
/// CST shape:
/// - `class_declaration` with `name`, optional `superclass`, optional `interfaces`
///   - body: `class_body` containing `field_declaration` nodes
fn try_extract_java_model(
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
fn find_java_superclass(node: &Node, source: &str) -> Option<String> {
    if let Some(super_node) = child_by_field(node, "superclass") {
        let text = node_text(&super_node, source);
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// Find Java implemented interfaces from `implements` clause.
fn find_java_interfaces(node: &Node, source: &str) -> Vec<String> {
    let mut result = Vec::new();
    if let Some(interfaces_node) = child_by_field(node, "interfaces") {
        for i in 0..interfaces_node.child_count() {
            if let Some(child) = interfaces_node.child(i as u32) {
                let kind = child.kind();
                if kind == "type_identifier" || kind == "generic_type" || kind == "type_list" {
                    if kind == "type_list" {
                        // type_list contains multiple type identifiers
                        for j in 0..child.child_count() {
                            if let Some(typ) = child.child(j as u32) {
                                if typ.kind() == "type_identifier" || typ.kind() == "generic_type" {
                                    let text = node_text(&typ, source);
                                    if !text.is_empty() {
                                        result.push(text);
                                    }
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
fn extract_java_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32) {
            if child.kind() == "field_declaration" {
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
}

/// Detect Java visibility from modifier keywords.
fn detect_java_visibility(node: &Node, source: &str) -> Option<Visibility> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "modifiers" {
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
    }
    None
}

// ---------------------------------------------------------------------------
// C#
// ---------------------------------------------------------------------------

/// Extract C# class/struct/interface declarations with field declarations.
///
/// CST shape:
/// - `class_declaration`, `struct_declaration`, `interface_declaration`
///   - `name` (identifier)
///   - body: `declaration_list` containing `field_declaration`
fn try_extract_csharp_model(
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
fn find_csharp_base_type(node: &Node, source: &str) -> Option<String> {
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
fn extract_csharp_fields(body: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = body.child_count();
    for i in 0..count {
        if let Some(child) = body.child(i as u32) {
            if child.kind() == "field_declaration" {
                let field_type = child_by_field(&child, "type").map(|n| node_text(&n, source));

                let visibility = detect_csharp_visibility(&child, source);

                // variable_declaration → variable_declarator → identifier
                if let Some(var_decl) = child_by_field(&child, "declaration") {
                    for j in 0..var_decl.child_count() {
                        if let Some(declarator) = var_decl.child(j as u32) {
                            if declarator.kind() == "variable_declarator" {
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
    }
}

/// Detect C# visibility from modifier nodes.
fn detect_csharp_visibility(node: &Node, source: &str) -> Option<Visibility> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "modifier" {
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
    }
    None
}

// ---------------------------------------------------------------------------
// Go
// ---------------------------------------------------------------------------

/// Extract Go struct types from `type X struct { ... }` declarations.
///
/// CST shape:
/// - `type_declaration` containing `type_spec`
///   - `type_spec` has `name` (type_identifier) and `type` (struct_type)
///   - `struct_type` has `field_declaration_list` → `field_declaration` nodes
fn try_extract_go_model(node: &Node, source: &str, file_path: &Path, models: &mut Vec<DataModel>) {
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
fn extract_go_struct_fields(struct_node: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    // struct_type → field_declaration_list → field_declaration
    if let Some(field_list) = find_child_by_kind(struct_node, "field_declaration_list") {
        let count = field_list.child_count();
        for i in 0..count {
            if let Some(field) = field_list.child(i as u32) {
                if field.kind() == "field_declaration" {
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
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

/// Extract Rust struct items with field declarations.
///
/// CST shape:
/// - `struct_item` with `name` (type_identifier)
///   - `field_declaration_list` → `field_declaration` with `name` and `type`
fn try_extract_rust_model(
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
    if let Some(body) = child_by_field(node, "body") {
        if body.kind() == "field_declaration_list" {
            extract_rust_fields(&body, source, &mut fields);
        }
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
fn extract_rust_fields(list: &Node, source: &str, fields: &mut Vec<FieldInfo>) {
    let count = list.child_count();
    for i in 0..count {
        if let Some(child) = list.child(i as u32) {
            if child.kind() == "field_declaration" {
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
}

/// Check if a Rust node has a `visibility_modifier` child (i.e., `pub`).
fn has_visibility_modifier(node: &Node) -> bool {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "visibility_modifier" {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Find a named child (field) by its tree-sitter field name.
fn child_by_field<'a>(node: &'a Node<'a>, field_name: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field_name)
}

/// Find the first child with a given node kind.
fn find_child_by_kind<'a>(node: &'a Node<'a>, kind: &str) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == kind {
                return Some(child);
            }
        }
    }
    None
}

/// Extract the text of a type annotation node, stripping a leading `:` if present.
///
/// In TypeScript, type annotations are often `type_annotation` nodes that
/// include the colon, e.g., `: string`. We strip the colon for clean output.
fn extract_type_annotation_text(node: &Node, source: &str) -> String {
    let text = node_text(node, source);
    let trimmed = text.trim();
    if let Some(stripped) = trimmed.strip_prefix(':') {
        stripped.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    

    fn parse_and_extract(source: &str, language: SupportedLanguage) -> Vec<DataModel> {
        let path = PathBuf::from("test_file");
        let parsed = crate::tree_sitter::parse_source(&path, source, language, None).unwrap();
        extract_data_models(source, &parsed.tree, language, &path)
    }

    // 1. TypeScript class with typed fields
    #[test]
    fn typescript_class_with_typed_fields() {
        let models = parse_and_extract(
            r#"
class User {
    public name: string;
    private email: string;
    age: number;
}
"#,
            SupportedLanguage::TypeScript,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "User");
        assert_eq!(model.model_kind, DataModelKind::Class);
        assert_eq!(model.fields.len(), 3);

        let name_field = model.fields.iter().find(|f| f.name == "name").unwrap();
        assert_eq!(name_field.field_type.as_deref(), Some("string"));
        assert_eq!(name_field.visibility, Some(Visibility::Public));

        let email_field = model.fields.iter().find(|f| f.name == "email").unwrap();
        assert_eq!(email_field.field_type.as_deref(), Some("string"));
        assert_eq!(email_field.visibility, Some(Visibility::Private));

        let age_field = model.fields.iter().find(|f| f.name == "age").unwrap();
        assert_eq!(age_field.field_type.as_deref(), Some("number"));
    }

    // 2. TypeScript interface with property signatures
    #[test]
    fn typescript_interface_with_property_signatures() {
        let models = parse_and_extract(
            r#"
interface Product {
    id: number;
    title: string;
    price: number;
}
"#,
            SupportedLanguage::TypeScript,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Product");
        assert_eq!(model.model_kind, DataModelKind::Interface);
        assert_eq!(model.fields.len(), 3);

        let id_field = model.fields.iter().find(|f| f.name == "id").unwrap();
        assert_eq!(id_field.field_type.as_deref(), Some("number"));

        let title_field = model.fields.iter().find(|f| f.name == "title").unwrap();
        assert_eq!(title_field.field_type.as_deref(), Some("string"));
    }

    // 3. Python class with __init__ assignments
    #[test]
    fn python_class_with_init_assignments() {
        let models = parse_and_extract(
            r#"
class User:
    def __init__(self, name, email):
        self.name = name
        self.email = email
        self._internal = True
"#,
            SupportedLanguage::Python,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "User");
        assert_eq!(model.model_kind, DataModelKind::Class);
        assert_eq!(model.fields.len(), 3);

        let name_field = model.fields.iter().find(|f| f.name == "name").unwrap();
        assert_eq!(name_field.visibility, Some(Visibility::Public));

        let internal_field = model.fields.iter().find(|f| f.name == "_internal").unwrap();
        assert_eq!(internal_field.visibility, Some(Visibility::Private));
    }

    // 4. Java class with field declarations
    #[test]
    fn java_class_with_field_declarations() {
        let models = parse_and_extract(
            r#"
public class Order {
    private String orderId;
    public double total;
    protected String status;
}
"#,
            SupportedLanguage::Java,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Order");
        assert_eq!(model.model_kind, DataModelKind::Class);
        assert_eq!(model.fields.len(), 3);

        let order_id = model.fields.iter().find(|f| f.name == "orderId").unwrap();
        assert_eq!(order_id.field_type.as_deref(), Some("String"));
        assert_eq!(order_id.visibility, Some(Visibility::Private));

        let total = model.fields.iter().find(|f| f.name == "total").unwrap();
        assert_eq!(total.field_type.as_deref(), Some("double"));
        assert_eq!(total.visibility, Some(Visibility::Public));

        let status = model.fields.iter().find(|f| f.name == "status").unwrap();
        assert_eq!(status.visibility, Some(Visibility::Protected));
    }

    // 5. Go struct with typed fields
    #[test]
    fn go_struct_with_typed_fields() {
        let models = parse_and_extract(
            r#"
package main

type Server struct {
    Host string
    Port int
    debug bool
}
"#,
            SupportedLanguage::Go,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Server");
        assert_eq!(model.model_kind, DataModelKind::Struct);
        assert_eq!(model.fields.len(), 3);

        let host = model.fields.iter().find(|f| f.name == "Host").unwrap();
        assert_eq!(host.field_type.as_deref(), Some("string"));
        assert_eq!(host.visibility, Some(Visibility::Public));

        let debug = model.fields.iter().find(|f| f.name == "debug").unwrap();
        assert_eq!(debug.field_type.as_deref(), Some("bool"));
        assert_eq!(debug.visibility, Some(Visibility::Private));
    }

    // 6. Rust struct with typed fields
    #[test]
    fn rust_struct_with_typed_fields() {
        let models = parse_and_extract(
            r#"
pub struct Config {
    pub host: String,
    pub port: u16,
    secret: String,
}
"#,
            SupportedLanguage::Rust,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Config");
        assert_eq!(model.model_kind, DataModelKind::Struct);
        assert_eq!(model.fields.len(), 3);

        let host = model.fields.iter().find(|f| f.name == "host").unwrap();
        assert_eq!(host.field_type.as_deref(), Some("String"));
        assert_eq!(host.visibility, Some(Visibility::Public));

        let secret = model.fields.iter().find(|f| f.name == "secret").unwrap();
        assert_eq!(secret.field_type.as_deref(), Some("String"));
        assert_eq!(secret.visibility, Some(Visibility::Private));
    }

    // 7. Class with parent_type detection (extends keyword)
    #[test]
    fn typescript_class_with_extends() {
        let models = parse_and_extract(
            r#"
class Admin extends User {
    role: string;
}
"#,
            SupportedLanguage::TypeScript,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Admin");
        assert_eq!(model.parent_type.as_deref(), Some("User"));
        assert_eq!(model.fields.len(), 1);
    }

    // 8. Empty class returns model with zero fields
    #[test]
    fn empty_class_returns_model_with_zero_fields() {
        let models = parse_and_extract(
            r#"
class Empty {}
"#,
            SupportedLanguage::TypeScript,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Empty");
        assert_eq!(model.model_kind, DataModelKind::Class);
        assert!(model.fields.is_empty());
        assert!(model.parent_type.is_none());
    }
}
