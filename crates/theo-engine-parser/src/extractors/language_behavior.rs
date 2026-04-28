//! Language-specific behavioral traits.
//!
//! Defines the [`LanguageBehavior`] trait that encapsulates language-specific
//! conventions: module separators, source directory roots, visibility parsing,
//! signature extraction, doc comment extraction, parent name resolution, and
//! call node identification.
//!
//! Each language family gets a unit struct implementing the trait. The factory
//! function [`behavior_for`] maps [`SupportedLanguage`] variants to the correct
//! behavior instance, enabling downstream consumers to work polymorphically
//! without language-specific dispatch logic.

use tree_sitter::Node;

use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

// ---------------------------------------------------------------------------
// Trait definition
// ---------------------------------------------------------------------------

/// Language-specific behavioral conventions.
///
/// Provides default implementations where a sensible cross-language default
/// exists. Language-specific structs override only the methods that differ
/// from the defaults.
pub trait LanguageBehavior: Send + Sync {
    /// The separator used between module/namespace segments.
    ///
    /// Examples: `"."` for JavaScript, `"::"` for Rust, `"\\"` for PHP.
    fn module_separator(&self) -> &'static str {
        "."
    }

    /// Common source directory roots for this language.
    ///
    /// Used by module inference to strip prefix paths. For example,
    /// Java projects typically place source in `src/main/java/`.
    fn source_roots(&self) -> &[&str] {
        &["src"]
    }

    /// Extract visibility from a tree-sitter CST node.
    ///
    /// Returns `None` when the language has no visibility concept for the
    /// given node or when the visibility cannot be determined.
    fn parse_visibility(&self, _node: &Node, _source: &str) -> Option<Visibility> {
        None
    }

    /// The character that opens a function/method body.
    ///
    /// Used by [`extract_signature`](LanguageBehavior::extract_signature) to
    /// truncate the declaration at the body boundary. Returns `None` for
    /// languages where signature extraction uses a different strategy
    /// (e.g., Ruby takes the first line).
    fn signature_body_opener(&self) -> Option<char> {
        Some('{')
    }

    /// Extract the declaration signature from a definition node.
    ///
    /// Default: truncates the node text at [`signature_body_opener`](LanguageBehavior::signature_body_opener).
    fn extract_signature(&self, node: &Node, source: &str) -> Option<String> {
        let node_text = node.utf8_text(source.as_bytes()).ok()?;

        let truncated = match self.signature_body_opener() {
            Some(opener) => truncate_at_char(node_text, opener),
            None => node_text.lines().next().map(|l| l.to_string()),
        };

        let sig = truncated.as_deref().unwrap_or(node_text).trim().to_string();

        if sig.is_empty() { None } else { Some(sig) }
    }

    /// Extract a doc comment above the given node.
    ///
    /// Default: looks for `/** ... */`, `///`, or `//` comment siblings
    /// preceding the node (C-family convention).
    fn extract_doc_comment(&self, node: &Node, source: &str) -> Option<String> {
        extract_block_or_line_comment(node, source)
    }

    /// Find the name of the enclosing class, module, trait, or impl block.
    ///
    /// Default: walks up the CST looking for enclosing type definition nodes.
    fn find_parent_name(&self, node: &Node, source: &str) -> Option<String> {
        find_parent_generic(node, source)
    }

    /// CST node kinds that represent function/method calls.
    ///
    /// Used by call graph extraction to identify call sites.
    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }

    /// Determine whether a symbol definition node represents a test function.
    ///
    /// Language-specific detection patterns include:
    /// - **Naming conventions:** `test_*` (Python, Ruby, PHP), `Test*` (Go)
    /// - **Annotations/attributes:** `@Test` (Java/Kotlin), `[Test]`/`[Fact]`/`[Theory]` (C#),
    ///   `#[test]` (Rust), `#[Test]` (PHP)
    /// - **File heuristic:** TS/JS functions named `test*` in test files
    ///
    /// Returns `false` by default (GenericBehavior and languages without test patterns).
    fn is_test_symbol(&self, _node: &Node, _source: &str, _symbol_name: &str) -> bool {
        false
    }

    /// Check if a module name belongs to this language's standard library.
    ///
    /// Used by downstream analysis to differentiate "known stdlib" external
    /// imports from "unknown third-party" external imports. The check uses
    /// the top-level module name (e.g., `os` from `os.path`, `collections`
    /// from `collections.abc`).
    ///
    /// Returns `false` by default — only languages with well-defined stdlib
    /// boundaries override this (currently Python).
    fn is_stdlib_module(&self, _module_name: &str) -> bool {
        false
    }

    /// Check if a symbol name is a language builtin (type, function, constant).
    ///
    /// Builtins are symbols available without any import statement — they exist
    /// in the language's global scope. When Phase 2 resolution fails to find a
    /// symbol and it matches a builtin, it can be classified as `External`
    /// instead of `Unresolved`, improving confidence scoring.
    ///
    /// Returns `false` by default — only languages with well-defined builtin
    /// sets override this (currently Python and TypeScript/JavaScript).
    fn is_builtin_symbol(&self, _name: &str) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Behavior implementations
