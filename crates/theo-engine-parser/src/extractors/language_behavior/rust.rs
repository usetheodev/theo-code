//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;
use super::dispatch::*;
use super::trait_def::*;
use super::constants::*;

impl LanguageBehavior for RustBehavior {
    fn module_separator(&self) -> &'static str {
        "::"
    }

    fn source_roots(&self) -> &[&str] {
        &["src"]
    }

    fn parse_visibility(&self, node: &Node, _source: &str) -> Option<Visibility> {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32)
                && child.kind() == "visibility_modifier" {
                    return Some(Visibility::Public);
                }
        }
        Some(Visibility::Private)
    }

    fn extract_doc_comment(&self, node: &Node, source: &str) -> Option<String> {
        extract_rust_doc_comment(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }

    /// Rust: `#[test]` or `#[tokio::test]` attribute on the function.
    fn is_test_symbol(&self, node: &Node, source: &str, _symbol_name: &str) -> bool {
        has_preceding_rust_attribute(node, source, &["test", "tokio::test", "rstest"])
    }
}
