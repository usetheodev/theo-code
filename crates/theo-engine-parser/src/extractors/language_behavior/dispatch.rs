//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;

pub static TYPESCRIPT_BEHAVIOR: TypeScriptBehavior = TypeScriptBehavior;
pub static PYTHON_BEHAVIOR: PythonBehavior = PythonBehavior;
pub static JAVA_BEHAVIOR: JavaBehavior = JavaBehavior;
pub static CSHARP_BEHAVIOR: CSharpBehavior = CSharpBehavior;
pub static GO_BEHAVIOR: GoBehavior = GoBehavior;
pub static PHP_BEHAVIOR: PhpBehavior = PhpBehavior;
pub static RUBY_BEHAVIOR: RubyBehavior = RubyBehavior;
pub static RUST_BEHAVIOR: RustBehavior = RustBehavior;
pub static GENERIC_BEHAVIOR: GenericBehavior = GenericBehavior;

/// Return the [`LanguageBehavior`] implementation for a given language.
///
/// Languages that share a grammar family map to the same behavior:
/// - TypeScript, TSX, JavaScript, JSX -> [`TypeScriptBehavior`]
/// - Java, Kotlin, Scala -> [`JavaBehavior`]
/// - C, C++, Swift -> [`GenericBehavior`]
pub fn behavior_for(language: SupportedLanguage) -> &'static dyn LanguageBehavior {
    match language {
        SupportedLanguage::TypeScript
        | SupportedLanguage::Tsx
        | SupportedLanguage::JavaScript
        | SupportedLanguage::Jsx => &TYPESCRIPT_BEHAVIOR,
        SupportedLanguage::Python => &PYTHON_BEHAVIOR,
        SupportedLanguage::Java | SupportedLanguage::Kotlin => &JAVA_BEHAVIOR,
        SupportedLanguage::CSharp => &CSHARP_BEHAVIOR,
        SupportedLanguage::Go => &GO_BEHAVIOR,
        SupportedLanguage::Php => &PHP_BEHAVIOR,
        SupportedLanguage::Ruby => &RUBY_BEHAVIOR,
        SupportedLanguage::Rust => &RUST_BEHAVIOR,
        SupportedLanguage::Swift | SupportedLanguage::C | SupportedLanguage::Cpp => {
            &GENERIC_BEHAVIOR
        }
        SupportedLanguage::Scala => &JAVA_BEHAVIOR,
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (used by trait implementations)
// ---------------------------------------------------------------------------

/// Truncate text at the first occurrence of `ch`, trimming whitespace.
pub fn truncate_at_char(text: &str, ch: char) -> Option<String> {
    text.find(ch).map(|pos| text[..pos].trim().to_string())
}

/// Parse a visibility keyword string into [`Visibility`].
pub fn parse_visibility_keyword(keyword: &str) -> Option<Visibility> {
    match keyword.trim() {
        "public" => Some(Visibility::Public),
        "private" => Some(Visibility::Private),
        "protected" => Some(Visibility::Protected),
        "internal" => Some(Visibility::Internal),
        _ => None,
    }
}

/// Look for visibility modifier keywords in child nodes (Java/PHP pattern).
pub fn extract_visibility_modifier_child(node: &Node, source: &str) -> Option<Visibility> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let kind = child.kind();
            if kind == "modifiers" || kind == "modifier" || kind == "visibility_modifier" {
                let mod_text = match child.utf8_text(source.as_bytes()) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if let Some(vis) = parse_visibility_keyword(mod_text) {
                    return Some(vis);
                }
            }
            // Direct keyword nodes (some grammars use these)
            if let Some(vis) = parse_visibility_keyword(kind) {
                return Some(vis);
            }
        }
    }
    // Fallback: check if text starts with a visibility keyword
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let first_word = text.split_whitespace().next()?;
    parse_visibility_keyword(first_word)
}

/// Extract the name identifier from a node (looks for common name child kinds).
pub fn extract_name_from_node(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            let kind = child.kind();
            if kind == "identifier"
                || kind == "type_identifier"
                || kind == "name"
                || kind == "constant"
                || kind == "property_identifier"
                || kind == "field_identifier"
            {
                return child
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
        }
    }
    None
}

/// Generic parent finder: walk up looking for class/module/trait/impl nodes.
pub fn find_parent_generic(node: &Node, source: &str) -> Option<String> {
    let mut current = node.parent()?;
    loop {
        let kind = current.kind();
        if is_enclosing_type(kind) {
            return extract_name_child(&current, source);
        }
        current = current.parent()?;
    }
}

/// Check if a CST node kind represents an enclosing type definition.
pub fn is_enclosing_type(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "class_definition"
            | "class"
            | "record_declaration"
            | "interface_declaration"
            | "trait_item"
            | "trait_declaration"
            | "impl_item"
            | "struct_declaration"
            | "struct_item"
            | "enum_declaration"
            | "enum_item"
            | "module"
            | "mod_item"
    )
}

