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
mod tests {
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
}
