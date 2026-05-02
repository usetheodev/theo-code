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
fn go_functions_and_methods() {
    let symbols = symbols_for(
        r#"
package main

func main() {
    fmt.Println("hello")
}

func (s *Server) Start(port int) error {
    return nil
}
"#,
        SupportedLanguage::Go,
        "main.go",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == SymbolKind::Function)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Start" && s.kind == SymbolKind::Method)
    );
}

// --- C# ---

#[test]
fn go_function_signature() {
    let symbols = symbols_for(
        "package main\n\nfunc Add(a int, b int) int {\n\treturn a + b\n}\n",
        SupportedLanguage::Go,
        "math.go",
    );
    let func = symbols.iter().find(|s| s.name == "Add").unwrap();
    let sig = func.signature.as_deref().unwrap();
    assert!(sig.contains("Add"));
    assert!(sig.contains("int"));
}

#[test]
fn go_capitalization_visibility() {
    let symbols = symbols_for(
        "package main\n\nfunc Exported() {}\nfunc internal() {}\n",
        SupportedLanguage::Go,
        "main.go",
    );
    let exported = symbols.iter().find(|s| s.name == "Exported").unwrap();
    let internal = symbols.iter().find(|s| s.name == "internal").unwrap();
    assert_eq!(exported.visibility, Some(Visibility::Public));
    assert_eq!(internal.visibility, Some(Visibility::Private));
}

#[test]
fn go_method_parent_is_receiver_type() {
    let symbols = symbols_for(
        "package main\n\nfunc (s *Server) Start() error {\n\treturn nil\n}\n",
        SupportedLanguage::Go,
        "server.go",
    );
    let method = symbols.iter().find(|s| s.name == "Start").unwrap();
    assert_eq!(method.parent.as_deref(), Some("Server"));
}

#[test]
fn go_struct_and_interface_declarations() {
    let symbols = symbols_for(
        r#"
package main

type Server struct {
    Port int
}

type Handler interface {
    Handle(req Request) Response
}
"#,
        SupportedLanguage::Go,
        "types.go",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Server" && s.kind == SymbolKind::Struct),
        "should detect struct declaration"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Handler" && s.kind == SymbolKind::Interface),
        "should detect interface declaration"
    );
}

// --- Doc comments ---

#[test]
fn go_comment_extracted() {
    let symbols = symbols_for(
        "package main\n\n// Add adds two numbers.\nfunc Add(a int, b int) int {\n\treturn a + b\n}\n",
        SupportedLanguage::Go,
        "math.go",
    );
    let sym = symbols.iter().find(|s| s.name == "Add").unwrap();
    assert!(sym.doc.is_some(), "should extract Go doc comment");
    assert!(sym.doc.as_deref().unwrap().contains("adds two numbers"));
}

#[test]
fn go_test_function_detected() {
    let symbols = symbols_for(
        "package main\n\nimport \"testing\"\n\nfunc TestCreateUser(t *testing.T) {\n}\n",
        SupportedLanguage::Go,
        "user_test.go",
    );
    let sym = symbols.iter().find(|s| s.name == "TestCreateUser").unwrap();
    assert!(
        sym.is_test,
        "func Test*(t *testing.T) should be detected as test"
    );
}

#[test]
fn go_regular_function_not_test() {
    let symbols = symbols_for(
        "package main\n\nfunc CreateUser() {\n}\n",
        SupportedLanguage::Go,
        "user.go",
    );
    let sym = symbols.iter().find(|s| s.name == "CreateUser").unwrap();
    assert!(!sym.is_test, "regular Go func should not be test");
}