// ---------------------------------------------------------------------------

/// Behavior for TypeScript, TSX, JavaScript, and JSX.
pub(crate) struct TypeScriptBehavior;

impl LanguageBehavior for TypeScriptBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src", "lib", "app"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        let text = node.utf8_text(source.as_bytes()).ok()?;
        if text.starts_with("export") {
            return Some(Visibility::Public);
        }
        if let Some(parent) = node.parent()
            && parent.kind() == "export_statement" {
                return Some(Visibility::Public);
            }
        None
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }

    /// TS/JS: functions named `test*` are considered test symbols.
    ///
    /// This is a name-prefix heuristic. BDD-style `describe`/`it` blocks
    /// are call_expressions, not function_declarations — not detected here.
    fn is_test_symbol(&self, _node: &Node, _source: &str, symbol_name: &str) -> bool {
        symbol_name.starts_with("test")
    }

    fn is_builtin_symbol(&self, name: &str) -> bool {
        JS_BUILTINS.binary_search(&name).is_ok()
    }
}

/// Python 3.12 standard library top-level module names.
///
/// Comprehensive list covering the most commonly imported stdlib modules.
/// Used by [`PythonBehavior::is_stdlib_module`] to distinguish stdlib from
/// third-party imports. Sorted alphabetically for maintainability.
///
/// Source: <https://docs.python.org/3.12/py-modindex.html>
const PYTHON_STDLIB_MODULES: &[&str] = &[
    "abc",
    "argparse",
    "ast",
    "asyncio",
    "atexit",
    "base64",
    "bisect",
    "builtins",
    "calendar",
    "cmath",
    "codecs",
    "collections",
    "colorsys",
    "concurrent",
    "configparser",
    "contextlib",
    "contextvars",
    "copy",
    "csv",
    "ctypes",
    "dataclasses",
    "datetime",
    "decimal",
    "difflib",
    "dis",
    "email",
    "enum",
    "errno",
    "faulthandler",
    "fcntl",
    "fileinput",
    "fnmatch",
    "fractions",
    "ftplib",
    "functools",
    "gc",
    "getpass",
    "gettext",
    "glob",
    "gzip",
    "hashlib",
    "heapq",
    "hmac",
    "html",
    "http",
    "importlib",
    "inspect",
    "io",
    "ipaddress",
    "itertools",
    "json",
    "keyword",
    "linecache",
    "locale",
    "logging",
    "lzma",
    "marshal",
    "math",
    "mimetypes",
    "mmap",
    "multiprocessing",
    "numbers",
    "operator",
    "os",
    "pathlib",
    "pdb",
    "pickle",
    "pkgutil",
    "platform",
    "plistlib",
    "pprint",
    "profile",
    "pstats",
    "queue",
    "random",
    "re",
    "readline",
    "reprlib",
    "resource",
    "runpy",
    "sched",
    "secrets",
    "select",
    "selectors",
    "shelve",
    "shlex",
    "shutil",
    "signal",
    "site",
    "smtplib",
    "socket",
    "sqlite3",
    "ssl",
    "stat",
    "statistics",
    "string",
    "struct",
    "subprocess",
    "sys",
    "sysconfig",
    "syslog",
    "tempfile",
    "termios",
    "textwrap",
    "threading",
    "time",
    "timeit",
    "token",
    "tokenize",
    "tomllib",
    "traceback",
    "tracemalloc",
    "tty",
    "turtle",
    "types",
    "typing",
    "unicodedata",
    "unittest",
    "urllib",
    "uuid",
    "venv",
    "warnings",
    "wave",
    "weakref",
    "webbrowser",
    "xml",
    "xmlrpc",
    "zipfile",
    "zipimport",
    "zlib",
];

