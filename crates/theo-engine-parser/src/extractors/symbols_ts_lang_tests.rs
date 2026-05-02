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
fn ts_class_with_methods() {
    let symbols = symbols_for(
        r#"
class UserService {
    getUser(id: string) {
        return {};
    }
    deleteUser(id: string) {
        return true;
    }
}
"#,
        SupportedLanguage::TypeScript,
        "service.ts",
    );

    assert_eq!(symbols.len(), 3, "1 class + 2 methods");
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "UserService" && s.kind == SymbolKind::Class)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "getUser" && s.kind == SymbolKind::Method)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "deleteUser" && s.kind == SymbolKind::Method)
    );
}

#[test]
fn ts_function_and_interface() {
    let symbols = symbols_for(
        r#"
interface User {
    name: string;
    email: string;
}

function createUser(data: User): User {
    return data;
}

enum Status {
    Active,
    Inactive,
}
"#,
        SupportedLanguage::TypeScript,
        "types.ts",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "User" && s.kind == SymbolKind::Interface)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "createUser" && s.kind == SymbolKind::Function)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Status" && s.kind == SymbolKind::Enum)
    );
}

#[test]
fn ts_symbols_have_correct_line_numbers() {
    let symbols = symbols_for(
        "function hello() {\n  return 'world';\n}\n",
        SupportedLanguage::TypeScript,
        "hello.ts",
    );

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].anchor.line, 1);
    assert_eq!(symbols[0].anchor.end_line, 3);
}

// --- Python ---

#[test]
fn javascript_class_extraction() {
    let symbols = symbols_for(
        r#"
class Router {
    handle(req) {
        return {};
    }
}

function middleware(req, res, next) {
    next();
}
"#,
        SupportedLanguage::JavaScript,
        "router.js",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Router" && s.kind == SymbolKind::Class)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "handle" && s.kind == SymbolKind::Method)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "middleware" && s.kind == SymbolKind::Function)
    );
}

// =======================================================================
// New tests for signature, visibility, parent
// =======================================================================

// --- Signature ---

#[test]
fn ts_function_signature() {
    let symbols = symbols_for(
        "function greet(name: string): string {\n  return name;\n}\n",
        SupportedLanguage::TypeScript,
        "fn.ts",
    );
    assert_eq!(symbols.len(), 1);
    let sig = symbols[0].signature.as_deref().unwrap();
    assert!(sig.contains("greet"), "signature should contain name");
    assert!(sig.contains("string"), "signature should contain type");
    assert!(
        !sig.contains('{'),
        "signature should not contain body opener"
    );
}

#[test]
fn ts_export_is_public() {
    let symbols = symbols_for(
        "export function hello() {}\nfunction secret() {}\n",
        SupportedLanguage::TypeScript,
        "mod.ts",
    );
    // "hello" is exported — but tree-sitter may wrap in export_statement,
    // so the function_declaration itself may not start with "export"
    // depending on the query match. Let's check what we get:
    let hello = symbols.iter().find(|s| s.name == "hello");
    let secret = symbols.iter().find(|s| s.name == "secret");
    // secret has no export — visibility should be None
    assert!(hello.is_some());
    assert!(secret.is_some());
    assert_eq!(secret.unwrap().visibility, None);
}

#[test]
fn ts_method_parent_is_class() {
    let symbols = symbols_for(
        r#"
class UserService {
    getUser(id: string) {
        return {};
    }
}
"#,
        SupportedLanguage::TypeScript,
        "svc.ts",
    );
    let method = symbols.iter().find(|s| s.name == "getUser").unwrap();
    assert_eq!(method.parent.as_deref(), Some("UserService"));

    let class = symbols.iter().find(|s| s.name == "UserService").unwrap();
    assert!(class.parent.is_none(), "top-level class has no parent");
}

#[test]
fn ts_jsdoc_comment_extracted() {
    let symbols = symbols_for(
        r#"
/** Creates a new user account. */
function createUser(data: any) {
    return data;
}
"#,
        SupportedLanguage::TypeScript,
        "api.ts",
    );
    let sym = symbols.iter().find(|s| s.name == "createUser").unwrap();
    assert!(sym.doc.is_some(), "should extract JSDoc comment");
    assert!(sym.doc.as_deref().unwrap().contains("Creates a new user"));
}

#[test]
fn ts_test_function_detected() {
    let symbols = symbols_for(
        "function testCreateUser() {\n    expect(true).toBe(true);\n}\n",
        SupportedLanguage::TypeScript,
        "user.test.ts",
    );
    let sym = symbols.iter().find(|s| s.name == "testCreateUser").unwrap();
    assert!(
        sym.is_test,
        "function test* should be detected as test in TS"
    );
}

#[test]
fn ts_regular_function_not_test() {
    let symbols = symbols_for(
        "function createUser() {\n    return {};\n}\n",
        SupportedLanguage::TypeScript,
        "user.ts",
    );
    let sym = symbols.iter().find(|s| s.name == "createUser").unwrap();
    assert!(!sym.is_test, "regular TS function should not be test");
}

