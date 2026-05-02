//! Multi-language parser using tree-sitter.
//!
//! Parses source files into concrete syntax trees (CSTs) for downstream
//! semantic extraction. Supports 16 languages covering the most widely
//! used programming languages in production codebases.
//!
//! Thread-local parser caches avoid re-creating `tree_sitter::Parser`
//! instances for each file when processing many files of the same language
//! on the same rayon thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use crate::error::{ParserError, Result};

thread_local! {
    /// Per-thread cache of tree-sitter parsers keyed by language.
    ///
    /// Parsers hold mutable internal state and cannot be shared across threads,
    /// but within a single rayon thread they can be reused across files of the
    /// same language, avoiding repeated allocation and `set_language` calls.
    static PARSER_CACHE: RefCell<HashMap<SupportedLanguage, tree_sitter::Parser>> =
        RefCell::new(HashMap::new());
}

/// Languages supported by the Intently engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SupportedLanguage {
    #[serde(rename = "typescript")]
    TypeScript,
    #[serde(rename = "tsx")]
    Tsx,
    #[serde(rename = "javascript")]
    JavaScript,
    #[serde(rename = "jsx")]
    Jsx,
    #[serde(rename = "python")]
    Python,
    #[serde(rename = "java")]
    Java,
    #[serde(rename = "csharp")]
    CSharp,
    #[serde(rename = "go")]
    Go,
    #[serde(rename = "rust")]
    Rust,
    #[serde(rename = "php")]
    Php,
    #[serde(rename = "ruby")]
    Ruby,
    #[serde(rename = "kotlin")]
    Kotlin,
    #[serde(rename = "swift")]
    Swift,
    #[serde(rename = "c")]
    C,
    #[serde(rename = "cpp")]
    Cpp,
    #[serde(rename = "scala")]
    Scala,
}

impl SupportedLanguage {
    /// Returns the language family for extraction purposes.
    ///
    /// Languages in the same family share enough CST node structure
    /// to use the same extractor (e.g., JS/TS both use `call_expression`).
    pub fn family(self) -> LanguageFamily {
        match self {
            Self::TypeScript | Self::Tsx | Self::JavaScript | Self::Jsx => {
                LanguageFamily::JavaScriptLike
            }
            Self::Python => LanguageFamily::Python,
            Self::Java | Self::Kotlin | Self::Scala => LanguageFamily::JvmLike,
            Self::CSharp => LanguageFamily::CSharp,
            Self::Go => LanguageFamily::Go,
            Self::Rust => LanguageFamily::Rust,
            Self::Php => LanguageFamily::Php,
            Self::Ruby => LanguageFamily::Ruby,
            Self::Swift => LanguageFamily::Swift,
            Self::C | Self::Cpp => LanguageFamily::CLike,
        }
    }
}

/// Language families that share enough CST structure for extractor reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageFamily {
    JavaScriptLike,
    Python,
    JvmLike,
    CSharp,
    Go,
    Rust,
    Php,
    Ruby,
    Swift,
    CLike,
}

impl std::fmt::Display for SupportedLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TypeScript => write!(f, "typescript"),
            Self::Tsx => write!(f, "tsx"),
            Self::JavaScript => write!(f, "javascript"),
            Self::Jsx => write!(f, "jsx"),
            Self::Python => write!(f, "python"),
            Self::Java => write!(f, "java"),
            Self::CSharp => write!(f, "csharp"),
            Self::Go => write!(f, "go"),
            Self::Rust => write!(f, "rust"),
            Self::Php => write!(f, "php"),
            Self::Ruby => write!(f, "ruby"),
            Self::Kotlin => write!(f, "kotlin"),
            Self::Swift => write!(f, "swift"),
            Self::C => write!(f, "c"),
            Self::Cpp => write!(f, "cpp"),
            Self::Scala => write!(f, "scala"),
        }
    }
}

/// Result of parsing a source file with tree-sitter.
pub struct ParsedFile {
    pub language: SupportedLanguage,
    pub tree: tree_sitter::Tree,
}

/// Detect language from file extension.
///
/// Returns `None` for unsupported extensions. For ambiguous extensions
/// like `.h` (could be C or C++), defaults to C.
pub fn detect_language(path: &Path) -> Option<SupportedLanguage> {
    let ext = path.extension()?.to_str()?;
    match ext {
        // TypeScript
        "ts" | "mts" | "cts" => Some(SupportedLanguage::TypeScript),
        "tsx" => Some(SupportedLanguage::Tsx),
        // JavaScript
        "js" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),
        "jsx" => Some(SupportedLanguage::Jsx),
        // Python
        "py" | "pyw" | "pyi" => Some(SupportedLanguage::Python),
        // Java
        "java" => Some(SupportedLanguage::Java),
        // C#
        "cs" => Some(SupportedLanguage::CSharp),
        // Go
        "go" => Some(SupportedLanguage::Go),
        // Rust
        "rs" => Some(SupportedLanguage::Rust),
        // PHP
        "php" => Some(SupportedLanguage::Php),
        // Ruby
        "rb" => Some(SupportedLanguage::Ruby),
        // Kotlin
        "kt" | "kts" => Some(SupportedLanguage::Kotlin),
        // Swift
        "swift" => Some(SupportedLanguage::Swift),
        // C
        "c" | "h" => Some(SupportedLanguage::C),
        // C++
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(SupportedLanguage::Cpp),
        // Scala
        "scala" | "sc" => Some(SupportedLanguage::Scala),
        _ => None,
    }
}

