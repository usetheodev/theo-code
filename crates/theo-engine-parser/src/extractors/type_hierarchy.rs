//! Type hierarchy extraction from tree-sitter CSTs.
//!
//! Walks the CST to find class inheritance (`extends`) and interface/trait
//! implementation (`implements`) relationships. Each relationship produces
//! a [`Reference`] with the appropriate [`ReferenceKind`].
//!
//! Supported patterns per language:
//! - **TypeScript/JavaScript**: `class Foo extends Bar implements IBaz`
//! - **Python**: `class Foo(Bar, Baz):`
//! - **Java/Kotlin**: `class Foo extends Bar implements IBaz`
//! - **C#**: `class Foo : Bar, IBaz`
//! - **Go**: embedded struct fields (unnamed fields = composition)
//! - **Rust**: `impl Trait for Type`

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::tree_sitter::SupportedLanguage;
use crate::types::{Reference, ReferenceKind, ResolutionMethod};

use super::common::node_text;

/// Extract type hierarchy references (extends/implements) from a source file's CST.
///
/// Returns a `Vec<Reference>` where each entry represents either an
/// inheritance (`Extends`) or implementation (`Implements`) relationship.
/// Target files and lines are left as `None` — cross-file resolution
/// happens in a later stage.
pub fn extract_type_hierarchy(
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
    file_path: &Path,
) -> Vec<Reference> {
    let mut references = Vec::new();
    let root = tree.root_node();
    walk_node(&root, source, language, file_path, &mut references);
    references
}

fn walk_node(
    node: &Node,
    source: &str,
    language: SupportedLanguage,
    file_path: &Path,
    references: &mut Vec<Reference>,
) {
    match language {
        SupportedLanguage::TypeScript
        | SupportedLanguage::Tsx
        | SupportedLanguage::JavaScript
        | SupportedLanguage::Jsx => {
            extract_ts_class_hierarchy(node, source, file_path, references);
        }
        SupportedLanguage::Python => {
            extract_python_class_hierarchy(node, source, file_path, references);
        }
        SupportedLanguage::Java | SupportedLanguage::Kotlin => {
            extract_java_class_hierarchy(node, source, file_path, references);
        }
        SupportedLanguage::CSharp => {
            extract_csharp_class_hierarchy(node, source, file_path, references);
        }
        SupportedLanguage::Go => {
            extract_go_embedding(node, source, file_path, references);
        }
        SupportedLanguage::Rust => {
            extract_rust_impl_for(node, source, file_path, references);
        }
        _ => {
            // Scala, Swift, C, C++ — no extraction yet
        }
    }

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            walk_node(&child, source, language, file_path, references);
        }
    }
}

/// TypeScript/JavaScript: `class Foo extends Bar implements IBaz { ... }`
///
/// CST structure:
/// ```text
/// (class_declaration
///   name: (type_identifier)
///   (class_heritage
///     (extends_clause (identifier))
///     (implements_clause (type_identifier) ...)))
/// ```
fn extract_ts_class_hierarchy(
    node: &Node,
    source: &str,
    file_path: &Path,
    references: &mut Vec<Reference>,
) {
    if node.kind() != "class_declaration" && node.kind() != "class" {
        return;
    }

    let class_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "class_heritage" {
                extract_ts_heritage(&child, source, file_path, &class_name, references);
            }
        }
    }
}

