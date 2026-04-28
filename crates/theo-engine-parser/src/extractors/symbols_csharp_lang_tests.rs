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
fn csharp_class_with_methods() {
    let symbols = symbols_for(
        r#"
public class UsersController : ControllerBase {
    public IActionResult GetAll() {
        return Ok();
    }

    public IActionResult Create(UserDto dto) {
        return Created();
    }
}
"#,
        SupportedLanguage::CSharp,
        "UsersController.cs",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "UsersController" && s.kind == SymbolKind::Class)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "GetAll" && s.kind == SymbolKind::Method)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "Create" && s.kind == SymbolKind::Method)
    );
}

#[test]
fn extracts_csharp_record_declaration() {
    let symbols = symbols_for(
        r#"
public record UserDto(string Name, int Age);

public record OrderRecord {
    public string OrderId { get; init; }
    public decimal Total { get; init; }
}
"#,
        SupportedLanguage::CSharp,
        "Dtos.cs",
    );

    // record declarations are mapped to SymbolKind::Class
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "UserDto" && s.kind == SymbolKind::Class),
        "positional record should be extracted as class"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "OrderRecord" && s.kind == SymbolKind::Class),
        "nominal record should be extracted as class"
    );
}

// --- Rust ---

#[test]
fn csharp_method_parent_is_class() {
    let symbols = symbols_for(
        r#"
public class ItemsController {
    public void Delete() {}
}
"#,
        SupportedLanguage::CSharp,
        "Items.cs",
    );
    let method = symbols.iter().find(|s| s.name == "Delete").unwrap();
    assert_eq!(method.parent.as_deref(), Some("ItemsController"));
}

// =======================================================================
// is_test detection
// =======================================================================

#[test]
fn csharp_fact_attribute_detected() {
    let symbols = symbols_for(
        r#"
public class UserTests {
    [Fact]
    public void ShouldCreateUser() {
    }
}
"#,
        SupportedLanguage::CSharp,
        "UserTests.cs",
    );
    let sym = symbols
        .iter()
        .find(|s| s.name == "ShouldCreateUser")
        .unwrap();
    assert!(sym.is_test, "[Fact] attribute should mark method as test");
}