/// Extract the `name:` child text from an enclosing type node.
pub fn extract_name_child(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            let kind = child.kind();
            if kind == "identifier"
                || kind == "type_identifier"
                || kind == "name"
                || kind == "constant"
                || kind == "property_identifier"
            {
                return child
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Test detection helpers
// ---------------------------------------------------------------------------

/// Check for a Java/Kotlin annotation (`@Name`) on a method_declaration node.
///
/// Java tree-sitter grammar nests annotations inside `modifiers` child nodes
/// of the declaration. We walk the node's children looking for `modifiers`
/// containing `marker_annotation` or `annotation` nodes, and also check
/// direct preceding siblings (some grammar versions place them there).
pub fn has_preceding_annotation(node: &Node, source: &str, names: &[&str]) -> bool {
    // Strategy 1: Check child `modifiers` node (Java grammar nests annotations here)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let kind = child.kind();
            if kind == "modifiers" {
                // Walk inside modifiers for annotations
                for j in 0..child.child_count() {
                    if let Some(mod_child) = child.child(j as u32)
                        && check_annotation_node(&mod_child, source, names) {
                            return true;
                        }
                }
            }
            // Direct annotation child (some grammar versions)
            if check_annotation_node(&child, source, names) {
                return true;
            }
        }
    }

    // Strategy 2: Check preceding siblings (fallback)
    let mut sib = node.prev_named_sibling();
    while let Some(s) = sib {
        if check_annotation_node(&s, source, names) {
            return true;
        }
        let kind = s.kind();
        if kind != "marker_annotation"
            && kind != "annotation"
            && kind != "modifiers"
            && kind != "modifier"
        {
            break;
        }
        sib = s.prev_named_sibling();
    }

    false
}

/// Check if a single CST node is an annotation matching one of the target names.
pub fn check_annotation_node(node: &Node, source: &str, names: &[&str]) -> bool {
    let kind = node.kind();
    if (kind == "marker_annotation" || kind == "annotation")
        && let Ok(text) = node.utf8_text(source.as_bytes()) {
            let ann_name = text.trim_start_matches('@');
            for name in names {
                if ann_name == *name || ann_name.starts_with(&format!("{name}(")) {
                    return true;
                }
            }
        }
    false
}

/// Check for a C# attribute (`[Name]`) on a declaration node.
///
/// C# tree-sitter grammar nests attributes as child `attribute_list` nodes
/// of the declaration. We check both child nodes and preceding siblings.
pub fn has_preceding_attribute(node: &Node, source: &str, names: &[&str]) -> bool {
    // Strategy 1: Check child nodes (C# grammar nests attributes as children)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let kind = child.kind();
            if (kind == "attribute_list" || kind == "attribute")
                && let Ok(text) = child.utf8_text(source.as_bytes()) {
                    for name in names {
                        if text.contains(name) {
                            return true;
                        }
                    }
                }
        }
    }

    // Strategy 2: Check preceding siblings (fallback)
    let mut sib = node.prev_named_sibling();
    while let Some(s) = sib {
        let kind = s.kind();
        if (kind == "attribute_list" || kind == "attribute")
            && let Ok(text) = s.utf8_text(source.as_bytes()) {
                for name in names {
                    if text.contains(name) {
                        return true;
                    }
                }
            }
        if kind != "attribute_list" && kind != "attribute" && kind != "modifier" {
            break;
        }
        sib = s.prev_named_sibling();
    }
    false
}

/// Check for a Rust `#[name]` attribute_item preceding the function.
///
/// Rust tree-sitter grammar uses `attribute_item` nodes as siblings before
/// function_item nodes.
pub fn has_preceding_rust_attribute(node: &Node, source: &str, names: &[&str]) -> bool {
    let mut sib = node.prev_named_sibling();
    while let Some(s) = sib {
        if s.kind() == "attribute_item"
            && let Ok(text) = s.utf8_text(source.as_bytes()) {
                // text looks like `#[test]` or `#[tokio::test]`
                let inner = text.trim_start_matches("#[").trim_end_matches(']');
                for name in names {
                    if inner == *name || inner.starts_with(&format!("{name}(")) {
                        return true;
                    }
                }
            }
        // Attribute items can be stacked — keep walking
        if s.kind() != "attribute_item" && s.kind() != "line_comment" {
            break;
        }
        sib = s.prev_named_sibling();
    }
    false
}

// ---------------------------------------------------------------------------
// Doc comment helpers
// ---------------------------------------------------------------------------

