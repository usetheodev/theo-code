//! Code-aware compression — Tree-Sitter-based context compression.
//!
//! Research: LongCodeZip (2024) achieves 5.6x compression without quality loss
//! using AST-aware strategies. This module implements a simplified version:
//! - Relevant symbols: full body preserved
//! - Irrelevant symbols: compressed to signature only
//! - Imports: collapsed to a single summary line
//!
//! The compression operates on raw source text + symbol metadata, producing
//! a compressed version that preserves semantic meaning for LLM consumption.

use std::collections::HashSet;

use crate::types::{Symbol, SymbolKind};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of compressing a source file for context.
#[derive(Debug, Clone)]
pub struct CompressedSource {
    /// The compressed source text.
    pub text: String,
    /// Original token count (estimated as chars/4).
    pub original_tokens: usize,
    /// Compressed token count.
    pub compressed_tokens: usize,
    /// Number of symbols kept in full.
    pub symbols_kept_full: usize,
    /// Number of symbols compressed to signature.
    pub symbols_compressed: usize,
}

impl CompressedSource {
    /// Compression ratio (0.0 = fully compressed, 1.0 = no compression).
    pub fn ratio(&self) -> f64 {
        if self.original_tokens == 0 {
            return 1.0;
        }
        self.compressed_tokens as f64 / self.original_tokens as f64
    }
}

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

/// Compress source code for LLM context, keeping relevant symbols in full
/// and compressing others to signature-only.
///
/// # Arguments
/// - `source`: raw source text
/// - `symbols`: extracted symbols with anchors (from parser)
/// - `relevant_symbols`: set of symbol names that should be kept in full
/// - `file_path`: for the header line
///
/// # Returns
/// Compressed source with metrics.
pub fn compress_for_context(
    source: &str,
    symbols: &[Symbol],
    relevant_symbols: &HashSet<String>,
    file_path: &str,
) -> CompressedSource {
    let original_tokens = source.len().div_ceil(4);

    if symbols.is_empty() {
        // No symbols extracted — return source as-is with header.
        let text = format!("// {file_path}\n{source}");
        let compressed_tokens = text.len().div_ceil(4);
        return CompressedSource {
            text,
            original_tokens,
            compressed_tokens,
            symbols_kept_full: 0,
            symbols_compressed: 0,
        };
    }

    let source_bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut symbols_kept_full = 0;
    let mut symbols_compressed = 0;

    // Header.
    output.push_str(&format!("// {file_path}\n"));

    // Sort symbols by start_byte to process in order.
    let mut sorted_symbols: Vec<&Symbol> = symbols.iter().collect();
    sorted_symbols.sort_by_key(|s| s.anchor.start_byte);

    // Deduplicate overlapping/nested symbols — keep outermost only.
    let top_level = deduplicate_nested(&sorted_symbols);

    // Collect import lines (heuristic: lines starting with "use ", "import ", "from ").
    let import_summary = collapse_imports(source);
    if !import_summary.is_empty() {
        output.push_str(&import_summary);
        output.push('\n');
    }

    // Process each top-level symbol.
    let mut last_end = 0;
    for sym in &top_level {
        let start = sym.anchor.start_byte;
        let end = sym.anchor.end_byte.min(source_bytes.len());

        if start < last_end {
            continue; // Overlapping, skip.
        }

        let is_relevant = relevant_symbols.contains(&sym.name)
            || relevant_symbols.iter().any(|r| {
                // Match "ClassName.method_name" style.
                sym.parent
                    .as_ref()
                    .is_some_and(|p| *r == format!("{}.{}", p, sym.name))
            });

        if is_relevant {
            // Keep full body.
            if let Ok(body) = std::str::from_utf8(&source_bytes[start..end]) {
                output.push_str(body);
                output.push('\n');
            }
            symbols_kept_full += 1;
        } else {
            // Compress to signature only.
            if let Some(ref sig) = sym.signature {
                output.push_str(sig);
                output.push_str(" { ... }\n");
            } else {
                // Fallback: use kind + name.
                let kind_str = kind_keyword(&sym.kind);
                output.push_str(&format!("{kind_str} {} {{ ... }}\n", sym.name));
            }
            symbols_compressed += 1;
        }

        last_end = end;
    }

    let compressed_tokens = output.len().div_ceil(4);

    CompressedSource {
        text: output,
        original_tokens,
        compressed_tokens,
        symbols_kept_full,
        symbols_compressed,
    }
}

/// Remove nested symbols, keeping only outermost definitions.
fn deduplicate_nested<'a>(sorted: &[&'a Symbol]) -> Vec<&'a Symbol> {
    let mut result: Vec<&Symbol> = Vec::new();
    let mut last_end: usize = 0;

    for sym in sorted {
        if sym.anchor.start_byte >= last_end {
            result.push(sym);
            last_end = sym.anchor.end_byte;
        }
        // else: nested inside the previous symbol, skip.
    }
    result
}

/// Collapse import lines into a summary.
fn collapse_imports(source: &str) -> String {
    let import_lines: Vec<&str> = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("use ")
                || trimmed.starts_with("import ")
                || trimmed.starts_with("from ")
                || trimmed.starts_with("require(")
                || trimmed.starts_with("const ") && trimmed.contains("require(")
        })
        .collect();

    if import_lines.is_empty() {
        return String::new();
    }

    if import_lines.len() <= 3 {
        // Few imports — keep them.
        return import_lines.join("\n");
    }

    // Many imports — collapse to count.
    format!("// [{} imports collapsed]", import_lines.len())
}

