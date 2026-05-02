//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;
use super::dispatch::*;
use super::trait_def::*;
use super::constants::*;

impl LanguageBehavior for RubyBehavior {
    fn module_separator(&self) -> &'static str {
        "::"
    }

    fn source_roots(&self) -> &[&str] {
        &["app", "lib"]
    }

    fn signature_body_opener(&self) -> Option<char> {
        // Ruby: take the first line as the signature
        None
    }

    fn extract_doc_comment(&self, node: &Node, source: &str) -> Option<String> {
        extract_hash_comment(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call", "method_call"]
    }

    /// Ruby: `def test_*` naming convention.
    fn is_test_symbol(&self, _node: &Node, _source: &str, symbol_name: &str) -> bool {
        symbol_name.starts_with("test_")
    }
}

/// Behavior for Rust.
pub struct RustBehavior;

