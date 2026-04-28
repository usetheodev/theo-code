//! Shared helper for extractors/*_lang_tests.rs sibling files (T3.3 split).
#![cfg(test)]
#![allow(unused_imports)]

use std::path::PathBuf;

use super::*;

pub(super) fn symbols_for(source: &str, lang: SupportedLanguage, filename: &str) -> Vec<Symbol> {
    let path = PathBuf::from(filename);
    let parsed = crate::tree_sitter::parse_source(&path, source, lang, None).unwrap();
    extract_symbols(source, &parsed.tree, lang, &path)
}