/// Python builtin symbols — available without import.
///
/// Covers built-in functions, types, exceptions, and constants from CPython 3.12.
/// Sorted alphabetically for binary search.
const PYTHON_BUILTINS: &[&str] = &[
    "ArithmeticError",
    "AssertionError",
    "AttributeError",
    "BaseException",
    "BlockingIOError",
    "BrokenPipeError",
    "BufferError",
    "BytesWarning",
    "ConnectionError",
    "DeprecationWarning",
    "EOFError",
    "Ellipsis",
    "EnvironmentError",
    "Exception",
    "False",
    "FileExistsError",
    "FileNotFoundError",
    "FloatingPointError",
    "FutureWarning",
    "GeneratorExit",
    "IOError",
    "ImportError",
    "ImportWarning",
    "IndentationError",
    "IndexError",
    "InterruptedError",
    "IsADirectoryError",
    "KeyError",
    "KeyboardInterrupt",
    "LookupError",
    "MemoryError",
    "ModuleNotFoundError",
    "NameError",
    "None",
    "NotADirectoryError",
    "NotImplemented",
    "NotImplementedError",
    "OSError",
    "OverflowError",
    "PendingDeprecationWarning",
    "PermissionError",
    "ProcessLookupError",
    "RecursionError",
    "ReferenceError",
    "ResourceWarning",
    "RuntimeError",
    "RuntimeWarning",
    "StopAsyncIteration",
    "StopIteration",
    "SyntaxError",
    "SyntaxWarning",
    "SystemError",
    "SystemExit",
    "TabError",
    "TimeoutError",
    "True",
    "TypeError",
    "UnboundLocalError",
    "UnicodeDecodeError",
    "UnicodeEncodeError",
    "UnicodeError",
    "UnicodeTranslateError",
    "UnicodeWarning",
    "UserWarning",
    "ValueError",
    "Warning",
    "ZeroDivisionError",
    "abs",
    "all",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "copyright",
    "credits",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "exit",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "license",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "quit",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
];

/// TypeScript/JavaScript builtin globals — available without import.
///
/// Covers global constructors, objects, and functions from ECMAScript 2023 + Node.js.
/// Sorted alphabetically for binary search.
const JS_BUILTINS: &[&str] = &[
    "Array",
    "ArrayBuffer",
    "BigInt",
    "Boolean",
    "Buffer",
    "DataView",
    "Date",
    "Error",
    "EvalError",
    "Float32Array",
    "Float64Array",
    "Function",
    "Infinity",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "JSON",
    "Map",
    "Math",
    "NaN",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "Reflect",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URIError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakRef",
    "WeakSet",
    "clearInterval",
    "clearTimeout",
    "console",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "eval",
    "fetch",
    "globalThis",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "process",
    "queueMicrotask",
    "require",
    "setInterval",
    "setTimeout",
    "structuredClone",
    "undefined",
];

/// Behavior for Python.
pub(crate) struct PythonBehavior;

impl LanguageBehavior for PythonBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src", "app"]
    }

    fn signature_body_opener(&self) -> Option<char> {
        Some(':')
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        // Python uses naming convention — extract name from the node.
        // Both `_private` (convention) and `__mangled` (name-mangling)
        // are treated as Private in our model.
        let name = extract_name_from_node(node, source)?;
        if name.starts_with('_') {
            Some(Visibility::Private)
        } else {
            Some(Visibility::Public)
        }
    }

    fn extract_doc_comment(&self, node: &Node, source: &str) -> Option<String> {
        extract_python_docstring(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call"]
    }

    /// Python: `def test_*` or methods inside `unittest.TestCase` subclasses.
    fn is_test_symbol(&self, _node: &Node, _source: &str, symbol_name: &str) -> bool {
        symbol_name.starts_with("test_") || symbol_name.starts_with("test")
    }

    /// Python 3.x standard library modules.
    ///
    /// Matches the top-level module name against the CPython 3.12 stdlib.
    /// For dotted imports like `os.path`, the caller should extract the
    /// first segment (`os`) before calling this method.
    fn is_stdlib_module(&self, module_name: &str) -> bool {
        PYTHON_STDLIB_MODULES.contains(&module_name)
    }

    fn is_builtin_symbol(&self, name: &str) -> bool {
        PYTHON_BUILTINS.binary_search(&name).is_ok()
    }
}

