//! Data model extraction from tree-sitter CSTs.
//!
//! Extracts classes, structs, and interfaces along with their field
//! definitions from source code across multiple languages.  Each
//! language uses different CST node names for the same concepts,
//! so this module dispatches to language-specific walkers.
//!
//! Extracted information:
//! - Model name, kind (class/struct/interface), location
//! - Fields with name, type annotation, line, visibility
//! - Parent type (extends/inherits) when detectable
//! - Implemented interfaces when detectable

#![allow(unused_imports, dead_code)]

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::tree_sitter::SupportedLanguage;
use crate::types::{DataModel, DataModelKind, FieldInfo, Visibility};

use super::super::common::{anchor_from_node, node_text};

use super::typescript::*;
use super::python::*;
use super::java::*;
use super::csharp::*;
use super::go::*;
use super::rust::*;

/// Extract data models (classes, structs, interfaces with fields) from a CST.
///
/// Dispatches to language-specific extraction logic based on the
/// `SupportedLanguage`. Languages without dedicated support return
/// an empty vector.
pub fn extract_data_models(
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
    file_path: &Path,
) -> Vec<DataModel> {
    let root = tree.root_node();
    let mut models = Vec::new();

    collect_models(&root, source, language, file_path, &mut models);

    models
}

/// Recursively walk the CST collecting data models.
pub fn collect_models(
    node: &Node,
    source: &str,
    language: SupportedLanguage,
    file_path: &Path,
    models: &mut Vec<DataModel>,
) {
    match language {
        SupportedLanguage::TypeScript
        | SupportedLanguage::Tsx
        | SupportedLanguage::JavaScript
        | SupportedLanguage::Jsx => {
            try_extract_ts_model(node, source, file_path, models);
        }
        SupportedLanguage::Python => {
            try_extract_python_model(node, source, file_path, models);
        }
        SupportedLanguage::Java | SupportedLanguage::Kotlin => {
            try_extract_java_model(node, source, file_path, models);
        }
        SupportedLanguage::CSharp => {
            try_extract_csharp_model(node, source, file_path, models);
        }
        SupportedLanguage::Go => {
            try_extract_go_model(node, source, file_path, models);
        }
        SupportedLanguage::Rust => {
            try_extract_rust_model(node, source, file_path, models);
        }
        _ => {}
    }

    let count = node.child_count();
    for i in 0..count {
        if let Some(child) = node.child(i as u32) {
            collect_models(&child, source, language, file_path, models);
        }
    }
}
