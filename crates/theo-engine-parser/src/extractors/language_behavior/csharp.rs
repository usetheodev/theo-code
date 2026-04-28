//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;
use super::dispatch::*;
use super::trait_def::*;
use super::constants::*;

impl LanguageBehavior for CSharpBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src", "Controllers", "Services"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        // C# uses `modifier` child nodes (includes `internal`)
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32)
                && child.kind() == "modifier" {
                    let mod_text = match child.utf8_text(source.as_bytes()) {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    if let Some(vis) = parse_visibility_keyword(mod_text) {
                        return Some(vis);
                    }
                }
        }
        // Fallback: check first word of node text
        let text = node.utf8_text(source.as_bytes()).ok()?;
        let first_word = text.split_whitespace().next()?;
        parse_visibility_keyword(first_word)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["invocation_expression"]
    }

    /// C#: check for `[Test]`, `[Fact]`, or `[Theory]` attributes.
    fn is_test_symbol(&self, node: &Node, source: &str, _symbol_name: &str) -> bool {
        has_preceding_attribute(node, source, &["Test", "Fact", "Theory"])
    }
}

/// Behavior for Go.
pub struct GoBehavior;