/// Behavior for Java and Kotlin.
pub(crate) struct JavaBehavior;

impl LanguageBehavior for JavaBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src/main/java", "src"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        extract_visibility_modifier_child(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["method_invocation"]
    }

    /// Java/Kotlin: check for `@Test` annotation on the preceding sibling.
    fn is_test_symbol(&self, node: &Node, source: &str, _symbol_name: &str) -> bool {
        has_preceding_annotation(node, source, &["Test"])
    }
}

/// Behavior for C#.
pub(crate) struct CSharpBehavior;

impl LanguageBehavior for CSharpBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src", "Controllers", "Services"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        // C# uses `modifier` child nodes (includes `internal`)
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32)
                && child.kind() == "modifier" {
                    let mod_text = match child.utf8_text(source.as_bytes()) {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    if let Some(vis) = parse_visibility_keyword(mod_text) {
                        return Some(vis);
                    }
                }
        }
        // Fallback: check first word of node text
        let text = node.utf8_text(source.as_bytes()).ok()?;
        let first_word = text.split_whitespace().next()?;
        parse_visibility_keyword(first_word)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["invocation_expression"]
    }

    /// C#: check for `[Test]`, `[Fact]`, or `[Theory]` attributes.
    fn is_test_symbol(&self, node: &Node, source: &str, _symbol_name: &str) -> bool {
        has_preceding_attribute(node, source, &["Test", "Fact", "Theory"])
    }
}

/// Behavior for Go.
pub(crate) struct GoBehavior;

impl LanguageBehavior for GoBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["cmd", "internal", "pkg"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        // Go uses capitalization convention
        let name = extract_name_from_node(node, source)?;
        let first_char = name.chars().next()?;
        if first_char.is_uppercase() {
            Some(Visibility::Public)
        } else {
            Some(Visibility::Private)
        }
    }

    fn find_parent_name(&self, node: &Node, source: &str) -> Option<String> {
        // Go methods have a receiver type — extract it from method_declaration
        if node.kind() == "method_declaration" {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32)
                    && child.kind() == "parameter_list" {
                        let text = child.utf8_text(source.as_bytes()).ok()?;
                        let cleaned = text.trim_matches(|c| c == '(' || c == ')');
                        let type_name = cleaned.split_whitespace().last()?.trim_start_matches('*');
                        return Some(type_name.to_string());
                    }
            }
        }
        find_parent_generic(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }

    /// Go: `func Test*(t *testing.T)` — name starts with `Test` and has
    /// a `testing.T` or `testing.B` or `testing.M` parameter.
    fn is_test_symbol(&self, node: &Node, source: &str, symbol_name: &str) -> bool {
        if !symbol_name.starts_with("Test")
            && !symbol_name.starts_with("Benchmark")
            && !symbol_name.starts_with("Fuzz")
        {
            return false;
        }
        // Verify the function signature contains testing.T/B/M/F
        let text = match node.utf8_text(source.as_bytes()) {
            Ok(t) => t,
            Err(_) => return false,
        };
        text.contains("testing.T")
            || text.contains("testing.B")
            || text.contains("testing.M")
            || text.contains("testing.F")
    }
}

/// Behavior for PHP.
pub(crate) struct PhpBehavior;

