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
fn py_class_with_methods() {
    let symbols = symbols_for(
        r#"
class UserService:
    def __init__(self):
        self.users = []

    def get_user(self, user_id):
        return None

    def create_user(self, data):
        pass
"#,
        SupportedLanguage::Python,
        "service.py",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "UserService" && s.kind == SymbolKind::Class)
    );
    // Python functions inside a class are still function_definition nodes
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "__init__" && s.kind == SymbolKind::Function)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "get_user" && s.kind == SymbolKind::Function)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "create_user" && s.kind == SymbolKind::Function)
    );
    assert_eq!(symbols.len(), 4, "1 class + 3 functions");
}

// --- Java ---

#[test]
fn python_function_signature() {
    let symbols = symbols_for(
        "def process(data, timeout=30):\n    pass\n",
        SupportedLanguage::Python,
        "proc.py",
    );
    assert_eq!(symbols.len(), 1);
    let sig = symbols[0].signature.as_deref().unwrap();
    assert!(sig.contains("process"), "should contain name");
    assert!(sig.contains("data"), "should contain param");
    assert!(!sig.contains(':'), "should not contain body colon");
}

#[test]
fn python_underscore_visibility() {
    let symbols = symbols_for(
        "def public_fn():\n    pass\n\ndef _private_fn():\n    pass\n\ndef __mangled():\n    pass\n",
        SupportedLanguage::Python,
        "mod.py",
    );
    let public = symbols.iter().find(|s| s.name == "public_fn").unwrap();
    let private = symbols.iter().find(|s| s.name == "_private_fn").unwrap();
    let mangled = symbols.iter().find(|s| s.name == "__mangled").unwrap();

    assert_eq!(public.visibility, Some(Visibility::Public));
    assert_eq!(private.visibility, Some(Visibility::Private));
    assert_eq!(mangled.visibility, Some(Visibility::Private));
}

#[test]
fn python_method_parent_is_class() {
    let symbols = symbols_for(
        "class MyClass:\n    def my_method(self):\n        pass\n",
        SupportedLanguage::Python,
        "cls.py",
    );
    let method = symbols.iter().find(|s| s.name == "my_method").unwrap();
    assert_eq!(method.parent.as_deref(), Some("MyClass"));
}

#[test]
fn python_docstring_extracted() {
    let symbols = symbols_for(
        r#"
def process(data):
    """Process the incoming data."""
    return data
"#,
        SupportedLanguage::Python,
        "proc.py",
    );
    let sym = symbols.iter().find(|s| s.name == "process").unwrap();
    assert!(sym.doc.is_some(), "should extract docstring");
    assert!(
        sym.doc
            .as_deref()
            .unwrap()
            .contains("Process the incoming data")
    );
}

#[test]
fn python_test_function_detected() {
    let symbols = symbols_for(
        "def test_create_user():\n    pass\n",
        SupportedLanguage::Python,
        "test_users.py",
    );
    let sym = symbols
        .iter()
        .find(|s| s.name == "test_create_user")
        .unwrap();
    assert!(sym.is_test, "def test_* should be detected as test");
}

#[test]
fn python_regular_function_not_test() {
    let symbols = symbols_for(
        "def create_user():\n    pass\n",
        SupportedLanguage::Python,
        "users.py",
    );
    let sym = symbols.iter().find(|s| s.name == "create_user").unwrap();
    assert!(!sym.is_test, "regular function should not be test");
}

