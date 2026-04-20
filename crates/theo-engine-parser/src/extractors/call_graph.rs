//! Call graph extraction from tree-sitter CSTs.
//!
//! Walks the CST to find all function/method call sites and records
//! them as `Reference` edges. Each reference links the enclosing
//! function (source) to the callee (target). Targets are unresolved
//! at this stage — cross-file resolution happens in `import_resolver`.

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::tree_sitter::SupportedLanguage;
use crate::types::{Reference, ReferenceKind, ResolutionMethod, Symbol};

use super::common::{node_text, node_text_ref};
use super::language_behavior::behavior_for;

/// Extract all call-site references from a source file's CST.
///
/// Each call expression produces a `Reference` with:
/// - `source_symbol`: the enclosing function/method name (or `""` for module-level)
/// - `target_symbol`: the callee name (e.g., `validate`, `fmt.Println`, `this.save`)
/// - `reference_kind`: always `ReferenceKind::Call`
/// - `target_file`/`target_line`: `None` (resolved later by import resolver)
pub fn extract_call_sites(
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
    file_path: &Path,
    symbols: &[Symbol],
) -> Vec<Reference> {
    let behavior = behavior_for(language);
    let call_kinds = behavior.call_node_kinds();
    let mut references = Vec::new();
    let root = tree.root_node();
    walk_for_calls(
        &root,
        source,
        language,
        file_path,
        symbols,
        call_kinds,
        &mut references,
    );
    references
}

/// Recursive CST walk collecting call-site references.
fn walk_for_calls(
    node: &Node,
    source: &str,
    language: SupportedLanguage,
    file_path: &Path,
    _symbols: &[Symbol],
    call_kinds: &[&str],
    results: &mut Vec<Reference>,
) {
    if call_kinds.contains(&node.kind()) {
        if let Some(callee) = extract_callee_name(node, source, language) {
            let enclosing = find_enclosing_function(node, source, language);
            let line = node.start_position().row + 1;

            results.push(Reference {
                source_symbol: enclosing.unwrap_or_default(),
                source_file: file_path.to_path_buf(),
                source_line: line,
                target_symbol: callee,
                target_file: None,
                target_line: None,
                reference_kind: ReferenceKind::Call,
                confidence: 0.0,
                resolution_method: ResolutionMethod::Unresolved,
                is_test_reference: false,
            });
        }
    }

    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i as u32) {
            walk_for_calls(
                &child, source, language, file_path, _symbols, call_kinds, results,
            );
        }
    }
}

/// Extract the callee name from a call expression node.
///
/// Handles several CST patterns:
/// - Simple calls: `foo()` → `"foo"`
/// - Method calls: `obj.method()` → `"obj.method"`
/// - Scoped calls (PHP/C++): `Cls::method()` → `"Cls::method"`
/// - Chained calls: `a.b.c()` → `"a.b.c"`
fn extract_callee_name(node: &Node, source: &str, language: SupportedLanguage) -> Option<String> {
    match language {
        // PHP has distinct node kinds for different call patterns
        SupportedLanguage::Php => extract_callee_php(node, source),
        // Java/Kotlin method_invocation has specific children
        SupportedLanguage::Java | SupportedLanguage::Kotlin => extract_callee_java(node, source),
        // C# invocation_expression
        SupportedLanguage::CSharp => extract_callee_csharp(node, source),
        // Python/Ruby `call` node
        SupportedLanguage::Python | SupportedLanguage::Ruby => {
            extract_callee_python_ruby(node, source)
        }
        // JS/TS/Go/Rust/C/C++/Swift/Scala: call_expression
        _ => extract_callee_generic(node, source),
    }
}

/// Generic callee extraction for `call_expression` nodes.
///
/// The first named child is typically the function being called:
/// - `identifier` → simple call (`foo()`)
/// - `member_expression` / `selector_expression` → method call (`obj.foo()`)
/// - `field_expression` (Rust) → method call (`self.foo()`)
fn extract_callee_generic(node: &Node, source: &str) -> Option<String> {
    let func_node = node.named_child(0)?;
    let text = node_text_ref(&func_node, source);
    // Filter out noise: constructors with `new`, raw string/number calls
    if text.is_empty() || text.starts_with('"') || text.starts_with('\'') {
        return None;
    }
    Some(text.to_string())
}

/// Java/Kotlin: `method_invocation` has `name` and optional `object` children.
fn extract_callee_java(node: &Node, source: &str) -> Option<String> {
    // method_invocation: [object.]name(arguments)
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, source);

    if let Some(obj_node) = node.child_by_field_name("object") {
        let obj = node_text(&obj_node, source);
        Some(format!("{}.{}", obj, name))
    } else {
        Some(name)
    }
}