/// Find a comment node immediately preceding the given node.
pub fn find_preceding_comment<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    // Named sibling first
    if let Some(sib) = node.prev_named_sibling() {
        let kind = sib.kind();
        if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
            return Some(sib);
        }
    }
    // Walk unnamed siblings (comments are unnamed in some grammars like Java)
    let mut sib = node.prev_sibling();
    while let Some(s) = sib {
        let kind = s.kind();
        if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
            return Some(s);
        }
        if !s.is_named() {
            sib = s.prev_sibling();
            continue;
        }
        break;
    }
    None
}

/// C-family languages: look for `/** ... */`, `///`, or `//` comments preceding the node.
pub fn extract_block_or_line_comment(node: &Node, source: &str) -> Option<String> {
    let sibling = find_preceding_comment(node)?;
    let kind = sibling.kind();
    if kind != "comment" && kind != "line_comment" && kind != "block_comment" {
        return None;
    }
    let text = sibling.utf8_text(source.as_bytes()).ok()?;

    // JSDoc/JavaDoc style: /** ... */
    if text.starts_with("/**") {
        return Some(clean_block_comment(text));
    }
    // Triple-slash style: ///
    if text.starts_with("///") {
        let mut comments = vec![text.trim_start_matches("///").trim().to_string()];
        let mut sib = sibling.prev_named_sibling();
        while let Some(s) = sib {
            if s.kind() == "comment" || s.kind() == "line_comment" {
                let t = match s.utf8_text(source.as_bytes()) {
                    Ok(t) => t,
                    Err(_) => break,
                };
                if t.starts_with("///") {
                    comments.push(t.trim_start_matches("///").trim().to_string());
                    sib = s.prev_named_sibling();
                    continue;
                }
            }
            break;
        }
        comments.reverse();
        return Some(comments.join("\n"));
    }
    // Go-style: single-line // comments
    if text.starts_with("//") {
        let mut comments = vec![text.trim_start_matches("//").trim().to_string()];
        let mut sib = sibling.prev_named_sibling();
        while let Some(s) = sib {
            if s.kind() == "comment" {
                let t = match s.utf8_text(source.as_bytes()) {
                    Ok(t) => t,
                    Err(_) => break,
                };
                if t.starts_with("//") {
                    comments.push(t.trim_start_matches("//").trim().to_string());
                    sib = s.prev_named_sibling();
                    continue;
                }
            }
            break;
        }
        comments.reverse();
        return Some(comments.join("\n"));
    }

    None
}

/// Python: extract docstring from the first expression_statement in the body.
pub fn extract_python_docstring(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32)
            && child.kind() == "block"
                && let Some(first_stmt) = child.named_child(0)
                    && first_stmt.kind() == "expression_statement"
                        && let Some(str_node) = first_stmt.named_child(0)
                            && (str_node.kind() == "string"
                                || str_node.kind() == "concatenated_string")
                            {
                                let text = str_node.utf8_text(source.as_bytes()).ok()?;
                                return Some(clean_python_docstring(text));
                            }
    }
    None
}

/// Rust: look for `///` line comments preceding the node.
pub fn extract_rust_doc_comment(node: &Node, source: &str) -> Option<String> {
    let mut comments = Vec::new();
    let mut sibling = find_preceding_comment(node);
    while let Some(sib) = sibling {
        let kind = sib.kind();
        if kind == "line_comment" || kind == "comment" {
            let text = match sib.utf8_text(source.as_bytes()) {
                Ok(t) => t,
                Err(_) => break,
            };
            if text.starts_with("///") {
                comments.push(text.trim_start_matches("///").trim().to_string());
                sibling = find_preceding_comment(&sib);
                continue;
            }
        }
        break;
    }
    if comments.is_empty() {
        return None;
    }
    comments.reverse();
    Some(comments.join("\n"))
}

/// Ruby: look for `#` comments preceding the node.
pub fn extract_hash_comment(node: &Node, source: &str) -> Option<String> {
    let mut comments = Vec::new();
    let mut sibling = find_preceding_comment(node);
    while let Some(sib) = sibling {
        if sib.kind() == "comment" {
            let text = match sib.utf8_text(source.as_bytes()) {
                Ok(t) => t,
                Err(_) => break,
            };
            if text.starts_with('#') {
                comments.push(text.trim_start_matches('#').trim().to_string());
                sibling = find_preceding_comment(&sib);
                continue;
            }
        }
        break;
    }
    if comments.is_empty() {
        return None;
    }
    comments.reverse();
    Some(comments.join("\n"))
}

/// Clean a `/** ... */` block comment.
pub fn clean_block_comment(text: &str) -> String {
    let trimmed = text.trim_start_matches("/**").trim_end_matches("*/").trim();
    trimmed
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Clean a Python docstring (`"""..."""` or `'''...'''`).
pub fn clean_python_docstring(text: &str) -> String {
    let inner = text
        .trim_start_matches("\"\"\"")
        .trim_start_matches("'''")
        .trim_end_matches("\"\"\"")
        .trim_end_matches("'''")
        .trim();
    inner
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
