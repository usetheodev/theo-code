//! Cross-file import resolution for the CodeModel.
//!
//! This module performs a post-processing step after per-file extraction:
//! it takes the aggregated imports and symbols from all files and resolves
//! relative imports to concrete file paths and symbol definitions. Each
//! resolved import produces a [`Reference`] with [`ReferenceKind::Import`].
//!
//! Resolution strategy:
//! - **Relative imports** (`./foo`, `../bar`): resolve to a filesystem path
//!   by trying common extensions and index file conventions, then match
//!   specifiers against the target file's exported symbols.
//! - **Package imports** (`express`, `lodash`): marked as external with
//!   `target_file: None` since the definition lives outside the project.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::extractors::language_behavior::behavior_for;
use crate::symbol_table::{self, SymbolTable};
use crate::tree_sitter::SupportedLanguage;
use crate::types::{ImportInfo, Reference, ReferenceKind, ResolutionMethod, Symbol};

/// File extensions to try when resolving a relative import path.
///
/// Ordered by frequency in typical polyglot codebases. When a relative
/// import like `./user-service` is encountered, we try appending each
/// extension until a file is found in `file_symbols`.
const RESOLVE_EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".rs", ".java", ".cs", ".rb", ".php",
];

/// Index file names to try for directory imports (JS/TS convention).
///
/// When `./utils` resolves to a directory, these are tried in order:
/// `./utils/index.ts`, `./utils/index.js`.
const INDEX_FILES: &[&str] = &["index.ts", "index.js", "__init__.py"];

/// Resolve imports to cross-file references.
///
/// Takes a map of file -> (imports) and a map of file -> (symbols), plus
/// the project root, then tries to resolve each import to a concrete file
/// and symbol.
///
/// Resolution strategy:
/// 1. Relative imports (`./foo`, `../bar`) -- resolve filesystem path,
///    look up symbols in target file.
/// 2. Named imports (`{ Foo } from './bar'`) -- find `Foo` in target file's symbols.
/// 3. Package imports (`express`, `lodash`) -- mark as external (`target_file: None`).
///
/// Returns a `Vec<Reference>` with `ReferenceKind::Import` for each resolved import.
pub fn resolve_imports(
    file_imports: &HashMap<PathBuf, Vec<ImportInfo>>,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
    project_root: &Path,
) -> Vec<Reference> {
    // Build symbol index: name -> Vec<(file, line)> for fast lookup.
    let symbol_index = build_symbol_index(file_symbols);

    let mut references = Vec::new();

    for (importing_file, imports) in file_imports {
        for import in imports {
            let resolved_refs = resolve_single_import(
                importing_file,
                import,
                file_symbols,
                &symbol_index,
                project_root,
            );
            references.extend(resolved_refs);
        }
    }

    // Sort for deterministic output: by (source_file, source_line, target_symbol).
    references.sort_by(|a, b| {
        (&a.source_file, a.source_line, &a.target_symbol).cmp(&(
            &b.source_file,
            b.source_line,
            &b.target_symbol,
        ))
    });

    references
}

