//! Symbol extraction via tree-sitter queries.
//!
//! Extracts code-level symbols (classes, functions, methods, interfaces,
//! traits, enums, structs, modules) from the CST using tree-sitter's
//! query API. Each supported language has a tailored S-expression query
//! that captures symbol definitions by name and span.
//!
//! Post-processing enriches each symbol with:
//! - **signature** — the declaration text up to the body opener
//! - **visibility** — access modifier (language-specific heuristics)
//! - **parent** — enclosing class/module/trait/impl name
//!
//! Languages without a dedicated query return an empty symbol list.
//! Query compilation errors (e.g., due to grammar version mismatches)
//! are handled gracefully — a warning is logged and empty symbols returned.

use std::path::Path;

use tree_sitter::StreamingIterator;
use tree_sitter::{Node, Tree};

use crate::tree_sitter::SupportedLanguage;
use crate::types::{Symbol, SymbolKind};

use super::common::anchor_from_node;
use super::language_behavior::behavior_for;

// ---------------------------------------------------------------------------
// Query patterns per language
// ---------------------------------------------------------------------------

/// TypeScript / TSX / JSX (all use the TypeScript grammar family).
const TS_SYMBOLS_QUERY: &str = r#"
(class_declaration name: (type_identifier) @name) @definition.class
(function_declaration name: (identifier) @name) @definition.function
(method_definition name: (property_identifier) @name) @definition.method
(interface_declaration name: (type_identifier) @name) @definition.interface
(enum_declaration name: (identifier) @name) @definition.enum
"#;

/// JavaScript (uses tree-sitter-javascript grammar — class names are `identifier`).
const JS_SYMBOLS_QUERY: &str = r#"
(class_declaration name: (identifier) @name) @definition.class
(function_declaration name: (identifier) @name) @definition.function
(method_definition name: (property_identifier) @name) @definition.method
"#;

/// Python — classes and functions (methods are function_definition inside a class body).
const PY_SYMBOLS_QUERY: &str = r#"
(class_definition name: (identifier) @name) @definition.class
(function_definition name: (identifier) @name) @definition.function
"#;

/// Java — classes, methods, interfaces, enums.
const JAVA_SYMBOLS_QUERY: &str = r#"
(class_declaration name: (identifier) @name) @definition.class
(method_declaration name: (identifier) @name) @definition.method
(interface_declaration name: (identifier) @name) @definition.interface
(enum_declaration name: (identifier) @name) @definition.enum
"#;

/// C# — classes, records, methods, interfaces, structs, enums.
const CS_SYMBOLS_QUERY: &str = r#"
(class_declaration name: (identifier) @name) @definition.class
(record_declaration name: (identifier) @name) @definition.class
(method_declaration name: (identifier) @name) @definition.method
(interface_declaration name: (identifier) @name) @definition.interface
(struct_declaration name: (identifier) @name) @definition.struct
(enum_declaration name: (identifier) @name) @definition.enum
"#;

/// Go — functions, methods, struct types, interface types.
const GO_SYMBOLS_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @definition.function
(method_declaration name: (field_identifier) @name) @definition.method
(type_declaration (type_spec name: (type_identifier) @name type: (struct_type))) @definition.struct
(type_declaration (type_spec name: (type_identifier) @name type: (interface_type))) @definition.interface
"#;

/// Rust — functions, structs, enums, traits, modules.
const RS_SYMBOLS_QUERY: &str = r#"
(function_item name: (identifier) @name) @definition.function
(struct_item name: (type_identifier) @name) @definition.struct
(enum_item name: (type_identifier) @name) @definition.enum
(trait_item name: (type_identifier) @name) @definition.trait
(mod_item name: (identifier) @name) @definition.module
"#;

/// PHP — classes, functions, methods, interfaces, traits, enums.
const PHP_SYMBOLS_QUERY: &str = r#"
(class_declaration name: (name) @name) @definition.class
(function_definition name: (name) @name) @definition.function
(method_declaration name: (name) @name) @definition.method
(interface_declaration name: (name) @name) @definition.interface
(trait_declaration name: (name) @name) @definition.trait
(enum_declaration name: (name) @name) @definition.enum
"#;

