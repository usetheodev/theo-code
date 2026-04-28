//! Single-purpose slice extracted from `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md, ADR D3).

#![allow(unused_imports, dead_code)]

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

use super::*;
use super::dispatch::*;
use super::trait_def::*;
use super::constants::*;

pub struct GenericBehavior;

impl LanguageBehavior for GenericBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src"]
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------