/// Resolve all references: import refs + call/hierarchy refs via SymbolTable.
///
/// Two-phase resolution:
/// 1. **Import resolution** (existing logic) — produces Import refs with confidence
/// 2. **Call/hierarchy resolution** — uses SymbolTable to resolve Call, Extends,
///    Implements, TypeUsage refs that extractors left as unresolved
///
/// The `unresolved_refs` parameter contains raw references from extractors
/// (calls, extends, implements) with `target_file: None`.
pub fn resolve_all_references(
    file_imports: &HashMap<PathBuf, Vec<ImportInfo>>,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
    unresolved_refs: &[Reference],
    project_root: &Path,
    file_languages: &HashMap<PathBuf, SupportedLanguage>,
    python_resolved: Option<&HashMap<String, PathBuf>>,
) -> Vec<Reference> {
    // Phase 1: Import resolution (existing logic)
    let import_refs = resolve_imports(file_imports, file_symbols, project_root);

    // Build symbol table for Phase 2
    let symbol_table = SymbolTable::from_file_symbols(file_symbols);

    // Phase 2: Resolve call/hierarchy references
    //
    // Pre-compute import indices per source file to avoid rebuilding them
    // for every reference. With 1M+ references from the same files, this
    // eliminates massive redundant work.
    let import_indices: HashMap<&PathBuf, HashMap<String, PathBuf>> = file_imports
        .iter()
        .map(|(file, imps)| {
            let index = symbol_table::build_import_index(
                file,
                imps,
                file_symbols,
                project_root,
                python_resolved,
            );
            (file, index)
        })
        .collect();
    let empty_index: HashMap<String, PathBuf> = HashMap::new();

    let mut resolved_refs: Vec<Reference> = Vec::with_capacity(unresolved_refs.len());

    for reference in unresolved_refs {
        // Skip import refs — those were handled in Phase 1
        if reference.reference_kind == ReferenceKind::Import {
            continue;
        }

        // Look up pre-computed import index for this source file
        let imports_for_file = import_indices
            .get(&reference.source_file)
            .unwrap_or(&empty_index);

        // Resolve via symbol table, with builtin detection for the source file's language
        let is_builtin: Box<dyn Fn(&str) -> bool> =
            if let Some(&lang) = file_languages.get(&reference.source_file) {
                let behavior = behavior_for(lang);
                Box::new(move |name: &str| behavior.is_builtin_symbol(name))
            } else {
                Box::new(|_: &str| false)
            };

        let result = symbol_table.resolve_with_builtins(
            &reference.target_symbol,
            &reference.source_file,
            imports_for_file,
            &is_builtin,
        );

        let (target_file, target_line) = match result.location {
            Some(loc) => (Some(loc.file), Some(loc.line)),
            None => (reference.target_file.clone(), reference.target_line),
        };

        // Use the higher confidence: keep original if already resolved
        let (confidence, method) = if reference.confidence > result.confidence {
            (reference.confidence, reference.resolution_method)
        } else {
            (result.confidence, result.method)
        };

        resolved_refs.push(Reference {
            source_symbol: reference.source_symbol.clone(),
            source_file: reference.source_file.clone(),
            source_line: reference.source_line,
            target_symbol: reference.target_symbol.clone(),
            target_file,
            target_line,
            reference_kind: reference.reference_kind,
            confidence,
            resolution_method: method,
            is_test_reference: false,
        });
    }

    // Combine import refs + resolved call/hierarchy refs.
    // No sort here — the caller (build_component) applies the final
    // par_sort_unstable_by for deterministic output. Skipping the
    // intermediate stable sort on potentially millions of references
    // saves ~2-3 seconds on large projects like PyTorch (1.28M refs).
    let mut all_refs = import_refs;
    all_refs.extend(resolved_refs);
    all_refs
}

/// Build an index mapping symbol names to their (file, line) locations.
fn build_symbol_index(
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
) -> HashMap<String, Vec<(PathBuf, usize)>> {
    let mut index: HashMap<String, Vec<(PathBuf, usize)>> = HashMap::new();
    for (file, symbols) in file_symbols {
        for symbol in symbols {
            index
                .entry(symbol.name.clone())
                .or_default()
                .push((file.clone(), symbol.anchor.line));
        }
    }
    index
}

/// Resolve a single import statement into zero or more references.
///
/// Creates one `Reference` per specifier in the import. For imports without
/// explicit specifiers (e.g., `import express from 'express'`), the source
/// string itself is used as the target symbol.
fn resolve_single_import(
    importing_file: &Path,
    import: &ImportInfo,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
    symbol_index: &HashMap<String, Vec<(PathBuf, usize)>>,
    project_root: &Path,
) -> Vec<Reference> {
    // Normalize Python-style relative imports (from . import, from ..utils import)
    // to JS-style relative paths (./module, ../utils) for the resolver.
    let normalized = normalize_python_relative(&import.source);
    let source_to_use = normalized.as_deref().unwrap_or(&import.source);

    let is_relative = source_to_use.starts_with("./") || source_to_use.starts_with("../");

    if is_relative {
        let normalized_import = ImportInfo {
            source: source_to_use.to_string(),
            specifiers: import.specifiers.clone(),
            line: import.line,
            aliases: import.aliases.clone(),
        };
        resolve_relative_import(
            importing_file,
            &normalized_import,
            file_symbols,
            project_root,
        )
    } else {
        // For Python files: try resolving absolute dotted imports as intra-project
        // before falling back to external. `from torch.nn import Module` should
        // resolve to `torch/nn/__init__.py` if it exists in the project.
        let is_python = importing_file
            .extension()
            .is_some_and(|ext| ext == "py" || ext == "pyi");
        if is_python {
            if let Some(resolved_path) =
                try_resolve_python_package_import(&import.source, file_symbols, project_root)
            {
                return resolve_intra_project_import(
                    importing_file,
                    import,
                    &resolved_path,
                    file_symbols,
                );
            }
        }
        resolve_external_import(importing_file, import, symbol_index)
    }
}

