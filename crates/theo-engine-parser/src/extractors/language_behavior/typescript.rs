//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;
use super::dispatch::*;
use super::trait_def::*;
use super::constants::*;

pub struct TypeScriptBehavior;

impl LanguageBehavior for TypeScriptBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src", "lib", "app"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        let text = node.utf8_text(source.as_bytes()).ok()?;
        if text.starts_with("export") {
            return Some(Visibility::Public);
        }
        if let Some(parent) = node.parent()
            && parent.kind() == "export_statement" {
                return Some(Visibility::Public);
            }
        None
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }

    /// TS/JS: functions named `test*` are considered test symbols.
    ///
    /// This is a name-prefix heuristic. BDD-style `describe`/`it` blocks
    /// are call_expressions, not function_declarations — not detected here.
    fn is_test_symbol(&self, _node: &Node, _source: &str, symbol_name: &str) -> bool {
        symbol_name.starts_with("test")
    }

    fn is_builtin_symbol(&self, name: &str) -> bool {
        JS_BUILTINS.binary_search(&name).is_ok()
    }
}
