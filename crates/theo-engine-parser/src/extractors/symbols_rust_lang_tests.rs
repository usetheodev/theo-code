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
fn rust_struct_enum_trait_function() {
    let symbols = symbols_for(
        r#"
pub struct Config {
    pub port: u16,
}

pub enum AppError {
    NotFound,
    Internal(String),
}

pub trait Repository {
    fn find(&self, id: &str) -> Option<()>;
}

fn helper() -> bool {
    true
}
"#,
        SupportedLanguage::Rust,
        "lib.rs",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Config" && s.kind == SymbolKind::Struct)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "AppError" && s.kind == SymbolKind::Enum)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Repository" && s.kind == SymbolKind::Trait)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "helper" && s.kind == SymbolKind::Function)
    );
    assert_eq!(symbols.len(), 4);
}

// --- PHP ---

#[test]
fn rust_function_signature() {
    let symbols = symbols_for(
        "pub fn compute(x: i32, y: i32) -> f64 {\n    0.0\n}\n",
        SupportedLanguage::Rust,
        "lib.rs",
    );
    assert_eq!(symbols.len(), 1);
    let sig = symbols[0].signature.as_deref().unwrap();
    assert!(sig.contains("compute"));
    assert!(sig.contains("i32"));
    assert!(sig.contains("f64"));
}

#[test]
fn rust_pub_visibility() {
    let symbols = symbols_for(
        "pub fn public_fn() {}\nfn private_fn() {}\n",
        SupportedLanguage::Rust,
        "lib.rs",
    );
    let pub_fn = symbols.iter().find(|s| s.name == "public_fn").unwrap();
    let priv_fn = symbols.iter().find(|s| s.name == "private_fn").unwrap();
    assert_eq!(pub_fn.visibility, Some(Visibility::Public));
    assert_eq!(priv_fn.visibility, Some(Visibility::Private));
}

#[test]
fn rust_doc_comment_extracted() {
    let symbols = symbols_for(
        "/// Compute the result.\npub fn compute() {}\n",
        SupportedLanguage::Rust,
        "lib.rs",
    );
    let sym = symbols.iter().find(|s| s.name == "compute").unwrap();
    assert!(sym.doc.is_some(), "should extract /// doc comment");
    assert!(sym.doc.as_deref().unwrap().contains("Compute the result"));
}

#[test]
fn rust_test_attribute_detected() {
    let symbols = symbols_for(
        "#[test]\nfn test_it_works() {\n    assert!(true);\n}\n",
        SupportedLanguage::Rust,
        "lib.rs",
    );
    let sym = symbols.iter().find(|s| s.name == "test_it_works").unwrap();
    assert!(
        sym.is_test,
        "#[test] attribute should mark function as test"
    );
}

#[test]
fn rust_regular_function_not_test() {
    let symbols = symbols_for(
        "fn helper() -> bool {\n    true\n}\n",
        SupportedLanguage::Rust,
        "lib.rs",
    );
    let sym = symbols.iter().find(|s| s.name == "helper").unwrap();
    assert!(!sym.is_test, "regular Rust fn should not be test");
}

