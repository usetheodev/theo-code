//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;
use super::dispatch::*;
use super::trait_def::*;
use super::constants::*;

impl LanguageBehavior for JavaBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src/main/java", "src"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        extract_visibility_modifier_child(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["method_invocation"]
    }

    /// Java/Kotlin: check for `@Test` annotation on the preceding sibling.
    fn is_test_symbol(&self, node: &Node, source: &str, _symbol_name: &str) -> bool {
        has_preceding_annotation(node, source, &["Test"])
    }
}

/// Behavior for C#.
pub struct CSharpBehavior;

