//! Sibling test body of `symbol_table.rs` — split per-area (T3.7 of code-hygiene-5x5).

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::symbol_table_test_helpers::*;
use super::*;
use crate::types::SourceAnchor;

#[test]
fn from_file_symbols_builds_both_levels() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    // Level 1: exact lookup
    let loc = table
        .resolve_in_file(Path::new("src/handler.ts"), "handleRequest")
        .unwrap();
    assert_eq!(loc.line, 5);
    assert_eq!(loc.kind, SymbolKind::Function);

    // Level 2: global lookup
    let globals = table.resolve_global("UserService");
    assert_eq!(globals.len(), 1);
    assert_eq!(globals[0].file, PathBuf::from("src/service.ts"));
}

#[test]
fn resolve_in_file_not_found() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    assert!(
        table
            .resolve_in_file(Path::new("src/handler.ts"), "nonexistent")
            .is_none()
    );
}

#[test]
fn resolve_global_unique() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let globals = table.resolve_global("UserService");
    assert_eq!(globals.len(), 1);
}

#[test]
fn resolve_global_multiple_matches() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    // "validate" exists in both handler.ts and utils/helpers.ts
    let globals = table.resolve_global("validate");
    assert_eq!(globals.len(), 2);
}

#[test]
fn resolve_global_not_found() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let globals = table.resolve_global("doesNotExist");
    assert!(globals.is_empty());
}

// --- Heuristic resolution chain ---

#[test]
fn resolve_via_import_based() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let mut imports = HashMap::new();
    imports.insert("UserService".to_string(), PathBuf::from("src/service.ts"));

    let result = table.resolve("UserService", Path::new("src/handler.ts"), &imports);

    assert!(result.location.is_some());
    assert_eq!(result.confidence, 0.95);
    assert_eq!(result.method, ResolutionMethod::ImportBased);
    assert_eq!(
        result.location.unwrap().file,
        PathBuf::from("src/service.ts")
    );
}

#[test]
fn resolve_via_same_file() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let imports = HashMap::new(); // no imports

    let result = table.resolve("validate", Path::new("src/handler.ts"), &imports);

    assert!(result.location.is_some());
    assert_eq!(result.confidence, 0.90);
    assert_eq!(result.method, ResolutionMethod::SameFile);
}

#[test]
fn resolve_via_global_unique() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let imports = HashMap::new();

    // "UserService" is globally unique and not in the source file
    let result = table.resolve("UserService", Path::new("src/utils/helpers.ts"), &imports);

    assert!(result.location.is_some());
    assert_eq!(result.confidence, 0.80);
    assert_eq!(result.method, ResolutionMethod::GlobalUnique);
}

#[test]
fn resolve_via_global_same_dir_prefers_same_directory() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let imports = HashMap::new();

    // "validate" is ambiguous (handler.ts + utils/helpers.ts)
    // Source file is in src/, so src/handler.ts should be preferred
    let result = table.resolve(
        "validate",
        Path::new("src/routes.ts"), // same dir as handler.ts
        &imports,
    );

    assert!(result.location.is_some());
    assert_eq!(result.method, ResolutionMethod::GlobalSameDir);
    assert_eq!(result.confidence, 0.60);
    assert_eq!(
        result.location.unwrap().file,
        PathBuf::from("src/handler.ts")
    );
}

#[test]
fn resolve_via_global_ambiguous_fallback() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let imports = HashMap::new();

    // "validate" is ambiguous, source file is in a different directory
    let result = table.resolve("validate", Path::new("other/dir/file.ts"), &imports);

    assert!(result.location.is_some());
    assert_eq!(result.method, ResolutionMethod::GlobalAmbiguous);
    assert_eq!(result.confidence, 0.40);
}

#[test]
fn resolve_unresolved() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let imports = HashMap::new();

    let result = table.resolve("nonexistent", Path::new("src/handler.ts"), &imports);

    assert!(result.location.is_none());
    assert_eq!(result.confidence, 0.0);
    assert_eq!(result.method, ResolutionMethod::Unresolved);
}

// --- Bare name extraction ---

#[test]
fn resolve_qualified_name_via_receiver_prefix() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let mut imports = HashMap::new();
    imports.insert("np".to_string(), PathBuf::from(EXTERNAL_SENTINEL));

    // "np.array" → receiver "np" is in imports as external
    let result =
        table.resolve_with_builtins("np.array", Path::new("src/handler.ts"), &imports, &|_| {
            false
        });

    assert!(result.location.is_none());
    assert_eq!(result.confidence, 0.70);
    assert_eq!(result.method, ResolutionMethod::ImportKnown);
}

// --- ImportKnown resolution ---

#[test]
fn resolve_via_import_known_for_external_specifier() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let mut imports = HashMap::new();
    imports.insert("Router".to_string(), PathBuf::from(EXTERNAL_SENTINEL));

    let result = table.resolve("Router", Path::new("src/handler.ts"), &imports);

    assert!(result.location.is_none());
    assert_eq!(result.confidence, 0.75);
    assert_eq!(result.method, ResolutionMethod::ImportKnown);
}

// --- Builtin resolution ---

#[test]
fn resolve_with_builtins_classifies_known_builtins_as_external() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);
    let imports = HashMap::new();

    let result =
        table.resolve_with_builtins("print", Path::new("src/handler.ts"), &imports, &|name| {
            name == "print" || name == "len"
        });

    assert!(result.location.is_none());
    assert_eq!(result.confidence, 0.65);
    assert_eq!(result.method, ResolutionMethod::External);
}

#[test]
fn resolve_with_builtins_unknown_symbol_stays_unresolved() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);
    let imports = HashMap::new();

    let result = table.resolve_with_builtins(
        "unknown_func",
        Path::new("src/handler.ts"),
        &imports,
        &|name| name == "print",
    );

    assert!(result.location.is_none());
    assert_eq!(result.confidence, 0.0);
    assert_eq!(result.method, ResolutionMethod::Unresolved);
}

#[test]
fn resolve_without_builtins_defaults_to_unresolved() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);
    let imports = HashMap::new();

    // Using the non-builtin resolve() should never match builtins
    let result = table.resolve("print", Path::new("src/handler.ts"), &imports);

    assert!(result.location.is_none());
    assert_eq!(result.confidence, 0.0);
    assert_eq!(result.method, ResolutionMethod::Unresolved);
}

// --- Resolve with qualified names (integration test) ---

#[test]
fn resolve_strips_receiver_for_method_call() {
    let fs = make_file_symbols();
    let table = SymbolTable::from_file_symbols(&fs);

    let imports = HashMap::new();

    // "user.getUser" should strip to "getUser" and find it
    let result = table.resolve("user.getUser", Path::new("src/handler.ts"), &imports);

    assert!(result.location.is_some());
    assert_eq!(
        result.location.unwrap().file,
        PathBuf::from("src/service.ts")
    );
}

// --- Python intra-project package resolution ---

