//! Two-level symbol table for cross-file reference resolution.
//!
//! Provides a per-file exact lookup (Level 1) and a global fuzzy lookup
//! (Level 2) to resolve call references that extractors leave as unresolved.
//!
//! Resolution chain (in priority order):
//! 1. **Import-based** (0.95) — symbol resolved via explicit import
//! 2. **Same-file** (0.90) — symbol found in the same file
//! 3. **Global unique** (0.80) — exactly one global match
//! 4. **Global same-directory** (0.60) — prefer match in same directory
//! 5. **Global ambiguous** (0.40) — multiple matches, pick first deterministically
//! 6. **Unresolved** (0.0) — no match found

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::types::{ResolutionMethod, Symbol, SymbolKind};

/// Location of a symbol in the codebase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolLocation {
    /// File containing the symbol definition.
    pub file: PathBuf,
    /// 1-based line number of the definition.
    pub line: usize,
    /// Kind of symbol (function, class, method, etc.).
    pub kind: SymbolKind,
    /// Enclosing parent name (class, module, impl block).
    pub parent: Option<String>,
}

/// Two-level symbol table for name resolution.
///
/// Level 1: per-file exact lookup `(file, name) -> SymbolLocation`
/// Level 2: global fuzzy lookup `name -> Vec<SymbolLocation>`
pub struct SymbolTable {
    /// Level 1: exact file-scoped lookup.
    per_file: HashMap<(PathBuf, String), SymbolLocation>,
    /// Level 2: global name -> all locations.
    global: HashMap<String, Vec<SymbolLocation>>,
}

/// Result of resolving a symbol name.
pub struct ResolveResult {
    /// The resolved location, if found.
    pub location: Option<SymbolLocation>,
    /// Confidence score (0.0–1.0).
    pub confidence: f64,
    /// How the resolution was achieved.
    pub method: ResolutionMethod,
}

impl SymbolTable {
    /// Build a symbol table from per-file symbol lists.
    pub fn from_file_symbols(file_symbols: &HashMap<PathBuf, Vec<Symbol>>) -> Self {
        let mut per_file = HashMap::new();
        let mut global: HashMap<String, Vec<SymbolLocation>> = HashMap::new();

        for (file, symbols) in file_symbols {
            for symbol in symbols {
                let location = SymbolLocation {
                    file: file.clone(),
                    line: symbol.anchor.line,
                    kind: symbol.kind,
                    parent: symbol.parent.clone(),
                };

                per_file.insert((file.clone(), symbol.name.clone()), location.clone());

                global
                    .entry(symbol.name.clone())
                    .or_default()
                    .push(location);
            }
        }

        // Sort global entries by file path for deterministic resolution
        for locations in global.values_mut() {
            locations.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
        }

        Self { per_file, global }
    }

    /// Level 1: exact lookup by (file, name).
    pub fn resolve_in_file(&self, file: &Path, name: &str) -> Option<&SymbolLocation> {
        self.per_file.get(&(file.to_path_buf(), name.to_string()))
    }

