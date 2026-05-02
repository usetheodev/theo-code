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
fn ruby_class_module_methods() {
    let symbols = symbols_for(
        r#"
module Authentication
  class SessionManager
    def create_session(user)
      # ...
    end

    def destroy_session
      # ...
    end
  end
end
"#,
        SupportedLanguage::Ruby,
        "session.rb",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Authentication" && s.kind == SymbolKind::Module)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "SessionManager" && s.kind == SymbolKind::Class)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "create_session" && s.kind == SymbolKind::Method)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "destroy_session" && s.kind == SymbolKind::Method)
    );
}

// --- Edge cases ---

#[test]
fn ruby_method_signature() {
    let symbols = symbols_for(
        "class Foo\n  def bar(x, y)\n    x + y\n  end\nend\n",
        SupportedLanguage::Ruby,
        "foo.rb",
    );
    let method = symbols.iter().find(|s| s.name == "bar").unwrap();
    let sig = method.signature.as_deref().unwrap();
    assert!(sig.contains("bar"));
    assert!(sig.contains("x"));
}

// --- Visibility ---

#[test]
fn ruby_method_parent_is_class() {
    let symbols = symbols_for(
        "class Foo\n  def bar\n    # noop\n  end\nend\n",
        SupportedLanguage::Ruby,
        "foo.rb",
    );
    let method = symbols.iter().find(|s| s.name == "bar").unwrap();
    assert_eq!(method.parent.as_deref(), Some("Foo"));
}

#[test]
fn ruby_hash_comment_extracted() {
    let symbols = symbols_for(
        "# Greet the user.\ndef greet(name)\n  puts name\nend\n",
        SupportedLanguage::Ruby,
        "greet.rb",
    );
    let sym = symbols.iter().find(|s| s.name == "greet").unwrap();
    assert!(sym.doc.is_some(), "should extract Ruby comment");
    assert!(sym.doc.as_deref().unwrap().contains("Greet the user"));
}

#[test]
fn ruby_test_method_detected() {
    let symbols = symbols_for(
        "class UserTest\n  def test_create\n    # assert\n  end\nend\n",
        SupportedLanguage::Ruby,
        "user_test.rb",
    );
    let sym = symbols.iter().find(|s| s.name == "test_create").unwrap();
    assert!(sym.is_test, "def test_* should be detected as test in Ruby");
}

#[test]
fn ruby_regular_method_not_test() {
    let symbols = symbols_for(
        "class User\n  def create\n  end\nend\n",
        SupportedLanguage::Ruby,
        "user.rb",
    );
    let sym = symbols.iter().find(|s| s.name == "create").unwrap();
    assert!(!sym.is_test, "regular Ruby method should not be test");
}

