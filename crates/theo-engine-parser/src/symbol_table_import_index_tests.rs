//! Sibling test body of `symbol_table.rs` — split per-area (T3.7 of code-hygiene-5x5).

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::symbol_table_test_helpers::*;
use super::*;
use crate::types::SourceAnchor;

#[test]
fn build_import_index_resolves_relative_imports() {
    let fs = make_file_symbols();
    let imports = vec![crate::types::ImportInfo {
        source: "./service".into(),
        specifiers: vec!["UserService".into()],
        line: 1,
        aliases: vec![],
    }];

    let index = build_import_index(
        Path::new("src/handler.ts"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    assert_eq!(
        index.get("UserService"),
        Some(&PathBuf::from("src/service.ts"))
    );
}

#[test]
fn build_import_index_registers_external_specifiers_with_sentinel() {
    let fs = make_file_symbols();
    let imports = vec![crate::types::ImportInfo {
        source: "express".into(),
        specifiers: vec!["Router".into(), "Request".into()],
        line: 1,
        aliases: vec![],
    }];

    let index = build_import_index(
        Path::new("src/handler.ts"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    assert_eq!(index.get("Router"), Some(&PathBuf::from(EXTERNAL_SENTINEL)));
    assert_eq!(
        index.get("Request"),
        Some(&PathBuf::from(EXTERNAL_SENTINEL))
    );
}

#[test]
fn build_import_index_external_default_import_uses_source_name() {
    let fs = make_file_symbols();
    let imports = vec![crate::types::ImportInfo {
        source: "express".into(),
        specifiers: vec![], // default import
        line: 1,
        aliases: vec![],
    }];

    let index = build_import_index(
        Path::new("src/handler.ts"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    assert_eq!(
        index.get("express"),
        Some(&PathBuf::from(EXTERNAL_SENTINEL))
    );
}

#[test]
fn build_import_index_relative_takes_precedence_over_external() {
    let fs = make_file_symbols();
    let imports = vec![
        // External import of "UserService"
        crate::types::ImportInfo {
            source: "some-package".into(),
            specifiers: vec!["UserService".into()],
            line: 1,
            aliases: vec![],
        },
        // Relative import that resolves to an actual file
        crate::types::ImportInfo {
            source: "./service".into(),
            specifiers: vec!["UserService".into()],
            line: 2,
            aliases: vec![],
        },
    ];

    let index = build_import_index(
        Path::new("src/handler.ts"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    // Relative import should win (more precise)
    assert_eq!(
        index.get("UserService"),
        Some(&PathBuf::from("src/service.ts"))
    );
}

#[test]
fn build_import_index_mixed_relative_and_external() {
    let fs = make_file_symbols();
    let imports = vec![
        crate::types::ImportInfo {
            source: "./service".into(),
            specifiers: vec!["UserService".into()],
            line: 1,
            aliases: vec![],
        },
        crate::types::ImportInfo {
            source: "lodash".into(),
            specifiers: vec!["debounce".into()],
            line: 2,
            aliases: vec![],
        },
    ];

    let index = build_import_index(
        Path::new("src/handler.ts"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    assert_eq!(
        index.get("UserService"),
        Some(&PathBuf::from("src/service.ts"))
    );
    assert_eq!(
        index.get("debounce"),
        Some(&PathBuf::from(EXTERNAL_SENTINEL))
    );
}

// --- Alias registration ---

#[test]
fn build_import_index_registers_aliases_for_external_imports() {
    let fs = make_file_symbols();
    let imports = vec![crate::types::ImportInfo {
        source: "numpy".into(),
        specifiers: vec!["numpy".into()],
        line: 1,
        aliases: vec![("np".into(), "numpy".into())],
    }];

    let index = build_import_index(
        Path::new("src/handler.ts"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    // Both the original name and the alias should be registered
    assert_eq!(index.get("numpy"), Some(&PathBuf::from(EXTERNAL_SENTINEL)));
    assert_eq!(index.get("np"), Some(&PathBuf::from(EXTERNAL_SENTINEL)));
}

#[test]
fn build_import_index_registers_aliases_for_relative_imports() {
    let fs = make_file_symbols();
    let imports = vec![crate::types::ImportInfo {
        source: "./service".into(),
        specifiers: vec!["UserService".into()],
        line: 1,
        aliases: vec![("US".into(), "UserService".into())],
    }];

    let index = build_import_index(
        Path::new("src/handler.ts"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    assert_eq!(
        index.get("UserService"),
        Some(&PathBuf::from("src/service.ts"))
    );
    assert_eq!(index.get("US"), Some(&PathBuf::from("src/service.ts")));
}

// --- Receiver-prefix resolution ---

#[test]
fn try_resolve_python_package_finds_init_py() {
    let mut fs = HashMap::new();
    fs.insert(
        PathBuf::from("torch/nn/__init__.py"),
        vec![make_symbol(
            "Module",
            "torch/nn/__init__.py",
            1,
            SymbolKind::Class,
        )],
    );

    let result = try_resolve_python_package("torch.nn", &fs, Path::new(""));
    assert_eq!(result, Some(PathBuf::from("torch/nn/__init__.py")));
}

#[test]
fn try_resolve_python_package_finds_module_py() {
    let mut fs = HashMap::new();
    fs.insert(
        PathBuf::from("torch/nn/conv.py"),
        vec![make_symbol(
            "Conv2d",
            "torch/nn/conv.py",
            1,
            SymbolKind::Class,
        )],
    );

    let result = try_resolve_python_package("torch.nn.conv", &fs, Path::new(""));
    assert_eq!(result, Some(PathBuf::from("torch/nn/conv.py")));
}

#[test]
fn try_resolve_python_package_prefers_init_over_module() {
    let mut fs = HashMap::new();
    // Both torch/nn/__init__.py and torch/nn.py exist — __init__.py wins
    fs.insert(
        PathBuf::from("torch/nn/__init__.py"),
        vec![make_symbol(
            "Module",
            "torch/nn/__init__.py",
            1,
            SymbolKind::Class,
        )],
    );
    fs.insert(
        PathBuf::from("torch/nn.py"),
        vec![make_symbol("Module", "torch/nn.py", 1, SymbolKind::Class)],
    );

    let result = try_resolve_python_package("torch.nn", &fs, Path::new(""));
    assert_eq!(result, Some(PathBuf::from("torch/nn/__init__.py")));
}

#[test]
fn try_resolve_python_package_returns_none_for_external() {
    let fs = HashMap::new(); // empty — no project files
    let result = try_resolve_python_package("numpy", &fs, Path::new(""));
    assert_eq!(result, None);
}

#[test]
fn try_resolve_python_package_finds_absolute_paths() {
    let mut fs = HashMap::new();
    fs.insert(
        PathBuf::from("/project/torch/nn/__init__.py"),
        vec![make_symbol(
            "Module",
            "/project/torch/nn/__init__.py",
            1,
            SymbolKind::Class,
        )],
    );

    let result = try_resolve_python_package("torch.nn", &fs, Path::new("/project"));
    assert_eq!(result, Some(PathBuf::from("/project/torch/nn/__init__.py")));
}

#[test]
fn build_import_index_resolves_python_intra_project_package() {
    let mut fs = HashMap::new();
    fs.insert(
        PathBuf::from("torch/nn/__init__.py"),
        vec![make_symbol(
            "Module",
            "torch/nn/__init__.py",
            1,
            SymbolKind::Class,
        )],
    );

    let imports = vec![crate::types::ImportInfo {
        source: "torch.nn".into(),
        specifiers: vec!["Module".into()],
        line: 1,
        aliases: vec![],
    }];

    let index = build_import_index(
        Path::new("myapp/main.py"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    // Should resolve to actual file, not external sentinel
    assert_eq!(
        index.get("Module"),
        Some(&PathBuf::from("torch/nn/__init__.py"))
    );
}

#[test]
fn build_import_index_python_external_when_not_in_project() {
    let fs = HashMap::new(); // no torch files in project

    let imports = vec![crate::types::ImportInfo {
        source: "torch.nn".into(),
        specifiers: vec!["Module".into()],
        line: 1,
        aliases: vec![],
    }];

    let index = build_import_index(
        Path::new("myapp/main.py"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    // Should fall through to external sentinel
    assert_eq!(index.get("Module"), Some(&PathBuf::from(EXTERNAL_SENTINEL)));
}

#[test]
fn build_import_index_non_python_file_skips_package_resolution() {
    let mut fs = HashMap::new();
    // Even though torch/nn/__init__.py exists, a .ts file shouldn't use Python resolution
    fs.insert(
        PathBuf::from("torch/nn/__init__.py"),
        vec![make_symbol(
            "Module",
            "torch/nn/__init__.py",
            1,
            SymbolKind::Class,
        )],
    );

    let imports = vec![crate::types::ImportInfo {
        source: "torch.nn".into(),
        specifiers: vec!["Module".into()],
        line: 1,
        aliases: vec![],
    }];

    let index = build_import_index(
        Path::new("myapp/main.ts"),
        &imports,
        &fs,
        Path::new(""),
        None,
    );

    // TypeScript file: should stay external
    assert_eq!(index.get("Module"), Some(&PathBuf::from(EXTERNAL_SENTINEL)));
}

#[test]
fn build_import_index_python_resolver_takes_priority_over_static() {
    let mut fs = HashMap::new();
    // Static resolution would point to __init__.py
    fs.insert(
        PathBuf::from("torch/nn/__init__.py"),
        vec![make_symbol(
            "Module",
            "torch/nn/__init__.py",
            1,
            SymbolKind::Class,
        )],
    );
    // But the actual defining file is modules/module.py
    fs.insert(
        PathBuf::from("torch/nn/modules/module.py"),
        vec![make_symbol(
            "Module",
            "torch/nn/modules/module.py",
            27,
            SymbolKind::Class,
        )],
    );

    let imports = vec![crate::types::ImportInfo {
        source: "torch.nn".into(),
        specifiers: vec!["Module".into()],
        line: 1,
        aliases: vec![],
    }];

    // Simulate Python runtime resolver output
    let mut python_resolved = HashMap::new();
    python_resolved.insert(
        "torch.nn.Module".to_string(),
        PathBuf::from("torch/nn/modules/module.py"),
    );

    let index = build_import_index(
        Path::new("myapp/main.py"),
        &imports,
        &fs,
        Path::new(""),
        Some(&python_resolved),
    );

    // Python resolver should win over static __init__.py resolution
    assert_eq!(
        index.get("Module"),
        Some(&PathBuf::from("torch/nn/modules/module.py"))
    );
}

#[test]
fn build_import_index_python_resolver_partial_coverage() {
    let mut fs = HashMap::new();
    fs.insert(
        PathBuf::from("torch/nn/__init__.py"),
        vec![make_symbol(
            "Module",
            "torch/nn/__init__.py",
            1,
            SymbolKind::Class,
        )],
    );

    let imports = vec![crate::types::ImportInfo {
        source: "torch.nn".into(),
        specifiers: vec!["Module".into(), "Conv2d".into()],
        line: 1,
        aliases: vec![],
    }];

    // Python resolver only knows about Module, not Conv2d
    let mut python_resolved = HashMap::new();
    python_resolved.insert(
        "torch.nn.Module".to_string(),
        PathBuf::from("torch/nn/modules/module.py"),
    );

    let index = build_import_index(
        Path::new("myapp/main.py"),
        &imports,
        &fs,
        Path::new(""),
        Some(&python_resolved),
    );

    // Module: resolved by Python resolver
    assert_eq!(
        index.get("Module"),
        Some(&PathBuf::from("torch/nn/modules/module.py"))
    );
    // Conv2d: falls to static resolution (torch/nn/__init__.py)
    assert_eq!(
        index.get("Conv2d"),
        Some(&PathBuf::from("torch/nn/__init__.py"))
    );
}

