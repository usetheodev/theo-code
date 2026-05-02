//! Single-purpose slice extracted from `types.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tree_sitter::SupportedLanguage;

// ---------------------------------------------------------------------------

use super::*;
use super::misc::is_false;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Source location spanning the full symbol definition in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
    pub doc: Option<String>,
    /// Full signature text, e.g. `fn foo(x: i32) -> bool`.
    /// LLMs read these natively — structured params would add
    /// complexity for zero value.
    pub signature: Option<String>,
    /// Access modifier. `None` means the language default applies.
    pub visibility: Option<Visibility>,
    /// Enclosing class, module, trait, or impl block name.
    pub parent: Option<String>,
    /// Whether this symbol is a test function/method.
    ///
    /// Detected via language-specific patterns: naming conventions (`test_*` in
    /// Python, `Test*` in Go), annotations (`@Test` in Java), or attributes
    /// (`#[test]` in Rust, `[Fact]` in C#).
    ///
    /// **Limitation:** BDD-style `describe`/`it` blocks are call expressions,
    /// not function declarations — they are NOT marked as test symbols.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_test: bool,
}

/// Kind of code symbol.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Class,
    Function,
    Method,
    Module,
    Interface,
    Trait,
    Enum,
    Struct,
}

// ---------------------------------------------------------------------------
// Knowledge graph types
// ---------------------------------------------------------------------------

/// A reference between two symbols (call, extends, implements, etc.).
///
/// References form the edges of the knowledge graph, connecting symbols
/// across files and modules. The `source_symbol` is the origin (caller,
/// subclass) and `target_symbol` is the destination (callee, superclass).
///
/// Each reference carries a `confidence` score (0.0–1.0) and a
/// `resolution_method` indicating how the target was resolved.
/// Downstream consumers can filter low-confidence edges to reduce noise.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Reference {
    /// Enclosing symbol at the call/usage site (e.g., the function that
    /// contains a call expression). Empty string if at module level.
    pub source_symbol: String,
    /// File containing the reference site.
    pub source_file: PathBuf,
    /// 1-based line of the reference site.
    pub source_line: usize,
    /// Target symbol name (callee, parent type, imported name).
    pub target_symbol: String,
    /// File where the target is defined (`None` if external/unresolved).
    pub target_file: Option<PathBuf>,
    /// 1-based line of the target definition (`None` if unresolved).
    pub target_line: Option<usize>,
    /// What kind of relationship this reference represents.
    pub reference_kind: ReferenceKind,
    /// Confidence that this reference is correctly resolved (0.0–1.0).
    ///
    /// 0.0 = unresolved, 1.0 = certain. Import-based: 0.95, same-file: 0.90,
    /// global-unique: 0.80, global-ambiguous: 0.40.
    #[serde(default)]
    pub confidence: f64,
    /// How this reference's target was resolved.
    #[serde(default)]
    pub resolution_method: ResolutionMethod,
    /// Whether this reference crosses a test→production boundary.
    ///
    /// `true` when the source file has `FileRole::Test` and the target
    /// file does NOT have `FileRole::Test`. Enables downstream consumers
    /// to separate test coupling from production architecture.
    #[serde(default)]
    pub is_test_reference: bool,
}

/// Classification of a reference relationship.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// Function or method call (`foo()`, `obj.bar()`).
    Call,
    /// Class inheritance (`class Foo extends Bar`).
    Extends,
    /// Interface/trait implementation (`implements Baz`, `impl Trait for`).
    Implements,
    /// Type used as parameter, return type, or field type.
    TypeUsage,
    /// Import/require statement (`import { Foo } from './bar'`).
    Import,
}

/// A data model (class, struct, interface) with its fields.
///
/// Data models are the "nouns" of the system. Extracting them with
/// field-level detail lets LLMs understand data shapes without reading
/// the full source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataModel {
    /// Name of the type (e.g., `User`, `OrderItem`).
    pub name: String,
    /// What kind of data model this is.
    pub model_kind: DataModelKind,
    /// Fields/properties of the model.
    pub fields: Vec<FieldInfo>,
    /// Source location spanning the full type definition in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
    /// Parent type (extends/inherits from).
    pub parent_type: Option<String>,
    /// Implemented interfaces or traits.
    pub implemented_interfaces: Vec<String>,
}

/// Classification of a data model type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DataModelKind {
    Class,
    Struct,
    Interface,
    Trait,
    Enum,
    Record,
}

/// A single field within a data model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldInfo {
    /// Field name (e.g., `email`, `order_id`).
    pub name: String,
    /// Type annotation if present (e.g., `String`, `Option<i32>`).
    pub field_type: Option<String>,
    /// 1-based line number.
    pub line: usize,
    /// Access modifier if detected.
    pub visibility: Option<Visibility>,
}
