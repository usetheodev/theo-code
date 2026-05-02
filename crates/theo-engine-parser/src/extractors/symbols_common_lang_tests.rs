//! Per-language sibling test file extracted from extractors/symbols_tests.rs (T3.3).
//!
//! Test-only file; gates use the inner cfg(test) attribute below to
//! classify every line as test code.

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use std::path::PathBuf;

use crate::types::Visibility;
use super::symbols_test_helpers::symbols_for;

#[test]
fn empty_file_returns_no_symbols() {
    let symbols = symbols_for("", SupportedLanguage::TypeScript, "empty.ts");
    assert!(symbols.is_empty());
}

#[test]
fn unsupported_language_returns_no_symbols() {
    let symbols = symbols_for("let x = 1;", SupportedLanguage::Swift, "main.swift");
    assert!(symbols.is_empty());
}

#[test]
fn no_comment_gives_none_doc() {
    let symbols = symbols_for(
        "function bare() {}\n",
        SupportedLanguage::TypeScript,
        "bare.ts",
    );
    assert_eq!(symbols[0].doc, None);
}

