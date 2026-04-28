//! Language-specific behavioral traits.
//!
//! Defines the [`LanguageBehavior`] trait that encapsulates language-specific
//! conventions: module separators, source directory roots, visibility parsing,
//! signature extraction, doc comment extraction, parent name resolution, and
//! call node identification.
//!
//! Each language family gets a unit struct implementing the trait. The factory
//! function [`behavior_for`] maps [`SupportedLanguage`] variants to the correct
//! behavior instance, enabling downstream consumers to work polymorphically
//! without language-specific dispatch logic.

use tree_sitter::Node;

use crate::types::Visibility;

use super::dispatch::*;

// ---------------------------------------------------------------------------
// Trait definition
// ---------------------------------------------------------------------------

/// Language-specific behavioral conventions.
///
/// Provides default implementations where a sensible cross-language default
/// exists. Language-specific structs override only the methods that differ
/// from the defaults.
pub trait LanguageBehavior: Send + Sync {
    /// The separator used between module/namespace segments.
    ///
    /// Examples: `"."` for JavaScript, `"::"` for Rust, `"\\"` for PHP.
    fn module_separator(&self) -> &'static str {
        "."
    }

    /// Common source directory roots for this language.
    ///
    /// Used by module inference to strip prefix paths. For example,
    /// Java projects typically place source in `src/main/java/`.
    fn source_roots(&self) -> &[&str] {
        &["src"]
    }

    /// Extract visibility from a tree-sitter CST node.
    ///
    /// Returns `None` when the language has no visibility concept for the
    /// given node or when the visibility cannot be determined.
    fn parse_visibility(&self, _node: &Node, _source: &str) -> Option<Visibility> {
        None
    }

    /// The character that opens a function/method body.
    ///
    /// Used by [`extract_signature`](LanguageBehavior::extract_signature) to
    /// truncate the declaration at the body boundary. Returns `None` for
    /// languages where signature extraction uses a different strategy
    /// (e.g., Ruby takes the first line).
    fn signature_body_opener(&self) -> Option<char> {
        Some('{')
    }

    /// Extract the declaration signature from a definition node.
    ///
    /// Default: truncates the node text at [`signature_body_opener`](LanguageBehavior::signature_body_opener).
    fn extract_signature(&self, node: &Node, source: &str) -> Option<String> {
        let node_text = node.utf8_text(source.as_bytes()).ok()?;

        let truncated = match self.signature_body_opener() {
            Some(opener) => truncate_at_char(node_text, opener),
            None => node_text.lines().next().map(|l| l.to_string()),
        };

        let sig = truncated.as_deref().unwrap_or(node_text).trim().to_string();

        if sig.is_empty() { None } else { Some(sig) }
    }

    /// Extract a doc comment above the given node.
    ///
    /// Default: looks for `/** ... */`, `///`, or `//` comment siblings
    /// preceding the node (C-family convention).
    fn extract_doc_comment(&self, node: &Node, source: &str) -> Option<String> {
        extract_block_or_line_comment(node, source)
    }

    /// Find the name of the enclosing class, module, trait, or impl block.
    ///
    /// Default: walks up the CST looking for enclosing type definition nodes.
    fn find_parent_name(&self, node: &Node, source: &str) -> Option<String> {
        find_parent_generic(node, source)
    }

    /// CST node kinds that represent function/method calls.
    ///
    /// Used by call graph extraction to identify call sites.
    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }

    /// Determine whether a symbol definition node represents a test function.
    ///
    /// Language-specific detection patterns include:
    /// - **Naming conventions:** `test_*` (Python, Ruby, PHP), `Test*` (Go)
    /// - **Annotations/attributes:** `@Test` (Java/Kotlin), `[Test]`/`[Fact]`/`[Theory]` (C#),
    ///   `#[test]` (Rust), `#[Test]` (PHP)
    /// - **File heuristic:** TS/JS functions named `test*` in test files
    ///
    /// Returns `false` by default (GenericBehavior and languages without test patterns).
    fn is_test_symbol(&self, _node: &Node, _source: &str, _symbol_name: &str) -> bool {
        false
    }

    /// Check if a module name belongs to this language's standard library.
    ///
    /// Used by downstream analysis to differentiate "known stdlib" external
    /// imports from "unknown third-party" external imports. The check uses
    /// the top-level module name (e.g., `os` from `os.path`, `collections`
    /// from `collections.abc`).
    ///
    /// Returns `false` by default — only languages with well-defined stdlib
    /// boundaries override this (currently Python).
    fn is_stdlib_module(&self, _module_name: &str) -> bool {
        false
    }

    /// Check if a symbol name is a language builtin (type, function, constant).
    ///
    /// Builtins are symbols available without any import statement — they exist
    /// in the language's global scope. When Phase 2 resolution fails to find a
    /// symbol and it matches a builtin, it can be classified as `External`
    /// instead of `Unresolved`, improving confidence scoring.
    ///
    /// Returns `false` by default — only languages with well-defined builtin
    /// sets override this (currently Python and TypeScript/JavaScript).
    fn is_builtin_symbol(&self, _name: &str) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Behavior implementations