impl LanguageBehavior for PhpBehavior {
    fn module_separator(&self) -> &'static str {
        "\\"
    }

    fn source_roots(&self) -> &[&str] {
        &["src", "app"]
    }

    fn parse_visibility(&self, node: &Node, source: &str) -> Option<Visibility> {
        extract_visibility_modifier_child(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &[
            "member_call_expression",
            "function_call_expression",
            "scoped_call_expression",
        ]
    }

    /// PHP: `function test*()` name prefix or `#[Test]` attribute.
    fn is_test_symbol(&self, node: &Node, source: &str, symbol_name: &str) -> bool {
        if symbol_name.starts_with("test") {
            return true;
        }
        has_preceding_attribute(node, source, &["Test"])
    }
}

/// Behavior for Ruby.
pub(crate) struct RubyBehavior;

impl LanguageBehavior for RubyBehavior {
    fn module_separator(&self) -> &'static str {
        "::"
    }

    fn source_roots(&self) -> &[&str] {
        &["app", "lib"]
    }

    fn signature_body_opener(&self) -> Option<char> {
        // Ruby: take the first line as the signature
        None
    }

    fn extract_doc_comment(&self, node: &Node, source: &str) -> Option<String> {
        extract_hash_comment(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call", "method_call"]
    }

    /// Ruby: `def test_*` naming convention.
    fn is_test_symbol(&self, _node: &Node, _source: &str, symbol_name: &str) -> bool {
        symbol_name.starts_with("test_")
    }
}

/// Behavior for Rust.
pub(crate) struct RustBehavior;

impl LanguageBehavior for RustBehavior {
    fn module_separator(&self) -> &'static str {
        "::"
    }

    fn source_roots(&self) -> &[&str] {
        &["src"]
    }

    fn parse_visibility(&self, node: &Node, _source: &str) -> Option<Visibility> {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32)
                && child.kind() == "visibility_modifier" {
                    return Some(Visibility::Public);
                }
        }
        Some(Visibility::Private)
    }

    fn extract_doc_comment(&self, node: &Node, source: &str) -> Option<String> {
        extract_rust_doc_comment(node, source)
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }

    /// Rust: `#[test]` or `#[tokio::test]` attribute on the function.
    fn is_test_symbol(&self, node: &Node, source: &str, _symbol_name: &str) -> bool {
        has_preceding_rust_attribute(node, source, &["test", "tokio::test", "rstest"])
    }
}

/// Fallback behavior for C, C++, Swift, and Scala.
pub(crate) struct GenericBehavior;

impl LanguageBehavior for GenericBehavior {
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &[&str] {
        &["src"]
    }

    fn call_node_kinds(&self) -> &[&str] {
        &["call_expression"]
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

// Static instances for each behavior (unit structs, zero-cost).
static TYPESCRIPT_BEHAVIOR: TypeScriptBehavior = TypeScriptBehavior;
static PYTHON_BEHAVIOR: PythonBehavior = PythonBehavior;
static JAVA_BEHAVIOR: JavaBehavior = JavaBehavior;
static CSHARP_BEHAVIOR: CSharpBehavior = CSharpBehavior;
static GO_BEHAVIOR: GoBehavior = GoBehavior;
static PHP_BEHAVIOR: PhpBehavior = PhpBehavior;
static RUBY_BEHAVIOR: RubyBehavior = RubyBehavior;
static RUST_BEHAVIOR: RustBehavior = RustBehavior;
static GENERIC_BEHAVIOR: GenericBehavior = GenericBehavior;

/// Return the [`LanguageBehavior`] implementation for a given language.
///
/// Languages that share a grammar family map to the same behavior:
/// - TypeScript, TSX, JavaScript, JSX -> [`TypeScriptBehavior`]
/// - Java, Kotlin, Scala -> [`JavaBehavior`]
/// - C, C++, Swift -> [`GenericBehavior`]
pub fn behavior_for(language: SupportedLanguage) -> &'static dyn LanguageBehavior {
    match language {
        SupportedLanguage::TypeScript
        | SupportedLanguage::Tsx
        | SupportedLanguage::JavaScript
        | SupportedLanguage::Jsx => &TYPESCRIPT_BEHAVIOR,
        SupportedLanguage::Python => &PYTHON_BEHAVIOR,
        SupportedLanguage::Java | SupportedLanguage::Kotlin => &JAVA_BEHAVIOR,
        SupportedLanguage::CSharp => &CSHARP_BEHAVIOR,
        SupportedLanguage::Go => &GO_BEHAVIOR,
        SupportedLanguage::Php => &PHP_BEHAVIOR,
        SupportedLanguage::Ruby => &RUBY_BEHAVIOR,
        SupportedLanguage::Rust => &RUST_BEHAVIOR,
        SupportedLanguage::Swift | SupportedLanguage::C | SupportedLanguage::Cpp => {
            &GENERIC_BEHAVIOR
        }
        SupportedLanguage::Scala => &JAVA_BEHAVIOR,
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (used by trait implementations)
// ---------------------------------------------------------------------------

/// Truncate text at the first occurrence of `ch`, trimming whitespace.
fn truncate_at_char(text: &str, ch: char) -> Option<String> {
    text.find(ch).map(|pos| text[..pos].trim().to_string())
}

/// Parse a visibility keyword string into [`Visibility`].
fn parse_visibility_keyword(keyword: &str) -> Option<Visibility> {
    match keyword.trim() {
        "public" => Some(Visibility::Public),
        "private" => Some(Visibility::Private),
        "protected" => Some(Visibility::Protected),
        "internal" => Some(Visibility::Internal),
        _ => None,
    }
}

/// Look for visibility modifier keywords in child nodes (Java/PHP pattern).
fn extract_visibility_modifier_child(node: &Node, source: &str) -> Option<Visibility> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let kind = child.kind();
            if kind == "modifiers" || kind == "modifier" || kind == "visibility_modifier" {
                let mod_text = match child.utf8_text(source.as_bytes()) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if let Some(vis) = parse_visibility_keyword(mod_text) {
                    return Some(vis);
                }
            }
            // Direct keyword nodes (some grammars use these)
            if let Some(vis) = parse_visibility_keyword(kind) {
                return Some(vis);
            }
        }
    }
    // Fallback: check if text starts with a visibility keyword
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let first_word = text.split_whitespace().next()?;
    parse_visibility_keyword(first_word)
}