/// Resolve a relative import (starts with `./` or `../`).
///
/// Attempts to find the target file by resolving the path relative to the
/// importing file's directory, trying common extensions and index files.
/// For each specifier, looks up the symbol in the resolved file.
fn resolve_relative_import(
    importing_file: &Path,
    import: &ImportInfo,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
    project_root: &Path,
) -> Vec<Reference> {
    let resolved_path =
        resolve_relative_path(importing_file, &import.source, file_symbols, project_root);

    let specifiers = effective_specifiers(import);

    specifiers
        .iter()
        .map(|specifier| {
            let (target_file, target_line) = match &resolved_path {
                Some(path) => {
                    let line = find_symbol_in_file(specifier, path, file_symbols);
                    (Some(path.clone()), line)
                }
                None => (None, None),
            };

            // Confidence scoring for import references:
            // - File resolved + symbol found: 0.95 (import-based, high confidence)
            // - File resolved + symbol NOT found: 0.50 (file known, symbol missing)
            // - File NOT resolved: 0.0 (unresolved relative import)
            let (confidence, resolution_method) = match (&target_file, target_line) {
                (Some(_), Some(_)) => (0.95, ResolutionMethod::ImportBased),
                (Some(_), None) => (0.50, ResolutionMethod::ImportBased),
                _ => (0.0, ResolutionMethod::Unresolved),
            };

            Reference {
                source_symbol: String::new(),
                source_file: importing_file.to_path_buf(),
                source_line: import.line,
                target_symbol: specifier.clone(),
                target_file,
                target_line,
                reference_kind: ReferenceKind::Import,
                confidence,
                resolution_method,
                is_test_reference: false,
            }
        })
        .collect()
}

/// Resolve a package/external import (no `./` or `../` prefix).
///
/// External imports cannot be resolved to a file within the project, so
/// `target_file` is always `None`.
fn resolve_external_import(
    importing_file: &Path,
    import: &ImportInfo,
    _symbol_index: &HashMap<String, Vec<(PathBuf, usize)>>,
) -> Vec<Reference> {
    let specifiers = effective_specifiers(import);

    specifiers
        .iter()
        .map(|specifier| Reference {
            source_symbol: String::new(),
            source_file: importing_file.to_path_buf(),
            source_line: import.line,
            target_symbol: specifier.clone(),
            target_file: None,
            target_line: None,
            reference_kind: ReferenceKind::Import,
            confidence: 0.60,
            resolution_method: ResolutionMethod::External,
            is_test_reference: false,
        })
        .collect()
}

/// Resolve an intra-project Python package import to references.
///
/// Called when `try_resolve_python_package_import` finds that a dotted
/// module path (e.g., `torch.nn`) maps to an actual file in the project.
/// Creates references with the resolved file as target, matching specifiers
/// against the file's exported symbols.
fn resolve_intra_project_import(
    importing_file: &Path,
    import: &ImportInfo,
    resolved_file: &Path,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
) -> Vec<Reference> {
    let specifiers = effective_specifiers(import);

    specifiers
        .iter()
        .map(|specifier| {
            let target_line = find_symbol_in_file(specifier, resolved_file, file_symbols);

            // Symbol found directly → 0.95; not found → 0.85 (likely re-exported
            // via __init__.py or barrel file — the import IS valid, symbol just
            // defined in a sub-module).
            let (confidence, resolution_method) = match target_line {
                Some(_) => (0.95, ResolutionMethod::ImportBased),
                None => (0.85, ResolutionMethod::ImportBased),
            };

            Reference {
                source_symbol: String::new(),
                source_file: importing_file.to_path_buf(),
                source_line: import.line,
                target_symbol: specifier.clone(),
                target_file: Some(resolved_file.to_path_buf()),
                target_line,
                reference_kind: ReferenceKind::Import,
                confidence,
                resolution_method,
                is_test_reference: false,
            }
        })
        .collect()
}

/// Try to resolve a Python dotted module path to a file within the project.
///
/// Converts dots to path separators and checks if the resulting path exists
/// in `file_symbols`. Tries in order:
/// 1. `module/path/__init__.py` (package)
/// 2. `module/path.py` (module file)
///
/// Also tries absolute paths (prefixed with `project_root`) since `file_symbols`
/// may contain absolute keys from `WalkDir`.
///
/// Returns `None` for stdlib/external modules that don't exist in the project.
fn try_resolve_python_package_import(
    source: &str,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
    project_root: &Path,
) -> Option<PathBuf> {
    // Convert dots to path separators: "torch.nn" → "torch/nn"
    let as_path = source.replace('.', "/");

    // Try relative paths first (common in tests and small projects)
    let init_path = PathBuf::from(format!("{as_path}/__init__.py"));
    if file_symbols.contains_key(&init_path) {
        return Some(init_path);
    }

    let module_path = PathBuf::from(format!("{as_path}.py"));
    if file_symbols.contains_key(&module_path) {
        return Some(module_path);
    }

    // Try absolute paths (WalkDir produces absolute keys in production)
    let abs_init = project_root.join(&init_path);
    if file_symbols.contains_key(&abs_init) {
        return Some(abs_init);
    }

    let abs_module = project_root.join(&module_path);
    if file_symbols.contains_key(&abs_module) {
        return Some(abs_module);
    }

    None
}