/// Map SymbolKind to a language-neutral keyword for compressed output.
fn kind_keyword(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "fn",
        SymbolKind::Method => "fn",
        SymbolKind::Class => "class",
        SymbolKind::Struct => "struct",
        SymbolKind::Trait => "trait",
        SymbolKind::Interface => "interface",
        SymbolKind::Enum => "enum",
        SymbolKind::Module => "mod",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SourceAnchor;
    use std::path::PathBuf;

    fn make_symbol(
        name: &str,
        kind: SymbolKind,
        signature: Option<&str>,
        start_byte: usize,
        end_byte: usize,
    ) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind,
            anchor: SourceAnchor {
                file: PathBuf::from("test.rs"),
                line: 1,
                end_line: 5,
                start_byte,
                end_byte,
                node_kind: String::new(),
            },
            doc: None,
            signature: signature.map(|s| s.to_string()),
            visibility: None,
            parent: None,
            is_test: false,
        }
    }

    #[test]
    fn compress_keeps_full_body_for_relevant_symbols() {
        let source = "fn foo() {\n    println!(\"hello\");\n}\n\nfn bar() {\n    println!(\"world\");\n}\n";
        let symbols = vec![
            make_symbol("foo", SymbolKind::Function, Some("fn foo()"), 0, 33),
            make_symbol("bar", SymbolKind::Function, Some("fn bar()"), 35, 68),
        ];
        let relevant: HashSet<String> = ["foo".to_string()].into();
        let result = compress_for_context(source, &symbols, &relevant, "test.rs");

        assert!(result.text.contains("println!(\"hello\")"), "foo body should be kept");
        assert!(!result.text.contains("println!(\"world\")"), "bar body should be compressed");
        assert!(result.text.contains("fn bar() { ... }"), "bar should be signature-only");
        assert_eq!(result.symbols_kept_full, 1);
        assert_eq!(result.symbols_compressed, 1);
    }

    #[test]
    fn compress_keeps_signature_removes_body_for_irrelevant() {
        let source = "fn irrelevant() {\n    very_long_body();\n    more_stuff();\n}\n";
        let symbols = vec![make_symbol(
            "irrelevant",
            SymbolKind::Function,
            Some("fn irrelevant()"),
            0,
            55,
        )];
        let relevant: HashSet<String> = HashSet::new();
        let result = compress_for_context(source, &symbols, &relevant, "test.rs");

        assert!(result.text.contains("fn irrelevant() { ... }"));
        assert!(!result.text.contains("very_long_body"));
        assert!(result.compressed_tokens < result.original_tokens);
    }

    #[test]
    fn compress_collapses_imports_to_list() {
        let source = "use std::io;\nuse std::fs;\nuse std::path::Path;\nuse std::collections::HashMap;\n\nfn main() {}\n";
        let symbols = vec![make_symbol(
            "main",
            SymbolKind::Function,
            Some("fn main()"),
            80,
            92,
        )];
        let relevant: HashSet<String> = HashSet::new();
        let result = compress_for_context(source, &symbols, &relevant, "test.rs");

        assert!(
            result.text.contains("[4 imports collapsed]"),
            "Should collapse 4 imports. Got: {}",
            result.text
        );
    }

    #[test]
    fn compress_no_symbols_returns_full_source() {
        let source = "// just a comment\nlet x = 42;\n";
        let symbols: Vec<Symbol> = vec![];
        let relevant: HashSet<String> = HashSet::new();
        let result = compress_for_context(source, &symbols, &relevant, "test.rs");

        assert!(result.text.contains("just a comment"));
        assert!(result.text.contains("let x = 42"));
        assert_eq!(result.symbols_kept_full, 0);
        assert_eq!(result.symbols_compressed, 0);
    }

    #[test]
    fn compress_ratio_decreases_with_more_irrelevant() {
        let source = "fn a() { long_body_1(); }\nfn b() { long_body_2(); }\nfn c() { long_body_3(); }\n";
        let symbols = vec![
            make_symbol("a", SymbolKind::Function, Some("fn a()"), 0, 25),
            make_symbol("b", SymbolKind::Function, Some("fn b()"), 26, 51),
            make_symbol("c", SymbolKind::Function, Some("fn c()"), 52, 77),
        ];

        // All relevant.
        let all_relevant: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let r1 = compress_for_context(source, &symbols, &all_relevant, "test.rs");

        // None relevant.
        let none_relevant: HashSet<String> = HashSet::new();
        let r2 = compress_for_context(source, &symbols, &none_relevant, "test.rs");

        assert!(
            r2.compressed_tokens < r1.compressed_tokens,
            "Compressing all should use fewer tokens ({}) than keeping all ({})",
            r2.compressed_tokens,
            r1.compressed_tokens
        );
    }

    #[test]
    fn compress_empty_source() {
        let source = "";
        let symbols: Vec<Symbol> = vec![];
        let relevant: HashSet<String> = HashSet::new();
        let result = compress_for_context(source, &symbols, &relevant, "empty.rs");
        assert!(result.text.contains("// empty.rs"));
    }

    #[test]
    fn compress_handles_nested_symbols() {
        // Class with a method inside — only the class should be in top-level.
        let source = "class Foo {\n  fn bar() { body(); }\n}\n";
        let symbols = vec![
            make_symbol("Foo", SymbolKind::Class, Some("class Foo"), 0, 36),
            make_symbol("bar", SymbolKind::Method, Some("fn bar()"), 14, 33),
        ];
        let relevant: HashSet<String> = ["Foo".to_string()].into();
        let result = compress_for_context(source, &symbols, &relevant, "test.rs");

        // Foo should be kept in full (includes bar as nested content).
        assert!(result.text.contains("body()"));
        // bar should NOT appear separately.
        assert_eq!(result.symbols_kept_full, 1);
        assert_eq!(result.symbols_compressed, 0); // bar is nested, not counted.
    }
}