/// Extract the name identifier from a node (looks for common name child kinds).
fn extract_name_from_node(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            let kind = child.kind();
            if kind == "identifier"
                || kind == "type_identifier"
                || kind == "name"
                || kind == "constant"
                || kind == "property_identifier"
                || kind == "field_identifier"
            {
                return child
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
        }
    }
    None
}

/// Generic parent finder: walk up looking for class/module/trait/impl nodes.
fn find_parent_generic(node: &Node, source: &str) -> Option<String> {
    let mut current = node.parent()?;
    loop {
        let kind = current.kind();
        if is_enclosing_type(kind) {
            return extract_name_child(&current, source);
        }
        current = current.parent()?;
    }
}

/// Check if a CST node kind represents an enclosing type definition.
fn is_enclosing_type(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "class_definition"
            | "class"
            | "record_declaration"
            | "interface_declaration"
            | "trait_item"
            | "trait_declaration"
            | "impl_item"
            | "struct_declaration"
            | "struct_item"
            | "enum_declaration"
            | "enum_item"
            | "module"
            | "mod_item"
    )
}

/// Extract the `name:` child text from an enclosing type node.
fn extract_name_child(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            let kind = child.kind();
            if kind == "identifier"
                || kind == "type_identifier"
                || kind == "name"
                || kind == "constant"
                || kind == "property_identifier"
            {
                return child
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Test detection helpers
// ---------------------------------------------------------------------------

/// Check for a Java/Kotlin annotation (`@Name`) on a method_declaration node.
///
/// Java tree-sitter grammar nests annotations inside `modifiers` child nodes
/// of the declaration. We walk the node's children looking for `modifiers`
/// containing `marker_annotation` or `annotation` nodes, and also check
/// direct preceding siblings (some grammar versions place them there).
fn has_preceding_annotation(node: &Node, source: &str, names: &[&str]) -> bool {
    // Strategy 1: Check child `modifiers` node (Java grammar nests annotations here)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let kind = child.kind();
            if kind == "modifiers" {
                // Walk inside modifiers for annotations
                for j in 0..child.child_count() {
                    if let Some(mod_child) = child.child(j as u32)
                        && check_annotation_node(&mod_child, source, names) {
                            return true;
                        }
                }
            }
            // Direct annotation child (some grammar versions)
            if check_annotation_node(&child, source, names) {
                return true;
            }
        }
    }

    // Strategy 2: Check preceding siblings (fallback)
    let mut sib = node.prev_named_sibling();
    while let Some(s) = sib {
        if check_annotation_node(&s, source, names) {
            return true;
        }
        let kind = s.kind();
        if kind != "marker_annotation"
            && kind != "annotation"
            && kind != "modifiers"
            && kind != "modifier"
        {
            break;
        }
        sib = s.prev_named_sibling();
    }

    false
}