/// C#: `invocation_expression` with first child as the callable.
fn extract_callee_csharp(node: &Node, source: &str) -> Option<String> {
    let func_node = node.named_child(0)?;
    let text = node_text_ref(&func_node, source);
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

/// Python/Ruby: `call` node with `function` field.
fn extract_callee_python_ruby(node: &Node, source: &str) -> Option<String> {
    // Python: call → function + arguments
    // Ruby: call → receiver.method or just method
    if let Some(func_node) = node.child_by_field_name("function") {
        let text = node_text_ref(&func_node, source);
        if text.is_empty() {
            return None;
        }
        return Some(text.to_string());
    }
    // Ruby `call` may have `method` field
    if let Some(method_node) = node.child_by_field_name("method") {
        let method = node_text_ref(&method_node, source);
        if let Some(recv_node) = node.child_by_field_name("receiver") {
            let recv = node_text_ref(&recv_node, source);
            return Some(format!("{}.{}", recv, method));
        }
        return Some(method.to_string());
    }
    // Fallback: first named child
    extract_callee_generic(node, source)
}

/// PHP: handles `scoped_call_expression`, `member_call_expression`,
/// and `function_call_expression` distinctly.
fn extract_callee_php(node: &Node, source: &str) -> Option<String> {
    match node.kind() {
        "scoped_call_expression" => {
            // Cls::method(args) → scope + name children
            let scope = node.child_by_field_name("scope")?;
            let name = node.child_by_field_name("name")?;
            Some(format!(
                "{}::{}",
                node_text(&scope, source),
                node_text(&name, source)
            ))
        }
        "member_call_expression" => {
            // $obj->method(args) → object + name children
            let name = node.child_by_field_name("name")?;
            if let Some(obj) = node.child_by_field_name("object") {
                Some(format!(
                    "{}->{}",
                    node_text(&obj, source),
                    node_text(&name, source)
                ))
            } else {
                Some(node_text(&name, source))
            }
        }
        "function_call_expression" => {
            // func(args) → function child
            let func_node = node
                .child_by_field_name("function")
                .or_else(|| node.named_child(0))?;
            Some(node_text(&func_node, source))
        }
        _ => extract_callee_generic(node, source),
    }
}

// ---------------------------------------------------------------------------
// Enclosing function resolution
// ---------------------------------------------------------------------------

/// CST node kinds that represent function/method definitions.
const FUNCTION_KINDS: &[&str] = &[
    // JS/TS
    "function_declaration",
    "method_definition",
    "arrow_function",
    // Python
    "function_definition",
    // Java/Kotlin/C#
    "method_declaration",
    "constructor_declaration",
    // Go
    "function_declaration",
    "method_declaration",
    // Rust
    "function_item",
    // PHP
    "function_definition",
    "method_declaration",
    // Ruby
    "method",
    "singleton_method",
    // C/C++
    "function_definition",
    // Swift
    "function_declaration",
    // Scala
    "function_definition",
];

/// Walk up the CST from a call node to find the enclosing function name.
fn find_enclosing_function(
    node: &Node,
    source: &str,
    language: SupportedLanguage,
) -> Option<String> {
    let mut current = node.parent()?;
    loop {
        let kind = current.kind();
        if FUNCTION_KINDS.contains(&kind) {
            return extract_function_name(&current, source, language);
        }
        current = current.parent()?;
    }
}

/// Extract the function/method name from a definition node.
fn extract_function_name(
    node: &Node,
    source: &str,
    _language: SupportedLanguage,
) -> Option<String> {
    // Most languages use a `name` field
    if let Some(name_node) = node.child_by_field_name("name") {
        return Some(node_text(&name_node, source));
    }
    // Fallback: look for an identifier child
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            if child.kind() == "identifier"
                || child.kind() == "property_identifier"
                || child.kind() == "type_identifier"
            {
                return Some(node_text(&child, source));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter::SupportedLanguage;
    use std::path::PathBuf;

    fn parse_and_extract(source: &str, language: SupportedLanguage) -> Vec<Reference> {
        let path = PathBuf::from("test_file");
        let parsed = crate::tree_sitter::parse_source(&path, source, language, None).unwrap();
        extract_call_sites(source, &parsed.tree, language, &path, &[])
    }

    #[test]
    fn typescript_simple_function_call() {
        let refs = parse_and_extract(
            "function main() { validate(input); }",
            SupportedLanguage::TypeScript,
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_symbol, "main");
        assert_eq!(refs[0].target_symbol, "validate");
        assert_eq!(refs[0].reference_kind, ReferenceKind::Call);
    }

    #[test]
    fn typescript_method_call() {
        let refs = parse_and_extract(
            "function handler() { user.save(); }",
            SupportedLanguage::TypeScript,
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_symbol, "handler");
        assert_eq!(refs[0].target_symbol, "user.save");
    }

    #[test]
    fn typescript_chained_method_call() {
        let refs = parse_and_extract(
            "function build() { app.use(cors()).listen(3000); }",
            SupportedLanguage::TypeScript,
        );
        // Should capture at least the outer calls
        assert!(!refs.is_empty());
        assert!(refs.iter().all(|r| r.source_symbol == "build"));
    }

    #[test]
    fn typescript_module_level_call() {
        let refs = parse_and_extract("const app = express();", SupportedLanguage::TypeScript);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_symbol, ""); // module level
        assert_eq!(refs[0].target_symbol, "express");
    }

    #[test]
    fn python_function_call() {
        let refs = parse_and_extract(
            "def process():\n    result = validate(data)\n    save(result)\n",
            SupportedLanguage::Python,
        );
        assert_eq!(refs.len(), 2);
        assert!(refs.iter().all(|r| r.source_symbol == "process"));
        let targets: Vec<&str> = refs.iter().map(|r| r.target_symbol.as_str()).collect();
        assert!(targets.contains(&"validate"));
        assert!(targets.contains(&"save"));
    }

    #[test]
    fn python_method_call() {
        let refs = parse_and_extract(
            "def handler():\n    db.query('SELECT 1')\n",
            SupportedLanguage::Python,
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target_symbol, "db.query");
    }

    #[test]
    fn java_method_invocation() {
        let source = r#"
public class Service {
    public void process() {
        validator.validate(input);
        save(result);
    }
}
"#;
        let refs = parse_and_extract(source, SupportedLanguage::Java);
        assert!(refs.len() >= 2);
        let targets: Vec<&str> = refs.iter().map(|r| r.target_symbol.as_str()).collect();
        assert!(targets.contains(&"validator.validate"));
        assert!(targets.contains(&"save"));
    }

    #[test]
    fn go_function_call() {
        let source = r#"
package main
func process() {
    result := compute(items)
    validate(result)
}
"#;
        let refs = parse_and_extract(source, SupportedLanguage::Go);
        assert!(refs.len() >= 2);
        assert!(refs.iter().all(|r| r.source_symbol == "process"));
    }

    #[test]
    fn rust_function_call() {
        let source = r#"
fn process(items: &[Item]) {
    let result = compute(items);
    validate(result);
}
"#;
        let refs = parse_and_extract(source, SupportedLanguage::Rust);
        assert!(refs.len() >= 2);
        assert!(refs.iter().all(|r| r.source_symbol == "process"));
    }

    #[test]
    fn csharp_invocation() {
        let source = r#"
public class Service {
    public void Process() {
        _validator.Validate(input);
        Save(result);
    }
}
"#;
        let refs = parse_and_extract(source, SupportedLanguage::CSharp);
        assert!(refs.len() >= 2);
    }

    #[test]
    fn multiple_functions_different_enclosing() {
        let source = r#"
function init() { setup(); }
function run() { execute(); cleanup(); }
"#;
        let refs = parse_and_extract(source, SupportedLanguage::TypeScript);
        assert_eq!(refs.len(), 3);

        let init_refs: Vec<_> = refs.iter().filter(|r| r.source_symbol == "init").collect();
        assert_eq!(init_refs.len(), 1);
        assert_eq!(init_refs[0].target_symbol, "setup");

        let run_refs: Vec<_> = refs.iter().filter(|r| r.source_symbol == "run").collect();
        assert_eq!(run_refs.len(), 2);
    }

    #[test]
    fn all_references_are_call_kind() {
        let refs = parse_and_extract(
            "function f() { a(); b(); c(); }",
            SupportedLanguage::TypeScript,
        );
        assert_eq!(refs.len(), 3);
        assert!(refs.iter().all(|r| r.reference_kind == ReferenceKind::Call));
    }

    #[test]
    fn target_file_and_line_are_none() {
        let refs = parse_and_extract("function f() { foo(); }", SupportedLanguage::TypeScript);
        assert_eq!(refs.len(), 1);
        assert!(refs[0].target_file.is_none());
        assert!(refs[0].target_line.is_none());
    }
}
