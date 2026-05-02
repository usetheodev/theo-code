//! Shared test fixtures for symbol_table_*_tests.rs sibling files (T3.7 split).
#![cfg(test)]
#![allow(unused_imports)]

use std::collections::HashMap;
use std::path::PathBuf;

use super::*;
use super::*;
use crate::types::SourceAnchor;

pub(super) fn make_symbol(name: &str, file: &str, line: usize, kind: SymbolKind) -> Symbol {
    Symbol {
        name: name.into(),
        kind,
        anchor: SourceAnchor::from_line_range(PathBuf::from(file), line, line + 10),
        doc: None,
        signature: None,
        visibility: None,
        parent: None,
        is_test: false,
    }
}

pub(super) fn make_file_symbols() -> HashMap<PathBuf, Vec<Symbol>> {
    let mut fs = HashMap::new();
    fs.insert(
        PathBuf::from("src/handler.ts"),
        vec![
            make_symbol("handleRequest", "src/handler.ts", 5, SymbolKind::Function),
            make_symbol("validate", "src/handler.ts", 20, SymbolKind::Function),
        ],
    );
    fs.insert(
        PathBuf::from("src/service.ts"),
        vec![
            make_symbol("UserService", "src/service.ts", 1, SymbolKind::Class),
            make_symbol("getUser", "src/service.ts", 10, SymbolKind::Method),
        ],
    );
    fs.insert(
        PathBuf::from("src/utils/helpers.ts"),
        vec![make_symbol(
            "validate",
            "src/utils/helpers.ts",
            1,
            SymbolKind::Function,
        )],
    );
    fs
}

// --- SymbolTable construction ---