/// Check if a single CST node is an annotation matching one of the target names.
fn check_annotation_node(node: &Node, source: &str, names: &[&str]) -> bool {
    let kind = node.kind();
    if (kind == "marker_annotation" || kind == "annotation")
        && let Ok(text) = node.utf8_text(source.as_bytes()) {
            let ann_name = text.trim_start_matches('@');
            for name in names {
                if ann_name == *name || ann_name.starts_with(&format!("{name}(")) {
                    return true;
                }
            }
        }
    false
}

/// Check for a C# attribute (`[Name]`) on a declaration node.
///
/// C# tree-sitter grammar nests attributes as child `attribute_list` nodes
/// of the declaration. We check both child nodes and preceding siblings.
fn has_preceding_attribute(node: &Node, source: &str, names: &[&str]) -> bool {
    // Strategy 1: Check child nodes (C# grammar nests attributes as children)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let kind = child.kind();
            if (kind == "attribute_list" || kind == "attribute")
                && let Ok(text) = child.utf8_text(source.as_bytes()) {
                    for name in names {
                        if text.contains(name) {
                            return true;
                        }
                    }
                }
        }
    }

    // Strategy 2: Check preceding siblings (fallback)
    let mut sib = node.prev_named_sibling();
    while let Some(s) = sib {
        let kind = s.kind();
        if (kind == "attribute_list" || kind == "attribute")
            && let Ok(text) = s.utf8_text(source.as_bytes()) {
                for name in names {
                    if text.contains(name) {
                        return true;
                    }
                }
            }
        if kind != "attribute_list" && kind != "attribute" && kind != "modifier" {
            break;
        }
        sib = s.prev_named_sibling();
    }
    false
}

/// Check for a Rust `#[name]` attribute_item preceding the function.
///
/// Rust tree-sitter grammar uses `attribute_item` nodes as siblings before
/// function_item nodes.
fn has_preceding_rust_attribute(node: &Node, source: &str, names: &[&str]) -> bool {
    let mut sib = node.prev_named_sibling();
    while let Some(s) = sib {
        if s.kind() == "attribute_item"
            && let Ok(text) = s.utf8_text(source.as_bytes()) {
                // text looks like `#[test]` or `#[tokio::test]`
                let inner = text.trim_start_matches("#[").trim_end_matches(']');
                for name in names {
                    if inner == *name || inner.starts_with(&format!("{name}(")) {
                        return true;
                    }
                }
            }
        // Attribute items can be stacked — keep walking
        if s.kind() != "attribute_item" && s.kind() != "line_comment" {
            break;
        }
        sib = s.prev_named_sibling();
    }
    false
}

// ---------------------------------------------------------------------------
// Doc comment helpers
// ---------------------------------------------------------------------------

/// Find a comment node immediately preceding the given node.
fn find_preceding_comment<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    // Named sibling first
    if let Some(sib) = node.prev_named_sibling() {
        let kind = sib.kind();
        if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
            return Some(sib);
        }
    }
    // Walk unnamed siblings (comments are unnamed in some grammars like Java)
    let mut sib = node.prev_sibling();
    while let Some(s) = sib {
        let kind = s.kind();
        if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
            return Some(s);
        }
        if !s.is_named() {
            sib = s.prev_sibling();
            continue;
        }
        break;
    }
    None
}