fn extract_ts_heritage(
    heritage_node: &Node,
    source: &str,
    file_path: &Path,
    class_name: &str,
    references: &mut Vec<Reference>,
) {
    let child_count = heritage_node.child_count();
    for i in 0..child_count {
        if let Some(clause) = heritage_node.child(i as u32) {
            match clause.kind() {
                "extends_clause" => {
                    // Children after the "extends" keyword are the type(s)
                    let type_count = clause.child_count();
                    for j in 0..type_count {
                        if let Some(type_node) = clause.child(j as u32) {
                            let kind = type_node.kind();
                            if kind == "identifier" || kind == "type_identifier" {
                                let target = node_text(&type_node, source);
                                references.push(Reference {
                                    source_symbol: class_name.to_string(),
                                    source_file: file_path.to_path_buf(),
                                    source_line: clause.start_position().row + 1,
                                    target_symbol: target,
                                    target_file: None,
                                    target_line: None,
                                    reference_kind: ReferenceKind::Extends,
                                    confidence: 0.0,
                                    resolution_method: ResolutionMethod::Unresolved,
                                    is_test_reference: false,
                                });
                            }
                        }
                    }
                }
                "implements_clause" => {
                    let type_count = clause.child_count();
                    for j in 0..type_count {
                        if let Some(type_node) = clause.child(j as u32) {
                            let kind = type_node.kind();
                            if kind == "identifier"
                                || kind == "type_identifier"
                                || kind == "generic_type"
                            {
                                // For generic_type, extract just the base name
                                let target = if kind == "generic_type" {
                                    type_node
                                        .child(0)
                                        .map(|n| node_text(&n, source))
                                        .unwrap_or_else(|| node_text(&type_node, source))
                                } else {
                                    node_text(&type_node, source)
                                };
                                references.push(Reference {
                                    source_symbol: class_name.to_string(),
                                    source_file: file_path.to_path_buf(),
                                    source_line: clause.start_position().row + 1,
                                    target_symbol: target,
                                    target_file: None,
                                    target_line: None,
                                    reference_kind: ReferenceKind::Implements,
                                    confidence: 0.0,
                                    resolution_method: ResolutionMethod::Unresolved,
                                    is_test_reference: false,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Python: `class Foo(Bar, Baz):`
///
/// CST structure:
/// ```text
/// (class_definition
///   name: (identifier)
///   superclasses: (argument_list (identifier) (identifier)))
/// ```
///
/// Python does not distinguish extends from implements at the syntax level.
/// All parent classes are treated as `Extends`.
fn extract_python_class_hierarchy(
    node: &Node,
    source: &str,
    file_path: &Path,
    references: &mut Vec<Reference>,
) {
    if node.kind() != "class_definition" {
        return;
    }

    let class_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();

    // The superclasses field is an argument_list containing parent classes
    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        let child_count = superclasses.child_count();
        for i in 0..child_count {
            if let Some(child) = superclasses.child(i as u32) {
                let kind = child.kind();
                if kind == "identifier" || kind == "attribute" {
                    let target = node_text(&child, source);
                    references.push(Reference {
                        source_symbol: class_name.to_string(),
                        source_file: file_path.to_path_buf(),
                        source_line: superclasses.start_position().row + 1,
                        target_symbol: target,
                        target_file: None,
                        target_line: None,
                        reference_kind: ReferenceKind::Extends,
                        confidence: 0.0,
                        resolution_method: ResolutionMethod::Unresolved,
                        is_test_reference: false,
                    });
                }
            }
        }
    }
}

/// Java/Kotlin: `class Foo extends Bar implements IBaz`
///
/// Java CST structure:
/// ```text
/// (class_declaration
///   name: (identifier)
///   (superclass (type_identifier))
///   (super_interfaces (type_list (type_identifier) ...)))
/// ```
///
/// Kotlin CST structure:
/// ```text
/// (class_declaration
///   (type_identifier)
///   (delegation_specifier_list
///     (delegation_specifier (user_type ...))))
/// ```
fn extract_java_class_hierarchy(
    node: &Node,
    source: &str,
    file_path: &Path,
    references: &mut Vec<Reference>,
) {
    if node.kind() != "class_declaration" && node.kind() != "interface_declaration" {
        return;
    }

    let class_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                // Java: `extends Bar`
                "superclass" => {
                    extract_type_identifiers_from(
                        &child,
                        source,
                        file_path,
                        &class_name,
                        ReferenceKind::Extends,
                        references,
                    );
                }
                // Java: `implements IBaz, IFoo`
                "super_interfaces" => {
                    extract_type_identifiers_from(
                        &child,
                        source,
                        file_path,
                        &class_name,
                        ReferenceKind::Implements,
                        references,
                    );
                }
                // Java interface: `extends IFoo, IBar`
                "extends_interfaces" => {
                    extract_type_identifiers_from(
                        &child,
                        source,
                        file_path,
                        &class_name,
                        ReferenceKind::Extends,
                        references,
                    );
                }
                // Kotlin: delegation_specifier_list
                "delegation_specifier_list" => {
                    // In Kotlin all supertypes go into delegation_specifier_list.
                    // Without semantic info we treat them all as Extends.
                    extract_type_identifiers_from(
                        &child,
                        source,
                        file_path,
                        &class_name,
                        ReferenceKind::Extends,
                        references,
                    );
                }
                _ => {}
            }
        }
    }
}

/// C#: `class Foo : Bar, IBaz`
///
/// CST structure:
/// ```text
/// (class_declaration
///   name: (identifier)
///   (base_list
///     (simple_base_type (identifier))
///     (simple_base_type (identifier))))
/// ```
///
/// C# does not syntactically distinguish extends from implements in the base list.
/// By convention, the first base type is the parent class (Extends) and
/// subsequent ones starting with "I" are interfaces (Implements). However,
/// since we cannot reliably distinguish at the syntax level, we treat
/// all base types as Extends for correctness.
fn extract_csharp_class_hierarchy(
    node: &Node,
    source: &str,
    file_path: &Path,
    references: &mut Vec<Reference>,
) {
    if node.kind() != "class_declaration"
        && node.kind() != "struct_declaration"
        && node.kind() != "interface_declaration"
    {
        return;
    }

    let class_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "base_list" {
                let base_count = child.child_count();
                for j in 0..base_count {
                    if let Some(base_type) = child.child(j as u32) {
                        // Skip punctuation (colon, commas)
                        if base_type.kind() == ":" || base_type.kind() == "," {
                            continue;
                        }
                        // Extract the type name from the base type node
                        let target = extract_deepest_identifier(&base_type, source);
                        if !target.is_empty() {
                            // Heuristic: names starting with "I" followed by uppercase
                            // are interfaces in C# convention
                            let kind = if is_csharp_interface_name(&target) {
                                ReferenceKind::Implements
                            } else {
                                ReferenceKind::Extends
                            };
                            references.push(Reference {
                                source_symbol: class_name.to_string(),
                                source_file: file_path.to_path_buf(),
                                source_line: base_type.start_position().row + 1,
                                target_symbol: target,
                                target_file: None,
                                target_line: None,
                                reference_kind: kind,
                                confidence: 0.0,
                                resolution_method: ResolutionMethod::Unresolved,
                                is_test_reference: false,
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Go: embedded struct fields (unnamed fields represent composition).
///
/// CST structure:
/// ```text
/// (type_declaration
///   (type_spec
///     name: (type_identifier)
///     type: (struct_type
///       (field_declaration_list
///         (field_declaration
///           type: (type_identifier))       // embedded — no name field
///         (field_declaration
///           name: (field_identifier)        // named — not embedded
///           type: (type_identifier))))))
/// ```
fn extract_go_embedding(
    node: &Node,
    source: &str,
    file_path: &Path,
    references: &mut Vec<Reference>,
) {
    // Look for type_spec containing a struct_type
    if node.kind() != "type_spec" {
        return;
    }

    let struct_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();

    let type_node = match node.child_by_field_name("type") {
        Some(t) if t.kind() == "struct_type" || t.kind() == "interface_type" => t,
        _ => return,
    };

    // Find the field_declaration_list
    let body = find_child_by_kind(&type_node, "field_declaration_list");
    let body = match body {
        Some(b) => b,
        None => return,
    };

    let field_count = body.child_count();
    for i in 0..field_count {
        if let Some(field) = body.child(i as u32) {
            if field.kind() != "field_declaration" {
                continue;
            }
            // An embedded field has no "name" field — only a "type" field
            if field.child_by_field_name("name").is_some() {
                continue; // Named field, not embedded
            }
            if let Some(type_child) = field.child_by_field_name("type") {
                let target = node_text(&type_child, source);
                // Skip pointer prefixes
                let target = target.trim_start_matches('*').to_string();
                if !target.is_empty() {
                    references.push(Reference {
                        source_symbol: struct_name.to_string(),
                        source_file: file_path.to_path_buf(),
                        source_line: field.start_position().row + 1,
                        target_symbol: target,
                        target_file: None,
                        target_line: None,
                        reference_kind: ReferenceKind::Extends,
                        confidence: 0.0,
                        resolution_method: ResolutionMethod::Unresolved,
                        is_test_reference: false,
                    });
                }
            }
        }
    }
}

/// Rust: `impl Trait for Type { ... }`
///
/// CST structure:
/// ```text
/// (impl_item
///   trait: (type_identifier)
///   type: (type_identifier)
///   body: (declaration_list ...))
/// ```
fn extract_rust_impl_for(
    node: &Node,
    source: &str,
    file_path: &Path,
    references: &mut Vec<Reference>,
) {
    if node.kind() != "impl_item" {
        return;
    }

    // `impl Trait for Type` has both "trait" and "type" fields.
    // Plain `impl Type` has only "type" — skip those.
    let trait_node = match node.child_by_field_name("trait") {
        Some(t) => t,
        None => return,
    };
    let type_node = match node.child_by_field_name("type") {
        Some(t) => t,
        None => return,
    };

    let trait_name = node_text(&trait_node, source);
    let type_name = node_text(&type_node, source);

    references.push(Reference {
        source_symbol: type_name,
        source_file: file_path.to_path_buf(),
        source_line: node.start_position().row + 1,
        target_symbol: trait_name,
        target_file: None,
        target_line: None,
        reference_kind: ReferenceKind::Implements,
        confidence: 0.0,
        resolution_method: ResolutionMethod::Unresolved,
        is_test_reference: false,
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Recursively extract type identifiers from a node and add them as references.
fn extract_type_identifiers_from(
    node: &Node,
    source: &str,
    file_path: &Path,
    class_name: &str,
    kind: ReferenceKind,
    references: &mut Vec<Reference>,
) {
    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "type_identifier" | "identifier" | "simple_identifier" => {
                    let target = node_text(&child, source);
                    references.push(Reference {
                        source_symbol: class_name.to_string(),
                        source_file: file_path.to_path_buf(),
                        source_line: child.start_position().row + 1,
                        target_symbol: target,
                        target_file: None,
                        target_line: None,
                        reference_kind: kind,
                        confidence: 0.0,
                        resolution_method: ResolutionMethod::Unresolved,
                        is_test_reference: false,
                    });
                }
                // Recurse into intermediate nodes (type_list, generic_type, etc.)
                _ => {
                    extract_type_identifiers_from(
                        &child, source, file_path, class_name, kind, references,
                    );
                }
            }
        }
    }
}

/// Find a direct child node with a given kind.
fn find_child_by_kind<'a>(node: &'a Node, kind: &str) -> Option<Node<'a>> {
    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == kind {
                return Some(child);
            }
        }
    }
    None
}

/// Extract the deepest identifier from a type node (handles nested generic_type etc.).
fn extract_deepest_identifier(node: &Node, source: &str) -> String {
    match node.kind() {
        "identifier" | "type_identifier" | "simple_identifier" | "predefined_type" => {
            node_text(node, source)
        }
        _ => {
            // Try children — look for the first identifier-like child
            let count = node.child_count();
            for i in 0..count {
                if let Some(child) = node.child(i as u32) {
                    let result = extract_deepest_identifier(&child, source);
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
            String::new()
        }
    }
}

/// Heuristic: C# interface names start with "I" followed by an uppercase letter.
fn is_csharp_interface_name(name: &str) -> bool {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some('I'), Some(c)) => c.is_uppercase(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter::SupportedLanguage;
    use std::path::PathBuf;

    fn parse_and_extract(source: &str, language: SupportedLanguage) -> Vec<Reference> {
        let path = PathBuf::from("test_file");
        let parsed = crate::tree_sitter::parse_source(&path, source, language, None).unwrap();
        extract_type_hierarchy(source, &parsed.tree, language, &path)
    }

    #[test]
    fn typescript_class_extends_another() {
        let refs = parse_and_extract(
            "class Dog extends Animal { bark() {} }",
            SupportedLanguage::TypeScript,
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_symbol, "Dog");
        assert_eq!(refs[0].target_symbol, "Animal");
        assert_eq!(refs[0].reference_kind, ReferenceKind::Extends);
        assert!(refs[0].target_file.is_none());
    }

    #[test]
    fn typescript_class_implements_interface() {
        let refs = parse_and_extract(
            "class UserService implements IService { run() {} }",
            SupportedLanguage::TypeScript,
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_symbol, "UserService");
        assert_eq!(refs[0].target_symbol, "IService");
        assert_eq!(refs[0].reference_kind, ReferenceKind::Implements);
    }

    #[test]
    fn python_class_inherits_from_parent() {
        let refs = parse_and_extract("class Dog(Animal):\n    pass", SupportedLanguage::Python);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_symbol, "Dog");
        assert_eq!(refs[0].target_symbol, "Animal");
        assert_eq!(refs[0].reference_kind, ReferenceKind::Extends);
    }

    #[test]
    fn java_class_extends_and_implements() {
        let refs = parse_and_extract(
            "public class Dog extends Animal implements Runnable { }",
            SupportedLanguage::Java,
        );
        assert_eq!(refs.len(), 2);

        let extends_ref = refs
            .iter()
            .find(|r| r.reference_kind == ReferenceKind::Extends);
        let implements_ref = refs
            .iter()
            .find(|r| r.reference_kind == ReferenceKind::Implements);

        assert!(extends_ref.is_some(), "should have an Extends reference");
        assert!(
            implements_ref.is_some(),
            "should have an Implements reference"
        );

        assert_eq!(extends_ref.unwrap().target_symbol, "Animal");
        assert_eq!(implements_ref.unwrap().target_symbol, "Runnable");
        assert_eq!(extends_ref.unwrap().source_symbol, "Dog");
    }

    #[test]
    fn csharp_class_with_base_list() {
        let refs = parse_and_extract(
            "public class Dog : Animal, IRunnable { }",
            SupportedLanguage::CSharp,
        );
        assert!(
            refs.len() >= 2,
            "should have at least 2 references, got {}",
            refs.len()
        );

        let extends_ref = refs
            .iter()
            .find(|r| r.reference_kind == ReferenceKind::Extends);
        let implements_ref = refs
            .iter()
            .find(|r| r.reference_kind == ReferenceKind::Implements);

        assert!(
            extends_ref.is_some(),
            "should have an Extends reference for Animal"
        );
        assert!(
            implements_ref.is_some(),
            "should have an Implements reference for IRunnable"
        );

        assert_eq!(extends_ref.unwrap().target_symbol, "Animal");
        assert_eq!(implements_ref.unwrap().target_symbol, "IRunnable");
    }

    #[test]
    fn go_struct_with_embedding() {
        let refs = parse_and_extract(
            "package main\ntype Dog struct {\n    Animal\n    Name string\n}",
            SupportedLanguage::Go,
        );
        assert_eq!(
            refs.len(),
            1,
            "only the embedded field (Animal) should produce a reference"
        );
        assert_eq!(refs[0].source_symbol, "Dog");
        assert_eq!(refs[0].target_symbol, "Animal");
        assert_eq!(refs[0].reference_kind, ReferenceKind::Extends);
    }

    #[test]
    fn rust_impl_trait_for_type() {
        let refs = parse_and_extract(
            "impl Display for Dog { fn fmt(&self, f: &mut Formatter) -> Result { Ok(()) } }",
            SupportedLanguage::Rust,
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_symbol, "Dog");
        assert_eq!(refs[0].target_symbol, "Display");
        assert_eq!(refs[0].reference_kind, ReferenceKind::Implements);
    }

    #[test]
    fn class_with_no_inheritance_returns_empty() {
        let refs = parse_and_extract(
            "class StandaloneClass { constructor() {} }",
            SupportedLanguage::TypeScript,
        );
        assert!(
            refs.is_empty(),
            "class with no inheritance should return empty"
        );
    }
}