    /// Level 2: global lookup by name — returns all matches.
    pub fn resolve_global(&self, name: &str) -> &[SymbolLocation] {
        self.global.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Resolve a symbol name using the full heuristic chain.
    ///
    /// Resolution order:
    /// 1. Import-based: check `imported_symbols` map, resolve in target file
    /// 2. Same-file: check `source_file`
    /// 3. Global unique: exactly one global match
    /// 4. Global same-directory: prefer match in same directory as source
    /// 5. Global ambiguous: multiple matches, pick first deterministically
    /// 6. Builtin: check `is_builtin` closure (language builtins)
    /// 7. Unresolved: no match
    pub fn resolve(
        &self,
        name: &str,
        source_file: &Path,
        imported_symbols: &HashMap<String, PathBuf>,
    ) -> ResolveResult {
        self.resolve_with_builtins(name, source_file, imported_symbols, &|_| false)
    }

    /// Resolve with an optional builtin symbol checker.
    ///
    /// Same as [`resolve`] but accepts a closure to check if an unresolved
    /// symbol is a language builtin (e.g., Python `print`, JS `console`).
    /// When a symbol matches a builtin, it's classified as `External` (0.65)
    /// instead of `Unresolved` (0.0).
    pub fn resolve_with_builtins(
        &self,
        name: &str,
        source_file: &Path,
        imported_symbols: &HashMap<String, PathBuf>,
        is_builtin: &dyn Fn(&str) -> bool,
    ) -> ResolveResult {
        let bare_name = extract_bare_name(name);

        // 1. Import-based resolution (0.95 internal, 0.85 re-export, 0.75 external)
        if let Some(target_file) = imported_symbols.get(bare_name) {
            if target_file.as_os_str() == EXTERNAL_SENTINEL {
                return ResolveResult {
                    location: None,
                    confidence: 0.75,
                    method: ResolutionMethod::ImportKnown,
                };
            }
            if let Some(loc) = self.resolve_in_file(target_file, bare_name) {
                return ResolveResult {
                    location: Some(loc.clone()),
                    confidence: 0.95,
                    method: ResolutionMethod::ImportBased,
                };
            }
            // File exists in project but symbol not directly defined there.
            // This is the re-export pattern (Python __init__.py, JS barrel files):
            // the symbol IS available via this module, just defined elsewhere.
            return ResolveResult {
                location: None,
                confidence: 0.85,
                method: ResolutionMethod::ImportBased,
            };
        }

        // 1b. Receiver-prefix check for qualified names (e.g., "np.array" → check "np")
        // If the receiver is an imported alias, the call targets an external package member.
        if bare_name != name
            && let Some(receiver) = extract_receiver(name)
                && let Some(target_file) = imported_symbols.get(receiver) {
                    if target_file.as_os_str() == EXTERNAL_SENTINEL {
                        return ResolveResult {
                            location: None,
                            confidence: 0.70,
                            method: ResolutionMethod::ImportKnown,
                        };
                    }
                    // Receiver points to a project file — try resolving bare name there
                    if let Some(loc) = self.resolve_in_file(target_file, bare_name) {
                        return ResolveResult {
                            location: Some(loc.clone()),
                            confidence: 0.90,
                            method: ResolutionMethod::ImportBased,
                        };
                    }
                    // Receiver file exists but symbol not found — re-export pattern
                    return ResolveResult {
                        location: None,
                        confidence: 0.80,
                        method: ResolutionMethod::ImportBased,
                    };
                }

        // 2. Same-file resolution (0.90)
        if let Some(loc) = self.resolve_in_file(source_file, bare_name) {
            return ResolveResult {
                location: Some(loc.clone()),
                confidence: 0.90,
                method: ResolutionMethod::SameFile,
            };
        }

        // 3-6. Global resolution
        let global_matches = self.resolve_global(bare_name);
        match global_matches.len() {
            0 => {
                // 6. Builtin check — known language builtins classified as External
                if is_builtin(bare_name) {
                    ResolveResult {
                        location: None,
                        confidence: 0.65,
                        method: ResolutionMethod::External,
                    }
                } else {
                    ResolveResult {
                        location: None,
                        confidence: 0.0,
                        method: ResolutionMethod::Unresolved,
                    }
                }
            }
            1 => ResolveResult {
                location: Some(global_matches[0].clone()),
                confidence: 0.80,
                method: ResolutionMethod::GlobalUnique,
            },
            _ => {
                // Prefer match in same directory as source file
                let source_dir = source_file.parent();
                if let Some(dir) = source_dir
                    && let Some(loc) = global_matches.iter().find(|l| l.file.parent() == Some(dir))
                    {
                        return ResolveResult {
                            location: Some(loc.clone()),
                            confidence: 0.60,
                            method: ResolutionMethod::GlobalSameDir,
                        };
                    }

                // Fall back to first match (deterministic since sorted)
                ResolveResult {
                    location: Some(global_matches[0].clone()),
                    confidence: 0.40,
                    method: ResolutionMethod::GlobalAmbiguous,
                }
            }
        }
    }
}

/// Sentinel path used to mark external import specifiers in the import index.
///
/// When `resolve()` encounters this sentinel as the target file, it returns
/// `ImportKnown` (0.75) instead of `ImportBased` (0.95), since the symbol
/// is confirmed via import but lives outside the project.
pub const EXTERNAL_SENTINEL: &str = "<external>";

/// Build an import index mapping imported names to their source files.
///
/// For each file, examines its imports and resolves them to target files
/// using the `file_symbols` map (same logic as `import_resolver`).
///
/// External imports (no `./` or `../` prefix) register their specifiers
/// with a sentinel path, enabling Phase 2 to recognize them as
/// "imported from external package" rather than falling to global search.
///
/// When `python_resolved` is provided (from the runtime Python resolver),
/// it takes priority over static resolution for Python imports, enabling
/// accurate re-export chain following (e.g., `torch.nn.Module` resolves
/// to `torch/nn/modules/module.py` instead of `torch/nn/__init__.py`).
pub fn build_import_index(
    file: &Path,
    imports: &[crate::types::ImportInfo],
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
    project_root: &Path,
    python_resolved: Option<&HashMap<String, PathBuf>>,
) -> HashMap<String, PathBuf> {
    let mut index = HashMap::new();
    let external_path = PathBuf::from(EXTERNAL_SENTINEL);
    let is_python = file
        .extension()
        .is_some_and(|ext| ext == "py" || ext == "pyi");

    for import in imports {
        let is_relative = import.source.starts_with("./") || import.source.starts_with("../");

        if is_relative {
            // Resolve the import path to a file using the same logic as import_resolver
            if let Some(resolved_file) =
                resolve_relative_path_for_index(file, &import.source, file_symbols, project_root)
            {
                let specifiers = if import.specifiers.is_empty() {
                    vec![import.source.clone()]
                } else {
                    import.specifiers.clone()
                };

                for specifier in specifiers {
                    index.insert(specifier, resolved_file.clone());
                }

                // Register aliases pointing to the same resolved file
                for (alias_name, _original) in &import.aliases {
                    index
                        .entry(alias_name.clone())
                        .or_insert_with(|| resolved_file.clone());
                }
            }
        } else {
            // Priority for non-relative imports:
            // 1. Python runtime resolver (follows re-export chains to actual files)
            // 2. Static Python package resolution (__init__.py / module.py)
            // 3. External sentinel
            let specifiers = if import.specifiers.is_empty() {
                vec![import.source.clone()]
            } else {
                import.specifiers.clone()
            };

            // Track which specifiers were resolved by higher-priority methods
            let mut resolved_specifiers = Vec::new();

            // 1. Python runtime resolver (per-symbol precision)
            if is_python
                && let Some(py_map) = python_resolved {
                    for specifier in &specifiers {
                        let key = format!("{}.{}", import.source, specifier);
                        if let Some(resolved_file) = py_map.get(&key) {
                            index.insert(specifier.clone(), resolved_file.clone());
                            resolved_specifiers.push(specifier.clone());
                        }
                    }
                }

            // 2. Static Python package resolution for remaining specifiers
            if is_python && resolved_specifiers.len() < specifiers.len()
                && let Some(static_file) =
                    try_resolve_python_package(&import.source, file_symbols, project_root)
                {
                    for specifier in &specifiers {
                        if !resolved_specifiers.contains(specifier) {
                            index
                                .entry(specifier.clone())
                                .or_insert_with(|| static_file.clone());
                            resolved_specifiers.push(specifier.clone());
                        }
                    }

                    for (alias_name, _original) in &import.aliases {
                        index
                            .entry(alias_name.clone())
                            .or_insert_with(|| static_file.clone());
                    }
                }

            // 3. External sentinel for anything still unresolved
            for specifier in &specifiers {
                if !resolved_specifiers.contains(specifier) {
                    index
                        .entry(specifier.clone())
                        .or_insert_with(|| external_path.clone());
                }
            }

            // Register aliases (runtime resolver doesn't handle aliases directly)
            if resolved_specifiers.is_empty() {
                // No specifier resolved — aliases also go to sentinel
                for (alias_name, _original) in &import.aliases {
                    index
                        .entry(alias_name.clone())
                        .or_insert_with(|| external_path.clone());
                }
            }
        }
    }

    index
}

/// Try to resolve a Python dotted module path to a file within the project.
///
/// Converts dots to path separators and checks `file_symbols` for:
/// 1. `module/path/__init__.py` (package directory)
/// 2. `module/path.py` (module file)
///
/// Returns `None` for stdlib/external modules not present in the project.
fn try_resolve_python_package(
    source: &str,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
    project_root: &Path,
) -> Option<PathBuf> {
    let as_path = source.replace('.', "/");

    // Try relative paths (common in tests)
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

/// Resolve a relative import path to a concrete file.
///
/// Reuses the same resolution strategy as `import_resolver::resolve_relative_path`:
/// 1. Try exact path
/// 2. Try path + known extensions
/// 3. Try path as directory + index files
fn resolve_relative_path_for_index(
    importing_file: &Path,
    source: &str,
    file_symbols: &HashMap<PathBuf, Vec<Symbol>>,
    project_root: &Path,
) -> Option<PathBuf> {
    let base_dir = importing_file.parent()?;
    let raw_path = base_dir.join(source);
    let normalized = normalize_path(&raw_path);
    let relative = normalized
        .strip_prefix(project_root)
        .map(|p| p.to_path_buf())
        .unwrap_or(normalized);

    // 1. Exact path
    if file_symbols.contains_key(&relative) {
        return Some(relative);
    }

    // 2. Try extensions
    const RESOLVE_EXTENSIONS: &[&str] = &[
        ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".rs", ".java", ".cs", ".rb", ".php",
    ];
    for ext in RESOLVE_EXTENSIONS {
        let mut s = relative.as_os_str().to_os_string();
        s.push(ext);
        let with_ext = PathBuf::from(s);
        if file_symbols.contains_key(&with_ext) {
            return Some(with_ext);
        }
    }

    // 3. Try index files
    const INDEX_FILES: &[&str] = &["index.ts", "index.js", "__init__.py"];
    for index_name in INDEX_FILES {
        let index_path = relative.join(index_name);
        if file_symbols.contains_key(&index_path) {
            return Some(index_path);
        }
    }

    None
}

/// Extract the receiver/prefix from a qualified callee string.
///
/// Returns the first segment before the last separator:
/// - `"np.array"` → `Some("np")`
/// - `"a.b.c"` → `Some("a")`  (first segment, not "a.b")
/// - `"Cls::method"` → `Some("Cls")`
/// - `"validate"` → `None` (no qualifier)
fn extract_receiver(target_symbol: &str) -> Option<&str> {
    // Try . separator first (most common: Python, JS/TS, Java, Go)
    if let Some(pos) = target_symbol.find('.') {
        return Some(&target_symbol[..pos]);
    }
    // Try :: separator (Rust, PHP, C++)
    if let Some(pos) = target_symbol.find("::") {
        return Some(&target_symbol[..pos]);
    }
    // Try -> separator (PHP)
    if let Some(pos) = target_symbol.find("->") {
        return Some(&target_symbol[..pos]);
    }
    None
}

/// Extract the bare function/method name from a qualified callee string.
///
/// Examples:
/// - `"user.save"` → `"save"`
/// - `"Cls::method"` → `"method"`
/// - `"validate"` → `"validate"`
/// - `"a.b.c"` → `"c"`
fn extract_bare_name(target_symbol: &str) -> &str {
    // Try :: separator first (Rust, PHP, C++)
    if let Some(pos) = target_symbol.rfind("::") {
        return &target_symbol[pos + 2..];
    }
    // Try . separator (JS/TS, Python, Java, Go)
    if let Some(pos) = target_symbol.rfind('.') {
        return &target_symbol[pos + 1..];
    }
    // Try -> separator (PHP)
    if let Some(pos) = target_symbol.rfind("->") {
        return &target_symbol[pos + 2..];
    }
    target_symbol
}

/// Normalize a path by resolving `.` and `..` components lexically.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
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

#[cfg(test)]
#[path = "symbol_table_tests.rs"]
mod tests;