/// Parse source code with tree-sitter, creating a fresh parser each time.
///
/// When `old_tree` is provided, tree-sitter performs incremental parsing —
/// reusing unchanged CST nodes and only re-parsing the edited region.
/// This reduces parse time from O(file_size) to O(edit_size).
///
/// For parallel workloads where many files of the same language are processed
/// on a single rayon thread, prefer [`parse_source_cached`] which reuses
/// thread-local parsers.
pub fn parse_source(
    path: &Path,
    source: &str,
    language: SupportedLanguage,
    old_tree: Option<&tree_sitter::Tree>,
) -> Result<ParsedFile> {
    let mut parser = tree_sitter::Parser::new();

    let ts_language = get_tree_sitter_language(language);

    parser
        .set_language(&ts_language)
        .map_err(|e| ParserError::ParseFailed {
            path: path.to_path_buf(),
            reason: format!("failed to set language {language}: {e}"),
        })?;

    let tree = parser
        .parse(source, old_tree)
        .ok_or_else(|| ParserError::ParseFailed {
            path: path.to_path_buf(),
            reason: "tree-sitter returned None (parse timeout or cancellation)".into(),
        })?;

    Ok(ParsedFile { language, tree })
}

/// Parse source code using a thread-local parser cache.
///
/// Reuses a cached `tree_sitter::Parser` for the given language within the
/// current thread. The language is set once per entry via `or_insert_with`,
/// so repeated calls for the same language avoid redundant `set_language` calls.
///
/// This is the preferred parsing path for `full_analysis` where many files
/// are processed in parallel via rayon — each rayon thread maintains its own
/// parser cache, eliminating per-file parser allocation overhead.
pub fn parse_source_cached(
    path: &Path,
    source: &str,
    language: SupportedLanguage,
    old_tree: Option<&tree_sitter::Tree>,
) -> Result<ParsedFile> {
    PARSER_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let parser = cache.entry(language).or_insert_with(|| {
            let mut p = tree_sitter::Parser::new();
            // Language will be set below on first use; we store a bare parser
            // here and set_language immediately after insertion.
            let _ = &mut p;
            p
        });

        let ts_language = get_tree_sitter_language(language);
        parser
            .set_language(&ts_language)
            .map_err(|e| ParserError::ParseFailed {
                path: path.to_path_buf(),
                reason: format!("failed to set language {language}: {e}"),
            })?;

        let tree = parser
            .parse(source, old_tree)
            .ok_or_else(|| ParserError::ParseFailed {
                path: path.to_path_buf(),
                reason: "tree-sitter returned None (parse timeout or cancellation)".into(),
            })?;

        Ok(ParsedFile { language, tree })
    })
}

/// Compute a tree-sitter `InputEdit` from the difference between old and new source.
///
/// Uses character-level diffing to find the first contiguous changed region.
/// Returns `None` if the sources are identical. The returned `InputEdit`
/// should be applied to the old tree via `tree.edit(&edit)` before passing
/// it to `parser.parse(new_source, Some(&edited_tree))`.
pub fn compute_input_edit(old_source: &str, new_source: &str) -> Option<tree_sitter::InputEdit> {
    if old_source == new_source {
        return None;
    }

    let old_bytes = old_source.as_bytes();
    let new_bytes = new_source.as_bytes();

    // Find the first byte where old and new differ (common prefix).
    let start_byte = old_bytes
        .iter()
        .zip(new_bytes.iter())
        .position(|(a, b)| a != b)
        .unwrap_or(old_bytes.len().min(new_bytes.len()));

    // Find the last byte where old and new differ (common suffix),
    // without overlapping with the common prefix.
    let max_suffix = (old_bytes.len() - start_byte).min(new_bytes.len() - start_byte);
    let common_suffix_len = old_bytes
        .iter()
        .rev()
        .zip(new_bytes.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();

    let old_end_byte = old_bytes.len() - common_suffix_len;
    let new_end_byte = new_bytes.len() - common_suffix_len;

    let start_position = byte_offset_to_point(old_source, start_byte);
    let old_end_position = byte_offset_to_point(old_source, old_end_byte);
    let new_end_position = byte_offset_to_point(new_source, new_end_byte);

    Some(tree_sitter::InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position,
        old_end_position,
        new_end_position,
    })
}

/// Convert a byte offset in a string to a tree-sitter Point (row, column).
fn byte_offset_to_point(source: &str, byte_offset: usize) -> tree_sitter::Point {
    let prefix = &source[..byte_offset.min(source.len())];
    let row = prefix.matches('\n').count();
    let last_newline = prefix.rfind('\n').map(|pos| pos + 1).unwrap_or(0);
    let column = byte_offset - last_newline;
    tree_sitter::Point { row, column }
}

/// Map our language enum to the corresponding tree-sitter grammar.
pub(crate) fn get_tree_sitter_language(language: SupportedLanguage) -> tree_sitter::Language {
    match language {
        SupportedLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        SupportedLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        SupportedLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        SupportedLanguage::Jsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        SupportedLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        SupportedLanguage::Java => tree_sitter_java::LANGUAGE.into(),
        SupportedLanguage::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        SupportedLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        SupportedLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        SupportedLanguage::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        SupportedLanguage::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        SupportedLanguage::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
        SupportedLanguage::Swift => tree_sitter_swift::LANGUAGE.into(),
        SupportedLanguage::C => tree_sitter_c::LANGUAGE.into(),
        SupportedLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        SupportedLanguage::Scala => tree_sitter_scala::LANGUAGE.into(),
    }
}

#[cfg(test)]
#[path = "tree_sitter_tests.rs"]
mod tests;
