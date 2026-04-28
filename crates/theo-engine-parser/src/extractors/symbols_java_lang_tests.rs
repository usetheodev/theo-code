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
fn java_class_with_methods() {
    let symbols = symbols_for(
        r#"
public class OrderService {
    public Order createOrder(OrderRequest req) {
        return new Order();
    }

    public void cancelOrder(String id) {
    }
}
"#,
        SupportedLanguage::Java,
        "OrderService.java",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "OrderService" && s.kind == SymbolKind::Class)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "createOrder" && s.kind == SymbolKind::Method)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "cancelOrder" && s.kind == SymbolKind::Method)
    );
    assert_eq!(symbols.len(), 3);
}

#[test]
fn java_interface_and_enum() {
    let symbols = symbols_for(
        r#"
public interface PaymentGateway {
    void charge(Amount amount);
}

public enum PaymentStatus {
    PENDING,
    COMPLETED,
    FAILED
}
"#,
        SupportedLanguage::Java,
        "Payment.java",
    );

    assert!(
        symbols
            .iter()
            .any(|s| s.name == "PaymentGateway" && s.kind == SymbolKind::Interface)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "PaymentStatus" && s.kind == SymbolKind::Enum)
    );
}

// --- Go ---

#[test]
fn java_method_signature() {
    let symbols = symbols_for(
        r#"
public class Svc {
    public List<String> findAll(int limit) {
        return null;
    }
}
"#,
        SupportedLanguage::Java,
        "Svc.java",
    );
    let method = symbols.iter().find(|s| s.name == "findAll").unwrap();
    let sig = method.signature.as_deref().unwrap();
    assert!(sig.contains("findAll"), "should contain method name");
    assert!(sig.contains("int limit"), "should contain params");
}

#[test]
fn java_visibility_modifiers() {
    let symbols = symbols_for(
        r#"
public class Svc {
    public void doPublic() {}
    private void doPrivate() {}
    protected void doProtected() {}
}
"#,
        SupportedLanguage::Java,
        "Svc.java",
    );
    let svc = symbols.iter().find(|s| s.name == "Svc").unwrap();
    assert_eq!(svc.visibility, Some(Visibility::Public));
    let pub_m = symbols.iter().find(|s| s.name == "doPublic").unwrap();
    assert_eq!(pub_m.visibility, Some(Visibility::Public));
    let priv_m = symbols.iter().find(|s| s.name == "doPrivate").unwrap();
    assert_eq!(priv_m.visibility, Some(Visibility::Private));
    let prot_m = symbols.iter().find(|s| s.name == "doProtected").unwrap();
    assert_eq!(prot_m.visibility, Some(Visibility::Protected));
}

#[test]
fn java_method_parent_is_class() {
    let symbols = symbols_for(
        r#"
public class OrderService {
    public void process() {}
}
"#,
        SupportedLanguage::Java,
        "Order.java",
    );
    let method = symbols.iter().find(|s| s.name == "process").unwrap();
    assert_eq!(method.parent.as_deref(), Some("OrderService"));
}

#[test]
fn java_javadoc_comment_extracted() {
    let symbols = symbols_for(
        r#"
public class Svc {
    /** Process the order. */
    public void process() {}
}
"#,
        SupportedLanguage::Java,
        "Svc.java",
    );
    let sym = symbols.iter().find(|s| s.name == "process").unwrap();
    assert!(sym.doc.is_some(), "should extract JavaDoc");
    assert!(sym.doc.as_deref().unwrap().contains("Process the order"));
}

#[test]
fn java_test_annotation_detected() {
    let symbols = symbols_for(
        r#"
public class UserTest {
    @Test
    void shouldCreateUser() {
    }
}
"#,
        SupportedLanguage::Java,
        "UserTest.java",
    );
    let sym = symbols
        .iter()
        .find(|s| s.name == "shouldCreateUser")
        .unwrap();
    assert!(sym.is_test, "@Test annotation should mark method as test");
}

#[test]
fn java_regular_method_not_test() {
    let symbols = symbols_for(
        r#"
public class UserService {
    public void createUser() {
    }
}
"#,
        SupportedLanguage::Java,
        "UserService.java",
    );
    let sym = symbols.iter().find(|s| s.name == "createUser").unwrap();
    assert!(!sym.is_test, "regular Java method should not be test");
}

