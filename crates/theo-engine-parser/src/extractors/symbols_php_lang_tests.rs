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
fn php_class_with_methods() {
    let symbols = symbols_for(
        r#"<?php
class UserController {
    public function index() {
        return view('users.index');
    }

    public function store(Request $request) {
        return redirect('/users');
    }
}
?>"#,
        SupportedLanguage::Php,
        "UserController.php",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "UserController" && s.kind == SymbolKind::Class)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "index" && s.kind == SymbolKind::Method)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "store" && s.kind == SymbolKind::Method)
    );
}

// --- Ruby ---

#[test]
fn php_visibility_modifiers() {
    let symbols = symbols_for(
        r#"<?php
class Svc {
    public function doPublic() {}
    private function doPrivate() {}
}
?>"#,
        SupportedLanguage::Php,
        "Svc.php",
    );
    let pub_m = symbols.iter().find(|s| s.name == "doPublic").unwrap();
    let priv_m = symbols.iter().find(|s| s.name == "doPrivate").unwrap();
    assert_eq!(pub_m.visibility, Some(Visibility::Public));
    assert_eq!(priv_m.visibility, Some(Visibility::Private));
}

// --- Parent ---

#[test]
fn php_method_parent_is_class() {
    let symbols = symbols_for(
        "<?php\nclass Ctrl {\n    public function index() {}\n}\n?>",
        SupportedLanguage::Php,
        "ctrl.php",
    );
    let method = symbols.iter().find(|s| s.name == "index").unwrap();
    assert_eq!(method.parent.as_deref(), Some("Ctrl"));
}

// --- Go struct/interface declarations ---

#[test]
fn php_doc_comment_extracted() {
    let symbols = symbols_for(
        "<?php\nclass Svc {\n    /** Save data. */\n    public function save() {}\n}\n?>",
        SupportedLanguage::Php,
        "svc.php",
    );
    let sym = symbols.iter().find(|s| s.name == "save").unwrap();
    assert!(sym.doc.is_some(), "should extract PHP doc comment");
    assert!(sym.doc.as_deref().unwrap().contains("Save data"));
}

#[test]
fn php_test_method_detected() {
    let symbols = symbols_for(
        "<?php\nclass UserTest {\n    public function testCreate() {}\n}\n?>",
        SupportedLanguage::Php,
        "UserTest.php",
    );
    let sym = symbols.iter().find(|s| s.name == "testCreate").unwrap();
    assert!(
        sym.is_test,
        "function test* should be detected as test in PHP"
    );
}

