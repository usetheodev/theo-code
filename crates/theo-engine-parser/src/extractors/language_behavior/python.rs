//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;
use super::dispatch::*;
use super::trait_def::*;
use super::constants::*;

pub struct PythonBehavior;

impl LanguageBehavior for PythonBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src", "app"]
    }

    fn signature_body_opener(&self) -> Option<char> {
        Some(':')
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        // Python uses naming convention — extract name from the node.
        // Both `_private` (convention) and `__mangled` (name-mangling)
        // are treated as Private in our model.
        let name = extract_name_from_node(node, source)?;
        if name.starts_with('_') {
            Some(Visibility::Private)
        } else {
            Some(Visibility::Public)
        }
    }

    fn extract_doc_comment(&self, node: &Node, source: &str) -> Option<String> {
        extract_python_docstring(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call"]
    }

    /// Python: `def test_*` or methods inside `unittest.TestCase` subclasses.
    fn is_test_symbol(&self, _node: &Node, _source: &str, symbol_name: &str) -> bool {
        symbol_name.starts_with("test_") || symbol_name.starts_with("test")
    }

    /// Python 3.x standard library modules.
    ///
    /// Matches the top-level module name against the CPython 3.12 stdlib.
    /// For dotted imports like `os.path`, the caller should extract the
    /// first segment (`os`) before calling this method.
    fn is_stdlib_module(&self, module_name: &str) -> bool {
        PYTHON_STDLIB_MODULES.contains(&module_name)
    }

    fn is_builtin_symbol(&self, name: &str) -> bool {
        PYTHON_BUILTINS.binary_search(&name).is_ok()
    }
}

/// Behavior for Java and Kotlin.
pub struct JavaBehavior;

