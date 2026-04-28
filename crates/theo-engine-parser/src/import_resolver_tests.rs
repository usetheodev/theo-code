//! Sibling test body of `import_resolver.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `import_resolver.rs` via `#[path = "import_resolver_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use super::*;
    use crate::types::{SourceAnchor, SymbolKind};

    /// Helper: create a minimal Symbol for testing.
    fn make_symbol(name: &str, file: &str, line: usize) -> Symbol {
        Symbol {
            name: name.into(),
            kind: SymbolKind::Class,
            anchor: SourceAnchor::from_line_range(PathBuf::from(file), line, line + 10),
            doc: None,
            signature: None,
            visibility: None,
            parent: None,
            is_test: false,
        }
    }

    #[test]
    fn relative_import_resolves_to_correct_file() {
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/handler.ts"),
            vec![ImportInfo {
                source: "./service".into(),
                specifiers: vec!["UserService".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("src/service.ts"),
            vec![make_symbol("UserService", "src/service.ts", 5)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_symbol, "UserService");
        assert_eq!(r.target_file, Some(PathBuf::from("src/service.ts")));
        assert_eq!(r.target_line, Some(5));
        assert_eq!(r.reference_kind, ReferenceKind::Import);
        assert_eq!(r.source_file, PathBuf::from("src/handler.ts"));
        assert_eq!(r.source_line, 1);
        assert!(r.source_symbol.is_empty());
    }

    #[test]
    fn named_import_finds_matching_symbol_in_target_file() {
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/app.ts"),
            vec![ImportInfo {
                source: "./utils/auth".into(),
                specifiers: vec!["verifyToken".into()],
                line: 3,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("src/utils/auth.ts"),
            vec![
                make_symbol("verifyToken", "src/utils/auth.ts", 10),
                make_symbol("generateToken", "src/utils/auth.ts", 25),
            ],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_symbol, "verifyToken");
        assert_eq!(r.target_file, Some(PathBuf::from("src/utils/auth.ts")));
        assert_eq!(r.target_line, Some(10));
    }

    #[test]
    fn package_import_stays_unresolved() {
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/app.ts"),
            vec![ImportInfo {
                source: "express".into(),
                specifiers: vec!["express".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let file_symbols: HashMap<PathBuf, Vec<Symbol>> = HashMap::new();

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_symbol, "express");
        assert!(r.target_file.is_none());
        assert!(r.target_line.is_none());
        assert_eq!(r.reference_kind, ReferenceKind::Import);
    }

    #[test]
    fn import_with_multiple_specifiers_creates_multiple_references() {
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/handler.ts"),
            vec![ImportInfo {
                source: "./models".into(),
                specifiers: vec!["User".into(), "Order".into(), "Product".into()],
                line: 2,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("src/models.ts"),
            vec![
                make_symbol("User", "src/models.ts", 1),
                make_symbol("Order", "src/models.ts", 20),
                make_symbol("Product", "src/models.ts", 40),
            ],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 3);
        // All references point to the same source file and line.
        for r in &refs {
            assert_eq!(r.source_file, PathBuf::from("src/handler.ts"));
            assert_eq!(r.source_line, 2);
            assert_eq!(r.target_file, Some(PathBuf::from("src/models.ts")));
            assert_eq!(r.reference_kind, ReferenceKind::Import);
        }
        // Sorted by target_symbol.
        let target_symbols: Vec<&str> = refs.iter().map(|r| r.target_symbol.as_str()).collect();
        assert_eq!(target_symbols, vec!["Order", "Product", "User"]);
    }

    #[test]
    fn nonexistent_relative_import_creates_reference_with_target_file_none() {
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/app.ts"),
            vec![ImportInfo {
                source: "./does-not-exist".into(),
                specifiers: vec!["Foo".into()],
                line: 5,
                aliases: vec![],
            }],
        );

        let file_symbols: HashMap<PathBuf, Vec<Symbol>> = HashMap::new();

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_symbol, "Foo");
        assert!(r.target_file.is_none());
        assert!(r.target_line.is_none());
    }

    #[test]
    fn path_resolution_handles_multiple_extensions() {
        // The import says `./service` and the file is `src/service.js`
        // (not `.ts`). Extension resolution should still find it.
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/app.ts"),
            vec![ImportInfo {
                source: "./service".into(),
                specifiers: vec!["createApp".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("src/service.js"),
            vec![make_symbol("createApp", "src/service.js", 3)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_file, Some(PathBuf::from("src/service.js")));
        assert_eq!(r.target_line, Some(3));
    }

    #[test]
    fn empty_imports_produce_no_references() {
        let file_imports: HashMap<PathBuf, Vec<ImportInfo>> = HashMap::new();
        let file_symbols: HashMap<PathBuf, Vec<Symbol>> = HashMap::new();

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert!(refs.is_empty());
    }

    #[test]
    fn external_imports_without_dot_prefix_mark_as_external() {
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/main.ts"),
            vec![
                ImportInfo {
                    source: "lodash".into(),
                    specifiers: vec!["debounce".into()],
                    line: 1,
                    aliases: vec![],
                },
                ImportInfo {
                    source: "@nestjs/common".into(),
                    specifiers: vec!["Controller".into(), "Get".into()],
                    line: 2,
                    aliases: vec![],
                },
            ],
        );

        let file_symbols: HashMap<PathBuf, Vec<Symbol>> = HashMap::new();

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 3);
        for r in &refs {
            assert!(
                r.target_file.is_none(),
                "external import should have no target_file"
            );
            assert!(
                r.target_line.is_none(),
                "external import should have no target_line"
            );
        }

        let target_symbols: Vec<&str> = refs.iter().map(|r| r.target_symbol.as_str()).collect();
        assert!(target_symbols.contains(&"debounce"));
        assert!(target_symbols.contains(&"Controller"));
        assert!(target_symbols.contains(&"Get"));
    }

    // --- Internal helper tests ---

    #[test]
    fn normalize_path_resolves_dot_and_dotdot() {
        let path = Path::new("src/handlers/../utils/./auth");
        let normalized = normalize_path(path);
        assert_eq!(normalized, PathBuf::from("src/utils/auth"));
    }

    #[test]
    fn index_file_resolution_for_directory_imports() {
        // Import `./components` should resolve to `src/components/index.ts`
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/app.ts"),
            vec![ImportInfo {
                source: "./components".into(),
                specifiers: vec!["Button".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("src/components/index.ts"),
            vec![make_symbol("Button", "src/components/index.ts", 2)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(
            r.target_file,
            Some(PathBuf::from("src/components/index.ts"))
        );
        assert_eq!(r.target_line, Some(2));
    }

    #[test]
    fn parent_directory_relative_import_resolves() {
        // `src/handlers/user.ts` imports `../models/user`
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/handlers/user.ts"),
            vec![ImportInfo {
                source: "../models/user".into(),
                specifiers: vec!["UserModel".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("src/models/user.ts"),
            vec![make_symbol("UserModel", "src/models/user.ts", 8)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_file, Some(PathBuf::from("src/models/user.ts")));
        assert_eq!(r.target_line, Some(8));
    }

    // --- normalize_python_relative tests ---

    #[test]
    fn normalize_python_single_dot_becomes_current_dir() {
        assert_eq!(normalize_python_relative("."), Some("./".to_string()));
    }

    #[test]
    fn normalize_python_single_dot_with_module() {
        assert_eq!(
            normalize_python_relative(".views"),
            Some("./views".to_string())
        );
    }

    #[test]
    fn normalize_python_double_dot_becomes_parent_dir() {
        assert_eq!(normalize_python_relative(".."), Some("../".to_string()));
    }

    #[test]
    fn normalize_python_double_dot_with_module() {
        assert_eq!(
            normalize_python_relative("..utils"),
            Some("../utils".to_string())
        );
    }

    #[test]
    fn normalize_python_triple_dot_becomes_two_levels_up() {
        assert_eq!(normalize_python_relative("..."), Some("../../".to_string()));
    }

    #[test]
    fn normalize_python_triple_dot_with_module() {
        assert_eq!(
            normalize_python_relative("...core"),
            Some("../../core".to_string())
        );
    }

    #[test]
    fn normalize_python_already_js_style_returns_none() {
        assert_eq!(normalize_python_relative("./foo"), None);
        assert_eq!(normalize_python_relative("../bar"), None);
    }

    #[test]
    fn normalize_python_absolute_import_returns_none() {
        assert_eq!(normalize_python_relative("os"), None);
        assert_eq!(normalize_python_relative("fastapi"), None);
        assert_eq!(normalize_python_relative("torch.nn"), None);
    }

    #[test]
    fn python_relative_import_resolves_through_normalization() {
        // Python `from .service import UserService` produces source=".service"
        // The normalizer should convert it to "./service" for the resolver.
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/handler.py"),
            vec![ImportInfo {
                source: ".service".into(),
                specifiers: vec!["UserService".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("src/service.py"),
            vec![make_symbol("UserService", "src/service.py", 5)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_symbol, "UserService");
        assert_eq!(r.target_file, Some(PathBuf::from("src/service.py")));
        assert_eq!(r.target_line, Some(5));
        assert_eq!(r.confidence, 0.95);
        assert_eq!(r.resolution_method, ResolutionMethod::ImportBased);
    }

    #[test]
    fn python_double_dot_import_resolves_to_parent_package() {
        // Python `from ..models import User` produces source="..models"
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/handlers/user.py"),
            vec![ImportInfo {
                source: "..models".into(),
                specifiers: vec!["User".into()],
                line: 3,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("src/models.py"),
            vec![make_symbol("User", "src/models.py", 10)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_symbol, "User");
        assert_eq!(r.target_file, Some(PathBuf::from("src/models.py")));
        assert_eq!(r.target_line, Some(10));
        assert_eq!(r.resolution_method, ResolutionMethod::ImportBased);
    }

    #[test]
    fn specifier_not_found_in_resolved_file_has_none_target_line() {
        // The file resolves, but the specific symbol is not in it.
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("src/app.ts"),
            vec![ImportInfo {
                source: "./service".into(),
                specifiers: vec!["NonexistentSymbol".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("src/service.ts"),
            vec![make_symbol("ActualSymbol", "src/service.ts", 5)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        // File resolves, but symbol does not exist in it.
        assert_eq!(r.target_file, Some(PathBuf::from("src/service.ts")));
        assert!(r.target_line.is_none());
    }

    // --- Python intra-project package import tests ---

    #[test]
    fn python_absolute_import_resolves_to_intra_project_init_py() {
        // `from torch.nn import Module` in a .py file should resolve to
        // `torch/nn/__init__.py` when that file exists in the project.
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("myapp/train.py"),
            vec![ImportInfo {
                source: "torch.nn".into(),
                specifiers: vec!["Module".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("torch/nn/__init__.py"),
            vec![make_symbol("Module", "torch/nn/__init__.py", 5)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_symbol, "Module");
        assert_eq!(r.target_file, Some(PathBuf::from("torch/nn/__init__.py")));
        assert_eq!(r.target_line, Some(5));
        assert_eq!(r.confidence, 0.95);
        assert_eq!(r.resolution_method, ResolutionMethod::ImportBased);
    }

    #[test]
    fn python_absolute_import_resolves_to_module_py() {
        // `from torch.nn.conv import Conv2d` → `torch/nn/conv.py`
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("myapp/model.py"),
            vec![ImportInfo {
                source: "torch.nn.conv".into(),
                specifiers: vec!["Conv2d".into()],
                line: 2,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("torch/nn/conv.py"),
            vec![make_symbol("Conv2d", "torch/nn/conv.py", 10)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_file, Some(PathBuf::from("torch/nn/conv.py")));
        assert_eq!(r.target_line, Some(10));
        assert_eq!(r.confidence, 0.95);
    }

    #[test]
    fn python_absolute_import_falls_to_external_when_not_in_project() {
        // `from numpy import array` — numpy not in project, stays external
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("myapp/main.py"),
            vec![ImportInfo {
                source: "numpy".into(),
                specifiers: vec!["array".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let file_symbols: HashMap<PathBuf, Vec<Symbol>> = HashMap::new();

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert!(r.target_file.is_none());
        assert_eq!(r.resolution_method, ResolutionMethod::External);
    }

    #[test]
    fn non_python_file_skips_package_import_resolution() {
        // TypeScript file importing "torch.nn" should NOT trigger Python resolution
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("myapp/main.ts"),
            vec![ImportInfo {
                source: "torch.nn".into(),
                specifiers: vec!["Module".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("torch/nn/__init__.py"),
            vec![make_symbol("Module", "torch/nn/__init__.py", 5)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        // Should be external, not intra-project
        assert!(r.target_file.is_none());
        assert_eq!(r.resolution_method, ResolutionMethod::External);
    }

    #[test]
    fn python_package_import_reexport_gets_high_confidence() {
        // File resolves but specific symbol not directly defined there (re-export)
        let mut file_imports = HashMap::new();
        file_imports.insert(
            PathBuf::from("myapp/main.py"),
            vec![ImportInfo {
                source: "torch.nn".into(),
                specifiers: vec!["NonExistent".into()],
                line: 1,
                aliases: vec![],
            }],
        );

        let mut file_symbols = HashMap::new();
        file_symbols.insert(
            PathBuf::from("torch/nn/__init__.py"),
            vec![make_symbol("Module", "torch/nn/__init__.py", 5)],
        );

        let refs = resolve_imports(&file_imports, &file_symbols, Path::new(""));

        assert_eq!(refs.len(), 1);
        let r = &refs[0];
        assert_eq!(r.target_file, Some(PathBuf::from("torch/nn/__init__.py")));
        assert!(r.target_line.is_none());
        // Re-export pattern: file exists but symbol defined elsewhere → 0.85
        assert_eq!(r.confidence, 0.85);
    }

    // --- try_resolve_python_package_import tests ---

    #[test]
    fn try_resolve_python_package_import_finds_init() {
        let mut fs = HashMap::new();
        fs.insert(
            PathBuf::from("torch/nn/__init__.py"),
            vec![make_symbol("Module", "torch/nn/__init__.py", 1)],
        );

        let result = try_resolve_python_package_import("torch.nn", &fs, Path::new(""));
        assert_eq!(result, Some(PathBuf::from("torch/nn/__init__.py")));
    }

    #[test]
    fn try_resolve_python_package_import_finds_module_file() {
        let mut fs = HashMap::new();
        fs.insert(
            PathBuf::from("mylib/utils.py"),
            vec![make_symbol("helper", "mylib/utils.py", 1)],
        );

        let result = try_resolve_python_package_import("mylib.utils", &fs, Path::new(""));
        assert_eq!(result, Some(PathBuf::from("mylib/utils.py")));
    }

    #[test]
    fn try_resolve_python_package_import_returns_none_for_missing() {
        let fs: HashMap<PathBuf, Vec<Symbol>> = HashMap::new();
        let result = try_resolve_python_package_import("nonexistent.module", &fs, Path::new(""));
        assert_eq!(result, None);
    }
