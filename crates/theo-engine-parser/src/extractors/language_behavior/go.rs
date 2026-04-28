//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;
use super::dispatch::*;
use super::trait_def::*;
use super::constants::*;

impl LanguageBehavior for GoBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["cmd", "internal", "pkg"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        // Go uses capitalization convention
        let name = extract_name_from_node(node, source)?;
        let first_char = name.chars().next()?;
        if first_char.is_uppercase() {
            Some(Visibility::Public)
        } else {
            Some(Visibility::Private)
        }
    }

    fn find_parent_name(&self, node: &Node, source: &str) -> Option<String> {
        // Go methods have a receiver type — extract it from method_declaration
        if node.kind() == "method_declaration" {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32)
                    && child.kind() == "parameter_list" {
                        let text = child.utf8_text(source.as_bytes()).ok()?;
                        let cleaned = text.trim_matches(|c| c == '(' || c == ')');
                        let type_name = cleaned.split_whitespace().last()?.trim_start_matches('*');
                        return Some(type_name.to_string());
                    }
            }
        }
        find_parent_generic(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }

    /// Go: `func Test*(t *testing.T)` — name starts with `Test` and has
    /// a `testing.T` or `testing.B` or `testing.M` parameter.
    fn is_test_symbol(&self, node: &Node, source: &str, symbol_name: &str) -> bool {
        if !symbol_name.starts_with("Test")
            && !symbol_name.starts_with("Benchmark")
            && !symbol_name.starts_with("Fuzz")
        {
            return false;
        }
        // Verify the function signature contains testing.T/B/M/F
        let text = match node.utf8_text(source.as_bytes()) {
            Ok(t) => t,
            Err(_) => return false,
        };
        text.contains("testing.T")
            || text.contains("testing.B")
            || text.contains("testing.M")
            || text.contains("testing.F")
    }
}

/// Behavior for PHP.
pub struct PhpBehavior;

