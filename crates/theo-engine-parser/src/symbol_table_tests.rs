//! Sibling test body of `symbol_table.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `symbol_table.rs` via `#[path = "symbol_table_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use super::*;
    use crate::types::SourceAnchor;

    fn make_symbol(name: &str, file: &str, line: usize, kind: SymbolKind) -> Symbol {
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

    fn make_file_symbols() -> HashMap<PathBuf, Vec<Symbol>> {
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