/// C-family languages: look for `/** ... */`, `///`, or `//` comments preceding the node.
fn extract_block_or_line_comment(node: &Node, source: &str) -> Option<String> {
    let sibling = find_preceding_comment(node)?;
    let kind = sibling.kind();
    if kind != "comment" && kind != "line_comment" && kind != "block_comment" {
        return None;
    }
    let text = sibling.utf8_text(source.as_bytes()).ok()?;

    // JSDoc/JavaDoc style: /** ... */
    if text.starts_with("/**") {
        return Some(clean_block_comment(text));
    }
    // Triple-slash style: ///
    if text.starts_with("///") {
        let mut comments = vec![text.trim_start_matches("///").trim().to_string()];
        let mut sib = sibling.prev_named_sibling();
        while let Some(s) = sib {
            if s.kind() == "comment" || s.kind() == "line_comment" {
                let t = match s.utf8_text(source.as_bytes()) {
                    Ok(t) => t,
                    Err(_) => break,
                };
                if t.starts_with("///") {
                    comments.push(t.trim_start_matches("///").trim().to_string());
                    sib = s.prev_named_sibling();
                    continue;
                }
            }
            break;
        }
        comments.reverse();
        return Some(comments.join("\n"));
    }
    // Go-style: single-line // comments
    if text.starts_with("//") {
        let mut comments = vec![text.trim_start_matches("//").trim().to_string()];
        let mut sib = sibling.prev_named_sibling();
        while let Some(s) = sib {
            if s.kind() == "comment" {
                let t = match s.utf8_text(source.as_bytes()) {
                    Ok(t) => t,
                    Err(_) => break,
                };
                if t.starts_with("//") {
                    comments.push(t.trim_start_matches("//").trim().to_string());
                    sib = s.prev_named_sibling();
                    continue;
                }
            }
            break;
        }
        comments.reverse();
        return Some(comments.join("\n"));
    }

    None
}

/// Python: extract docstring from the first expression_statement in the body.
fn extract_python_docstring(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32)
            && child.kind() == "block"
                && let Some(first_stmt) = child.named_child(0)
                    && first_stmt.kind() == "expression_statement"
                        && let Some(str_node) = first_stmt.named_child(0)
                            && (str_node.kind() == "string"
                                || str_node.kind() == "concatenated_string")
                            {
                                let text = str_node.utf8_text(source.as_bytes()).ok()?;
                                return Some(clean_python_docstring(text));
                            }
    }
    None
}

/// Rust: look for `///` line comments preceding the node.
fn extract_rust_doc_comment(node: &Node, source: &str) -> Option<String> {
    let mut comments = Vec::new();
    let mut sibling = find_preceding_comment(node);
    while let Some(sib) = sibling {
        let kind = sib.kind();
        if kind == "line_comment" || kind == "comment" {
            let text = match sib.utf8_text(source.as_bytes()) {
                Ok(t) => t,
                Err(_) => break,
            };
            if text.starts_with("///") {
                comments.push(text.trim_start_matches("///").trim().to_string());
                sibling = find_preceding_comment(&sib);
                continue;
            }
        }
        break;
    }
    if comments.is_empty() {
        return None;
    }
    comments.reverse();
    Some(comments.join("\n"))
}

/// Ruby: look for `#` comments preceding the node.
fn extract_hash_comment(node: &Node, source: &str) -> Option<String> {
    let mut comments = Vec::new();
    let mut sibling = find_preceding_comment(node);
    while let Some(sib) = sibling {
        if sib.kind() == "comment" {
            let text = match sib.utf8_text(source.as_bytes()) {
                Ok(t) => t,
                Err(_) => break,
            };
            if text.starts_with('#') {
                comments.push(text.trim_start_matches('#').trim().to_string());
                sibling = find_preceding_comment(&sib);
                continue;
            }
        }
        break;
    }
    if comments.is_empty() {
        return None;
    }
    comments.reverse();
    Some(comments.join("\n"))
}

/// Clean a `/** ... */` block comment.
fn clean_block_comment(text: &str) -> String {
    let trimmed = text.trim_start_matches("/**").trim_end_matches("*/").trim();
    trimmed
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Clean a Python docstring (`"""..."""` or `'''...'''`).
fn clean_python_docstring(text: &str) -> String {
    let inner = text
        .trim_start_matches("\"\"\"")
        .trim_start_matches("'''")
        .trim_end_matches("\"\"\"")
        .trim_end_matches("'''")
        .trim();
    inner
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------


#[cfg(test)]
#[path = "language_behavior_tests.rs"]
mod tests;
