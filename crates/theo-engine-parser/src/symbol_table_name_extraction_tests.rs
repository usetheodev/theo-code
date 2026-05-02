//! Sibling test body of `symbol_table.rs` — split per-area (T3.7 of code-hygiene-5x5).

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::symbol_table_test_helpers::*;
use super::*;
use crate::types::SourceAnchor;

#[test]
fn extract_bare_name_from_dot_qualified() {
    assert_eq!(extract_bare_name("user.save"), "save");
    assert_eq!(extract_bare_name("a.b.c"), "c");
}

#[test]
fn extract_bare_name_from_scope_qualified() {
    assert_eq!(extract_bare_name("Cls::method"), "method");
    assert_eq!(extract_bare_name("std::io::read"), "read");
}

#[test]
fn extract_bare_name_from_arrow_qualified() {
    assert_eq!(extract_bare_name("$obj->method"), "method");
}

#[test]
fn extract_bare_name_simple() {
    assert_eq!(extract_bare_name("validate"), "validate");
}

// --- Receiver extraction ---

#[test]
fn extract_receiver_from_dot_qualified() {
    assert_eq!(extract_receiver("np.array"), Some("np"));
    assert_eq!(extract_receiver("a.b.c"), Some("a"));
}

#[test]
fn extract_receiver_from_scope_qualified() {
    assert_eq!(extract_receiver("Cls::method"), Some("Cls"));
}

#[test]
fn extract_receiver_simple_name_returns_none() {
    assert_eq!(extract_receiver("validate"), None);
}

// --- Import index builder ---

