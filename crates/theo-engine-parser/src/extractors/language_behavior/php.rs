//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;
use super::dispatch::*;
use super::trait_def::*;
use super::constants::*;

impl LanguageBehavior for PhpBehavior {
    fn module_separator(&self) -> &'static str {
        "\\"
    }

    fn source_roots(&self) -> &[&str] {
        &["src", "app"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        extract_visibility_modifier_child(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &[
            "member_call_expression",
            "function_call_expression",
            "scoped_call_expression",
        ]
    }

    /// PHP: `function test*()` name prefix or `#[Test]` attribute.
    fn is_test_symbol(&self, node: &Node, source: &str, symbol_name: &str) -> bool {
        if symbol_name.starts_with("test") {
            return true;
        }
        has_preceding_attribute(node, source, &["Test"])
    }
}

/// Behavior for Ruby.
pub struct RubyBehavior;