/// Ruby — classes, modules, methods.
const RB_SYMBOLS_QUERY: &str = r#"
(class name: (constant) @name) @definition.class
(module name: (constant) @name) @definition.module
(method name: (identifier) @name) @definition.method
"#;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract code symbols from a parsed source file.
///
/// Uses tree-sitter queries to find class, function, method, interface,
/// trait, enum, struct, and module definitions. Each symbol is enriched
/// with signature, visibility, and parent information via CST post-processing.
///
/// Returns an empty vector for languages without a dedicated query or
/// on query compilation failure.
pub fn extract_symbols(
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
    file_path: &Path,
) -> Vec<Symbol> {
    // Diagnostic: warn if tree-sitter produced parse errors (grammar may be outdated)
    if tree.root_node().has_error() {
        log::debug!(
            "lang={}, file={}: tree-sitter produced parse errors — grammar may be outdated for this language version",
            language,
            file_path.display()
        );
    }

    let query_source = match get_symbol_query(language) {
        Some(q) => q,
        None => return Vec::new(),
    };

    let ts_language = tree.language();
    let query = match tree_sitter::Query::new(&ts_language, query_source) {
        Ok(q) => q,
        Err(e) => {
            log::debug!(
                "lang={}, error={}: failed to compile symbol query, returning empty symbols",
                language,
                e
            );
            return Vec::new();
        }
    };

    let behavior = behavior_for(language);
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut symbols = Vec::new();

    let capture_names = query.capture_names();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    while let Some(m) = matches.next() {
        let mut sym_name: Option<String> = None;
        let mut sym_kind: Option<SymbolKind> = None;
        let mut def_node: Option<Node> = None;

        for capture in m.captures {
            let cap_name = &capture_names[capture.index as usize];

            if *cap_name == "name" {
                sym_name = capture
                    .node
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            } else if let Some(kind) = parse_kind_from_capture(cap_name) {
                sym_kind = Some(kind);
                def_node = Some(capture.node);
            }
        }

        if let (Some(name), Some(kind), Some(node)) = (sym_name, sym_kind, def_node) {
            let signature = behavior.extract_signature(&node, source);
            let visibility = behavior.parse_visibility(&node, source);
            let parent = behavior.find_parent_name(&node, source);
            let doc = behavior.extract_doc_comment(&node, source);
            let is_test = behavior.is_test_symbol(&node, source, &name);

            symbols.push(Symbol {
                name,
                kind,
                anchor: anchor_from_node(&node, file_path),
                doc,
                signature,
                visibility,
                parent,
                is_test,
            });
        }
    }

    // Diagnostic: warn when a non-trivial file yields zero symbols
    if symbols.is_empty() && source.lines().count() > 10 {
        log::debug!(
            "lang={}, file={}, lines={}: zero symbols extracted from non-trivial file — check grammar compatibility",
            language,
            file_path.display(),
            source.lines().count()
        );
    }

    symbols
}

// Signature extraction, visibility extraction, doc comment extraction, and
// parent name resolution are now delegated to LanguageBehavior trait
// implementations in `language_behavior.rs`. The query selection and kind
// parsing remain here as they are query-specific, not language-convention.

// ---------------------------------------------------------------------------
// Query selection
// ---------------------------------------------------------------------------

/// Select the query pattern for a given language.
fn get_symbol_query(language: SupportedLanguage) -> Option<&'static str> {
    match language {
        SupportedLanguage::TypeScript | SupportedLanguage::Tsx | SupportedLanguage::Jsx => {
            Some(TS_SYMBOLS_QUERY)
        }
        SupportedLanguage::JavaScript => Some(JS_SYMBOLS_QUERY),
        SupportedLanguage::Python => Some(PY_SYMBOLS_QUERY),
        SupportedLanguage::Java => Some(JAVA_SYMBOLS_QUERY),
        SupportedLanguage::CSharp => Some(CS_SYMBOLS_QUERY),
        SupportedLanguage::Go => Some(GO_SYMBOLS_QUERY),
        SupportedLanguage::Rust => Some(RS_SYMBOLS_QUERY),
        SupportedLanguage::Php => Some(PHP_SYMBOLS_QUERY),
        SupportedLanguage::Ruby => Some(RB_SYMBOLS_QUERY),
        // Kotlin, Scala, Swift, C, C++ — BASIC extraction only.
        // These languages use the generic extractor (file-level nodes, no symbol detail).
        // Graph quality for these is significantly lower than Tier 1 (TS/Py/Go/Java/Rust).
        // Honest coverage: these are "basic" not "full".
        _ => None, // Returns None → generic extractor used → file nodes only
    }
}

/// Parse a SymbolKind from a capture name like "definition.class".
fn parse_kind_from_capture(capture_name: &str) -> Option<SymbolKind> {
    let suffix = capture_name.strip_prefix("definition.")?;
    match suffix {
        "class" => Some(SymbolKind::Class),
        "function" => Some(SymbolKind::Function),
        "method" => Some(SymbolKind::Method),
        "interface" => Some(SymbolKind::Interface),
        "trait" => Some(SymbolKind::Trait),
        "enum" => Some(SymbolKind::Enum),
        "struct" => Some(SymbolKind::Struct),
        "module" => Some(SymbolKind::Module),
        _ => None,
    }
}


#[cfg(test)]
#[path = "symbols_tests.rs"]
mod tests;