/// Normalize Python relative imports to JS-style relative paths.
///
/// Python uses leading dots for relative imports:
/// - `"."` → `"./"` (current package)
/// - `".."` → `"../"` (parent package)
/// - `"..utils"` → `"../utils"` (sibling module in parent package)
/// - `".views"` → `"./views"` (sibling module in current package)
///
/// Returns `None` for non-Python-style imports (no leading dots or already
/// in JS-style `./` format).
fn normalize_python_relative(source: &str) -> Option<String> {
    if source.starts_with("./") || source.starts_with("../") {
        return None; // Already JS-style
    }

    if !source.starts_with('.') {
        return None; // Absolute import
    }

    // Count leading dots
    let dot_count = source.chars().take_while(|c| *c == '.').count();
    let remainder = &source[dot_count..];

    match dot_count {
        1 => {
            if remainder.is_empty() {
                Some("./".to_string())
            } else {
                Some(format!("./{remainder}"))
            }
        }
        n => {
            // n dots = n-1 levels up: .. = ../, ... = ../../
            let ups = "../".repeat(n - 1);
            if remainder.is_empty() {
                Some(ups)
            } else {
                Some(format!("{ups}{remainder}"))
            }
        }
    }
}

/// Return the effective specifiers for an import.
///
/// If the import has explicit specifiers, use those. Otherwise, use the
/// source module name as the specifier (e.g., `import express from 'express'`
/// uses `"express"` as the target symbol).
fn effective_specifiers(import: &ImportInfo) -> Vec<String> {
    if import.specifiers.is_empty() {
        vec![import.source.clone()]
    } else {
        import.specifiers.clone()
    }
}

/// Resolve a relative import source to a concrete file path.
///
/// Tries the following in order:
/// 1. Exact path (already has extension and exists in `file_symbols`)
/// 2. Path + each extension in `RESOLVE_EXTENSIONS`
/// 3. Path as directory + each entry in `INDEX_FILES`
///
/// All paths are canonicalized relative to the importing file's parent
/// directory and then made relative to `project_root` for consistency
/// with the keys in `file_symbols`.
fn resolve_relative_path(
    importing_file: &Path,
    source: &str,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
    project_root: &Path,
) -> Option<PathBuf> {
    let base_dir = importing_file.parent()?;
    let raw_path = base_dir.join(source);

    // Normalize the path (resolve `.` and `..` components) without
    // requiring the path to exist on the filesystem. We use a simple
    // component-based normalization instead of `canonicalize()` because
    // the files may only exist in the `file_symbols` map (e.g., in tests).
    let normalized = normalize_path(&raw_path);

    // Make it relative to project_root if possible.
    let relative = make_relative(&normalized, project_root);

    // Try both the normalized (absolute) path and the relative path.
    // In production, file_symbols keys are absolute (from WalkDir);
    // in unit tests, keys are often relative (e.g., "src/service.ts").
    let candidates = [&normalized, &relative];

    for candidate in &candidates {
        // 1. Try the exact path.
        if file_symbols.contains_key(*candidate) {
            return Some((*candidate).clone());
        }

        // 2. Try appending each known extension.
        for ext in RESOLVE_EXTENSIONS {
            let with_ext = append_extension(candidate, ext);
            if file_symbols.contains_key(&with_ext) {
                return Some(with_ext);
            }
        }

        // 3. Try as directory with index files.
        for index_name in INDEX_FILES {
            let index_path = candidate.join(index_name);
            if file_symbols.contains_key(&index_path) {
                return Some(index_path);
            }
        }
    }

    None
}

/// Find a symbol by name in a specific file's symbol list.
///
/// Returns the line number if found, `None` otherwise.
fn find_symbol_in_file(
    symbol_name: &str,
    file: &Path,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
) -> Option<usize> {
    file_symbols
        .get(file)?
        .iter()
        .find(|s| s.name == symbol_name)
        .map(|s| s.anchor.line)
}

/// Normalize a path by resolving `.` and `..` components lexically.
///
/// Unlike `std::fs::canonicalize`, this does not require the path to exist
/// on the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => { /* skip `.` */ }
            std::path::Component::ParentDir => {
                // Pop the last normal component if possible.
                if let Some(std::path::Component::Normal(_)) = components.last() {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            _ => components.push(component),
        }
    }
    components.iter().collect()
}

/// Make a path relative to `root` if it starts with `root`.
///
/// Returns the path unchanged if it is not under `root`.
fn make_relative(path: &Path, root: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Append an extension string (e.g., `".ts"`) to a path.
fn append_extension(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(ext);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
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
}
