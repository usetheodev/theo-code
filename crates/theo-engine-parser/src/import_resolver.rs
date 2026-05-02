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
        if is_python
            && let Some(resolved_path) =
                try_resolve_python_package_import(&import.source, file_symbols, project_root)
            {
                return resolve_intra_project_import(
                    importing_file,
                    import,
                    &resolved_path,
                    file_symbols,
                );
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
#[path = "import_resolver_tests.rs"]
mod tests;
