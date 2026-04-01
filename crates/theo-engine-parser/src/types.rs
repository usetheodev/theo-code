//! Core data types for the CodeModel intermediate representation.
//!
//! These types model a codebase at a semantic level — services, APIs,
//! dependencies, and observable sinks — rather than at the file/line level.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tree_sitter::SupportedLanguage;

// ---------------------------------------------------------------------------
// Resolution confidence types
// ---------------------------------------------------------------------------

/// How a reference target was resolved to a concrete file/symbol.
///
/// Ordered by decreasing confidence. Downstream consumers can filter
/// on resolution method to select only high-quality edges.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionMethod {
    /// Resolved via an explicit import statement (highest confidence).
    ImportBased,
    /// Resolved to a symbol in the same file.
    SameFile,
    /// Resolved to a globally unique symbol name.
    GlobalUnique,
    /// Multiple global matches — resolved to symbol in the same directory as the caller.
    GlobalSameDir,
    /// Multiple global matches — resolved to arbitrary first match.
    GlobalAmbiguous,
    /// Resolved via an explicit import of an external package symbol.
    ///
    /// The symbol was imported explicitly (e.g., `from torch.nn import Module`)
    /// and then referenced in the code (e.g., `Module(...)`). We know the
    /// developer's intent from the import statement, but the definition lives
    /// outside the project boundary.
    ///
    /// Higher confidence than `External` (unguided) because the import statement
    /// provides direct evidence of the symbol's origin.
    ImportKnown,
    /// Resolved as an external dependency (stdlib, third-party package).
    ///
    /// The import source was classified as a non-project dependency (no `./` or
    /// `../` prefix). The symbol lives outside the project boundary — in the
    /// language's standard library, a third-party package, or a system library.
    External,
    /// Could not be resolved to any known symbol.
    #[default]
    Unresolved,
}

impl ResolutionMethod {
    /// Machine-readable short label for this resolution method.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ImportBased => "import_based",
            Self::SameFile => "same_file",
            Self::GlobalUnique => "global_unique",
            Self::GlobalSameDir => "global_same_dir",
            Self::GlobalAmbiguous => "global_ambiguous",
            Self::ImportKnown => "import_known",
            Self::External => "external",
            Self::Unresolved => "unresolved",
        }
    }
}

/// Precise source location anchoring a semantic fact to the CST.
///
/// Every extracted artifact (route, dependency, sink, symbol, data model) carries
/// a `SourceAnchor` that captures the full tree-sitter node position. This enables:
/// - Code context retrieval (anchor → source text for LLMs)
/// - AST rewriting (anchor → exact node for deterministic patches)
/// - Stable navigation (byte offsets survive line-number drift)
///
/// See ADR-002 for design rationale.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceAnchor {
    /// File containing the anchored node.
    pub file: PathBuf,
    /// 1-based start line.
    pub line: usize,
    /// 1-based end line.
    pub end_line: usize,
    /// Byte offset of the node's first byte in the source file.
    pub start_byte: usize,
    /// Byte offset past the node's last byte in the source file.
    pub end_byte: usize,
    /// Tree-sitter CST node type (e.g. `"call_expression"`, `"decorator"`).
    pub node_kind: String,
}

impl SourceAnchor {
    /// Create a minimal anchor from a file and line number.
    ///
    /// Sets `end_line = line`, byte offsets to 0, and `node_kind` to empty.
    /// Useful in tests and consumers that lack tree-sitter node data.
    pub fn from_line(file: PathBuf, line: usize) -> Self {
        Self {
            file,
            line,
            end_line: line,
            start_byte: 0,
            end_byte: 0,
            node_kind: String::new(),
        }
    }

    /// Create an anchor with a line range but no byte-level data.
    ///
    /// Useful for Symbol and DataModel constructions in tests.
    pub fn from_line_range(file: PathBuf, line: usize, end_line: usize) -> Self {
        Self {
            file,
            line,
            end_line,
            start_byte: 0,
            end_byte: 0,
            node_kind: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// File role classification
// ---------------------------------------------------------------------------

/// Classification of a file's role in the project.
///
/// Used by downstream consumers for scoring, filtering, and token budgeting.
/// Roles are assigned via path/filename heuristics during file discovery.
///
/// Priority order: Generated > Test > Documentation > Build > Config > Implementation > Other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileRole {
    /// Source code implementing business logic or application features.
    Implementation,
    /// Test files: unit tests, integration tests, spec files.
    Test,
    /// Configuration files: YAML, TOML, JSON, .env, editor configs.
    Config,
    /// Documentation: Markdown, README, docs directories.
    Documentation,
    /// Generated or vendored code: protobuf output, node_modules, vendor.
    Generated,
    /// Build system files: Cargo.toml, Makefile, Dockerfile, lock files.
    Build,
    /// Files that don't match any other role.
    Other,
}

impl FileRole {
    /// Machine-readable short label for this role.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Implementation => "impl",
            Self::Test => "test",
            Self::Config => "config",
            Self::Documentation => "docs",
            Self::Generated => "generated",
            Self::Build => "build",
            Self::Other => "other",
        }
    }

    /// Classify a file's role based on its path using heuristic rules.
    ///
    /// The priority waterfall checks directory components and filename patterns
    /// to assign the most specific role. Generated/vendored files take highest
    /// priority to ensure they are never mis-classified as implementation.
    pub fn from_path(path: &Path) -> Self {
        let path_str = path.to_string_lossy();
        let file_name = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        // Generated directories (highest priority — never mis-classify as impl)
        if Self::path_contains_component(&path_str, "vendor")
            || Self::path_contains_component(&path_str, "node_modules")
            || Self::path_contains_component(&path_str, "generated")
        {
            return Self::Generated;
        }

        // Generated filename patterns
        if Self::is_generated_filename(&file_name) {
            return Self::Generated;
        }

        // Test directories
        if Self::path_contains_component(&path_str, "tests")
            || Self::path_contains_component(&path_str, "__tests__")
            || Self::path_contains_component(&path_str, "spec")
            || Self::path_contains_component(&path_str, "test")
        {
            return Self::Test;
        }

        // Test filename patterns
        if Self::is_test_filename(&file_name) {
            return Self::Test;
        }

        // Documentation directory
        if Self::path_contains_component(&path_str, "docs") {
            return Self::Documentation;
        }

        // Build files (exact filenames)
        if Self::is_build_filename(&file_name) {
            return Self::Build;
        }

        // Config files
        if Self::is_config_extension(ext) || Self::is_config_filename(&file_name) {
            return Self::Config;
        }

        // Documentation by extension
        if matches!(ext, "md" | "mdx" | "rst" | "adoc") {
            return Self::Documentation;
        }

        // Implementation: known programming language extensions
        if Self::is_programming_extension(ext) {
            return Self::Implementation;
        }

        Self::Other
    }

    /// Check if a path contains a specific directory component.
    ///
    /// Splits on `/` and `\` and compares exact component names to avoid
    /// false positives (e.g., "attestation" matching "test").
    fn path_contains_component(path_str: &str, component: &str) -> bool {
        path_str.split(['/', '\\']).any(|c| c == component)
    }

    fn is_test_filename(file_name: &str) -> bool {
        let lower = file_name.to_lowercase();
        lower.ends_with("_test.go")
            || lower.ends_with("_test.rs")
            || lower.ends_with("_spec.rs")
            || lower.ends_with("_spec.rb")
            || lower.ends_with("_test.py")
            || lower.ends_with("test.java")
            || lower.ends_with("test.cs")
            || lower.ends_with("test.kt")
            || lower.ends_with(".test.js")
            || lower.ends_with(".test.ts")
            || lower.ends_with(".test.tsx")
            || lower.ends_with(".test.jsx")
            || lower.ends_with(".spec.js")
            || lower.ends_with(".spec.ts")
            || lower.ends_with(".spec.tsx")
            || lower.ends_with(".spec.jsx")
            || (lower.starts_with("test_") && (lower.ends_with(".py") || lower.ends_with(".rb")))
    }

    fn is_generated_filename(file_name: &str) -> bool {
        let lower = file_name.to_lowercase();
        lower.contains(".generated.")
            || lower.ends_with(".pb.go")
            || lower.ends_with(".g.dart")
            || lower.ends_with(".g.cs")
    }

    fn is_build_filename(file_name: &str) -> bool {
        matches!(
            file_name,
            "Makefile"
                | "makefile"
                | "GNUmakefile"
                | "Cargo.toml"
                | "package.json"
                | "build.rs"
                | "build.gradle"
                | "build.gradle.kts"
                | "pom.xml"
                | "CMakeLists.txt"
                | "Dockerfile"
                | "docker-compose.yml"
                | "docker-compose.yaml"
                | "Rakefile"
                | "Gemfile"
                | "Justfile"
                | "justfile"
                | "go.mod"
                | "go.sum"
                | "setup.py"
                | "setup.cfg"
                | "pyproject.toml"
                | "Pipfile"
                | "Cargo.lock"
                | "package-lock.json"
                | "yarn.lock"
                | "pnpm-lock.yaml"
                | "flake.nix"
                | "composer.json"
        )
    }

    fn is_config_extension(ext: &str) -> bool {
        matches!(ext, "yaml" | "yml" | "ini" | "cfg" | "env")
    }

    fn is_config_filename(file_name: &str) -> bool {
        let lower = file_name.to_lowercase();
        lower.starts_with(".env")
            || matches!(
                file_name,
                ".gitignore"
                    | ".gitattributes"
                    | ".editorconfig"
                    | ".prettierrc"
                    | ".eslintrc"
                    | ".babelrc"
                    | "tsconfig.json"
                    | "rustfmt.toml"
                    | "clippy.toml"
                    | ".rustfmt.toml"
                    | ".clippy.toml"
                    | "deny.toml"
            )
    }

    fn is_programming_extension(ext: &str) -> bool {
        matches!(
            ext,
            "rs" | "go"
                | "py"
                | "pyi"
                | "js"
                | "mjs"
                | "cjs"
                | "ts"
                | "tsx"
                | "mts"
                | "cts"
                | "jsx"
                | "java"
                | "kt"
                | "kts"
                | "rb"
                | "c"
                | "h"
                | "cpp"
                | "cc"
                | "cxx"
                | "hpp"
                | "cs"
                | "swift"
                | "scala"
                | "sc"
                | "php"
                | "sh"
                | "bash"
                | "zsh"
                | "lua"
                | "ex"
                | "exs"
                | "hs"
                | "r"
                | "html"
                | "htm"
                | "css"
                | "scss"
                | "sass"
                | "less"
        )
    }
}

impl std::fmt::Display for FileRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Token budget
// ---------------------------------------------------------------------------

/// Token budget configuration for downstream consumers.
///
/// Enforces a byte or token limit on a set of scored results.
/// Uses the `bytes / 4` heuristic for token estimation (same approach
/// used by most LLM tooling).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Maximum total bytes across all selected files.
    pub max_bytes: Option<u64>,
    /// Maximum total estimated tokens across all selected files.
    pub max_tokens: Option<u64>,
}

/// Estimate token count from byte size using the `bytes / 4` heuristic.
///
/// This is the standard rough approximation used by LLM tooling.
/// Actual token counts vary by tokenizer, but this is sufficient
/// for budget enforcement and cost estimation.
pub fn estimate_tokens(byte_size: u64) -> u64 {
    byte_size / 4
}

/// The CodeModel: a semantic snapshot of the entire codebase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeModel {
    pub version: String,
    pub project_name: String,
    pub components: Vec<Component>,
    pub stats: CodeModelStats,
    // TODO: integrate with pipeline — FileTree from file_tree module
    // pub file_tree: Option<FileTree>,
}

impl CodeModel {
    /// Return a filtered copy containing only references at or above `min_confidence`.
    ///
    /// Filters:
    /// - `Component.references` — removes entries below threshold
    /// - `FileTree.directory_dependencies` — removes entries with `avg_confidence < min_confidence`
    /// - `CodeModelStats` — recalculates `total_references`, `resolved_references`,
    ///   `avg_resolution_confidence`
    pub fn filtered(&self, min_confidence: f64) -> Self {
        let mut model = self.clone();

        for component in &mut model.components {
            component
                .references
                .retain(|r| r.confidence >= min_confidence);
        }

        // Recalculate reference stats
        let total_refs: usize = model.components.iter().map(|c| c.references.len()).sum();
        let resolved: usize = model
            .components
            .iter()
            .flat_map(|c| c.references.iter())
            .filter(|r| r.confidence > 0.0)
            .count();
        let confidence_sum: f64 = model
            .components
            .iter()
            .flat_map(|c| c.references.iter())
            .map(|r| r.confidence)
            .sum();

        model.stats.total_references = total_refs;
        model.stats.resolved_references = resolved;
        model.stats.avg_resolution_confidence = if total_refs == 0 {
            0.0
        } else {
            confidence_sum / total_refs as f64
        };

        model
    }
}

/// A logical component (service, library, module) in the system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Component {
    pub name: String,
    pub language: SupportedLanguage,
    pub interfaces: Vec<Interface>,
    pub dependencies: Vec<Dependency>,
    pub sinks: Vec<Sink>,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<ImportInfo>,
    pub references: Vec<Reference>,
    pub data_models: Vec<DataModel>,
    pub module_boundaries: Vec<ModuleBoundary>,
    /// Environment variable references aggregated from all files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_dependencies: Vec<EnvDependency>,
}

/// Location of a route parameter within the HTTP request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParameterLocation {
    /// URL path segment (e.g., `/users/:id`).
    Path,
    /// URL query string (e.g., `?page=1`).
    Query,
    /// HTTP header.
    Header,
    /// Request body field.
    Body,
}

/// A parameter associated with a route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteParameter {
    pub name: String,
    pub location: ParameterLocation,
    /// Type annotation, if available (e.g., `"string"`, `"int"`).
    /// `None` for untyped frameworks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_type: Option<String>,
}

/// An HTTP endpoint exposed by a component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Interface {
    pub method: HttpMethod,
    pub path: String,
    pub auth: Option<AuthKind>,
    /// Source location of the route definition in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
    /// Route parameters extracted from the path pattern and framework decorators.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<RouteParameter>,
    /// Name of the handler function/method for this route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler_name: Option<String>,
    /// Type name of the request body (e.g., `"CreateUserDto"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body_type: Option<String>,
}

/// HTTP methods supported by the extractor.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
    All,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
            Self::Put => write!(f, "PUT"),
            Self::Patch => write!(f, "PATCH"),
            Self::Delete => write!(f, "DELETE"),
            Self::Options => write!(f, "OPTIONS"),
            Self::Head => write!(f, "HEAD"),
            Self::All => write!(f, "ALL"),
        }
    }
}

/// Kind of authentication detected on an endpoint.
///
/// Different frameworks express auth in different ways:
/// - Express/Gin/Rails: middleware functions in the route handler chain
/// - FastAPI/Flask/Django: decorators on route handler functions
/// - Spring Boot: annotations on controller methods
/// - ASP.NET Core: attributes on action methods
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    /// Express/Gin/Rails: `app.get('/x', authMiddleware, handler)`
    Middleware(String),
    /// Python: `@login_required`, `@jwt_required`
    Decorator(String),
    /// Java/Kotlin: `@PreAuthorize`, `@Secured`
    Annotation(String),
    /// C#: `[Authorize]`, `[Authorize(Roles="admin")]`
    Attribute(String),
}

/// An external dependency (HTTP call, DB connection, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    pub target: String,
    pub dependency_type: DependencyType,
    /// Source location of the dependency call in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
}

/// Type of external dependency.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DependencyType {
    HttpCall,
}

/// An environment variable reference detected in source code.
///
/// Captures the variable name and source location. Dynamic access
/// (e.g., `process.env[varName]`) is represented as `var_name: "<dynamic>"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvDependency {
    pub var_name: String,
    /// Source location of the env access in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
}

/// A logging or output sink detected in the source code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sink {
    pub sink_type: SinkType,
    /// Source location of the log/sink call in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
    pub text: String,
    pub contains_pii: bool,
}

/// Type of sink.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SinkType {
    Log,
}

/// Visibility/access modifier of a code symbol.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}

/// A code symbol (class, function, method, trait, etc.) extracted from source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Source location spanning the full symbol definition in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
    pub doc: Option<String>,
    /// Full signature text, e.g. `fn foo(x: i32) -> bool`.
    /// LLMs read these natively — structured params would add
    /// complexity for zero value.
    pub signature: Option<String>,
    /// Access modifier. `None` means the language default applies.
    pub visibility: Option<Visibility>,
    /// Enclosing class, module, trait, or impl block name.
    pub parent: Option<String>,
    /// Whether this symbol is a test function/method.
    ///
    /// Detected via language-specific patterns: naming conventions (`test_*` in
    /// Python, `Test*` in Go), annotations (`@Test` in Java), or attributes
    /// (`#[test]` in Rust, `[Fact]` in C#).
    ///
    /// **Limitation:** BDD-style `describe`/`it` blocks are call expressions,
    /// not function declarations — they are NOT marked as test symbols.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_test: bool,
}

/// Kind of code symbol.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Class,
    Function,
    Method,
    Module,
    Interface,
    Trait,
    Enum,
    Struct,
}

// ---------------------------------------------------------------------------
// Knowledge graph types
// ---------------------------------------------------------------------------

/// A reference between two symbols (call, extends, implements, etc.).
///
/// References form the edges of the knowledge graph, connecting symbols
/// across files and modules. The `source_symbol` is the origin (caller,
/// subclass) and `target_symbol` is the destination (callee, superclass).
///
/// Each reference carries a `confidence` score (0.0–1.0) and a
/// `resolution_method` indicating how the target was resolved.
/// Downstream consumers can filter low-confidence edges to reduce noise.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Reference {
    /// Enclosing symbol at the call/usage site (e.g., the function that
    /// contains a call expression). Empty string if at module level.
    pub source_symbol: String,
    /// File containing the reference site.
    pub source_file: PathBuf,
    /// 1-based line of the reference site.
    pub source_line: usize,
    /// Target symbol name (callee, parent type, imported name).
    pub target_symbol: String,
    /// File where the target is defined (`None` if external/unresolved).
    pub target_file: Option<PathBuf>,
    /// 1-based line of the target definition (`None` if unresolved).
    pub target_line: Option<usize>,
    /// What kind of relationship this reference represents.
    pub reference_kind: ReferenceKind,
    /// Confidence that this reference is correctly resolved (0.0–1.0).
    ///
    /// 0.0 = unresolved, 1.0 = certain. Import-based: 0.95, same-file: 0.90,
    /// global-unique: 0.80, global-ambiguous: 0.40.
    #[serde(default)]
    pub confidence: f64,
    /// How this reference's target was resolved.
    #[serde(default)]
    pub resolution_method: ResolutionMethod,
    /// Whether this reference crosses a test→production boundary.
    ///
    /// `true` when the source file has `FileRole::Test` and the target
    /// file does NOT have `FileRole::Test`. Enables downstream consumers
    /// to separate test coupling from production architecture.
    #[serde(default)]
    pub is_test_reference: bool,
}

/// Classification of a reference relationship.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// Function or method call (`foo()`, `obj.bar()`).
    Call,
    /// Class inheritance (`class Foo extends Bar`).
    Extends,
    /// Interface/trait implementation (`implements Baz`, `impl Trait for`).
    Implements,
    /// Type used as parameter, return type, or field type.
    TypeUsage,
    /// Import/require statement (`import { Foo } from './bar'`).
    Import,
}

/// A data model (class, struct, interface) with its fields.
///
/// Data models are the "nouns" of the system. Extracting them with
/// field-level detail lets LLMs understand data shapes without reading
/// the full source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataModel {
    /// Name of the type (e.g., `User`, `OrderItem`).
    pub name: String,
    /// What kind of data model this is.
    pub model_kind: DataModelKind,
    /// Fields/properties of the model.
    pub fields: Vec<FieldInfo>,
    /// Source location spanning the full type definition in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
    /// Parent type (extends/inherits from).
    pub parent_type: Option<String>,
    /// Implemented interfaces or traits.
    pub implemented_interfaces: Vec<String>,
}

/// Classification of a data model type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DataModelKind {
    Class,
    Struct,
    Interface,
    Trait,
    Enum,
    Record,
}

/// A single field within a data model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldInfo {
    /// Field name (e.g., `email`, `order_id`).
    pub name: String,
    /// Type annotation if present (e.g., `String`, `Option<i32>`).
    pub field_type: Option<String>,
    /// 1-based line number.
    pub line: usize,
    /// Access modifier if detected.
    pub visibility: Option<Visibility>,
}

/// A logical module boundary inferred from directory structure and exports.
///
/// Module boundaries help LLMs understand the high-level architecture:
/// which files belong together, what each module exports, and how
/// modules depend on each other.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleBoundary {
    /// Module name (typically the directory name).
    pub name: String,
    /// Files belonging to this module.
    pub files: Vec<PathBuf>,
    /// Public symbols exported by this module.
    pub exported_symbols: Vec<String>,
    /// Names of modules this one depends on (via imports).
    pub depends_on: Vec<String>,
}

// ---------------------------------------------------------------------------
// Aggregate statistics
// ---------------------------------------------------------------------------

/// Aggregate statistics about the code model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CodeModelStats {
    pub files_analyzed: usize,
    pub total_interfaces: usize,
    pub total_dependencies: usize,
    pub total_sinks: usize,
    pub total_symbols: usize,
    /// Number of import references (counted from `ReferenceKind::Import`).
    pub total_imports: usize,
    pub total_references: usize,
    pub total_data_models: usize,
    pub total_modules: usize,
    /// Number of references that were resolved to a concrete target.
    #[serde(default)]
    pub resolved_references: usize,
    /// Average confidence across all references (0.0 if no references).
    #[serde(default)]
    pub avg_resolution_confidence: f64,
    /// Breakdown of files by role (impl, test, config, etc.).
    #[serde(default)]
    pub file_roles: HashMap<String, usize>,
    /// Total estimated tokens across all analyzed files (bytes / 4 heuristic).
    #[serde(default)]
    pub total_estimated_tokens: u64,
    /// Total number of directories in the file tree.
    #[serde(default)]
    pub total_directories: usize,
    /// Number of symbols identified as test functions/methods.
    #[serde(default)]
    pub total_test_symbols: usize,
    /// Total number of environment variable references across all files.
    #[serde(default)]
    pub total_env_dependencies: usize,
    /// Breakdown of references by resolution method (import_based, same_file,
    /// global_unique, global_ambiguous, unresolved).
    #[serde(default)]
    pub resolution_method_distribution: HashMap<String, usize>,
    /// Repository-level git statistics (churn, authorship).
    /// `None` when the `git` feature is disabled or not in a git repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_stats: Option<GitStats>,
}

/// Extraction results from a single source file, prior to aggregation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileExtraction {
    pub file: PathBuf,
    pub language: SupportedLanguage,
    pub interfaces: Vec<Interface>,
    pub dependencies: Vec<Dependency>,
    pub sinks: Vec<Sink>,
    pub imports: Vec<ImportInfo>,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub data_models: Vec<DataModel>,
    /// Environment variable references found in this file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_dependencies: Vec<EnvDependency>,
    /// Classification of this file's role in the project.
    #[serde(default = "default_file_role")]
    pub file_role: FileRole,
    /// Estimated token count (source bytes / 4).
    #[serde(default)]
    pub estimated_tokens: u64,
    /// SHA-256 content hash for cache invalidation.
    /// `None` for extractions created before hashing was added.
    #[serde(default)]
    pub content_hash: Option<[u8; 32]>,
    /// Per-file git metadata (churn, authorship, last modified).
    /// `None` when the `git` feature is disabled or the file is not tracked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_metadata: Option<GitFileMetadata>,
}

fn default_file_role() -> FileRole {
    FileRole::Implementation
}

/// Serde helper for `#[serde(skip_serializing_if = "is_false")]`.
fn is_false(b: &bool) -> bool {
    !(*b)
}

// ---------------------------------------------------------------------------
// Git metadata types (defined unconditionally for deserialization compat)
// ---------------------------------------------------------------------------

/// Per-file git metadata for churn and ownership analysis.
///
/// Computed by walking commit history (up to 1000 commits) and aggregating
/// per-file statistics. Available only when the `git` feature is enabled
/// and the project is inside a git repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GitFileMetadata {
    /// Unix timestamp of the most recent commit touching this file.
    pub last_modified: Option<i64>,
    /// Name or email of the author of the most recent commit.
    pub last_author: Option<String>,
    /// Number of commits that modified this file (churn proxy).
    pub commit_count: usize,
    /// Number of distinct authors who modified this file.
    pub distinct_authors: usize,
}

/// Aggregate git statistics across the entire repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GitStats {
    /// Total distinct authors across all analyzed commits.
    pub total_authors: usize,
    /// Total commits walked (capped at 1000).
    pub total_commits: usize,
    /// Average number of commits per file.
    pub avg_commits_per_file: f64,
    /// Top 10 files by commit count (highest churn).
    pub hottest_files: Vec<(PathBuf, usize)>,
}

/// An import/require statement found in a source file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportInfo {
    pub source: String,
    pub specifiers: Vec<String>,
    pub line: usize,
    /// Alias mappings: local alias name → original imported name.
    ///
    /// Populated for `import X as Y` (alias "Y" → original "X") and
    /// `from pkg import Foo as Bar` (alias "Bar" → original "Foo").
    /// Empty when no aliases are used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<(String, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_model_round_trip_serialization() {
        let model = CodeModel {
            version: "1.0".into(),
            project_name: "test-project".into(),
            components: vec![Component {
                name: "test-service".into(),
                language: SupportedLanguage::TypeScript,
                interfaces: vec![Interface {
                    method: HttpMethod::Get,
                    path: "/api/health".into(),
                    auth: None,
                    anchor: SourceAnchor::from_line(PathBuf::from("src/index.ts"), 10),
                    parameters: vec![],
                    handler_name: None,
                    request_body_type: None,
                }],
                dependencies: vec![],
                sinks: vec![],
                symbols: vec![],
                imports: vec![],
                references: vec![],
                data_models: vec![],
                module_boundaries: vec![],
                env_dependencies: vec![],
            }],
            stats: CodeModelStats {
                files_analyzed: 1,
                total_interfaces: 1,
                total_dependencies: 0,
                total_sinks: 0,
                total_symbols: 0,
                total_imports: 0,
                total_references: 0,
                total_data_models: 0,
                total_modules: 0,
                resolved_references: 0,
                avg_resolution_confidence: 0.0,
                file_roles: HashMap::new(),
                total_estimated_tokens: 0,
                total_directories: 0,
                total_test_symbols: 0,
                total_env_dependencies: 0,
                resolution_method_distribution: HashMap::new(),
                git_stats: None,
            },
        };

        let json = serde_json::to_string(&model).unwrap();
        let deserialized: CodeModel = serde_json::from_str(&json).unwrap();
        assert_eq!(model, deserialized);
    }

    #[test]
    fn file_extraction_round_trip_serialization() {
        let extraction = FileExtraction {
            file: PathBuf::from("src/server.ts"),
            language: SupportedLanguage::TypeScript,
            interfaces: vec![Interface {
                method: HttpMethod::Post,
                path: "/api/users".into(),
                auth: Some(AuthKind::Middleware("authMiddleware".into())),
                anchor: SourceAnchor::from_line(PathBuf::from("src/server.ts"), 15),
                parameters: vec![],
                handler_name: None,
                request_body_type: None,
            }],
            dependencies: vec![Dependency {
                target: "fetch(\"https://api.example.com\")".into(),
                dependency_type: DependencyType::HttpCall,
                anchor: SourceAnchor::from_line(PathBuf::from("src/server.ts"), 20),
            }],
            sinks: vec![Sink {
                sink_type: SinkType::Log,
                anchor: SourceAnchor::from_line(PathBuf::from("src/server.ts"), 25),
                text: "console.log(user.email)".into(),
                contains_pii: true,
            }],
            imports: vec![ImportInfo {
                source: "express".into(),
                specifiers: vec!["express".into()],
                line: 1,
                aliases: vec![],
            }],
            symbols: vec![],
            references: vec![],
            data_models: vec![],
            env_dependencies: vec![],
            file_role: FileRole::Implementation,
            estimated_tokens: 250,
            content_hash: None,
            git_metadata: None,
        };

        let json = serde_json::to_string(&extraction).unwrap();
        let deserialized: FileExtraction = serde_json::from_str(&json).unwrap();
        assert_eq!(extraction, deserialized);
    }

    #[test]
    fn interface_with_auth_serialization() {
        let iface = Interface {
            method: HttpMethod::Delete,
            path: "/api/users/:id".into(),
            auth: Some(AuthKind::Middleware("jwtAuth".into())),
            anchor: SourceAnchor::from_line(PathBuf::from("routes.ts"), 42),
            parameters: vec![],
            handler_name: None,
            request_body_type: None,
        };

        let json = serde_json::to_string(&iface).unwrap();
        assert!(json.contains("DELETE"));
        assert!(json.contains("jwtAuth"));

        let deserialized: Interface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface, deserialized);
    }

    #[test]
    fn http_method_display() {
        assert_eq!(HttpMethod::Get.to_string(), "GET");
        assert_eq!(HttpMethod::Post.to_string(), "POST");
        assert_eq!(HttpMethod::Delete.to_string(), "DELETE");
    }

    #[test]
    fn auth_kind_decorator_serialization() {
        let iface = Interface {
            method: HttpMethod::Post,
            path: "/api/users".into(),
            auth: Some(AuthKind::Decorator("login_required".into())),
            anchor: SourceAnchor::from_line(PathBuf::from("views.py"), 10),
            parameters: vec![],
            handler_name: None,
            request_body_type: None,
        };

        let json = serde_json::to_string(&iface).unwrap();
        assert!(json.contains("login_required"));
        let deserialized: Interface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface, deserialized);
    }

    #[test]
    fn auth_kind_annotation_serialization() {
        let iface = Interface {
            method: HttpMethod::Get,
            path: "/api/orders".into(),
            auth: Some(AuthKind::Annotation("PreAuthorize".into())),
            anchor: SourceAnchor::from_line(PathBuf::from("OrderController.java"), 25),
            parameters: vec![],
            handler_name: None,
            request_body_type: None,
        };

        let json = serde_json::to_string(&iface).unwrap();
        assert!(json.contains("PreAuthorize"));
        let deserialized: Interface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface, deserialized);
    }

    #[test]
    fn auth_kind_attribute_serialization() {
        let iface = Interface {
            method: HttpMethod::Delete,
            path: "/api/items/{id}".into(),
            auth: Some(AuthKind::Attribute("Authorize".into())),
            anchor: SourceAnchor::from_line(PathBuf::from("ItemsController.cs"), 30),
            parameters: vec![],
            handler_name: None,
            request_body_type: None,
        };

        let json = serde_json::to_string(&iface).unwrap();
        assert!(json.contains("Authorize"));
        let deserialized: Interface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface, deserialized);
    }

    #[test]
    fn symbol_round_trip_with_all_fields() {
        let symbol = Symbol {
            name: "process_payment".into(),
            kind: SymbolKind::Method,
            anchor: SourceAnchor::from_line_range(PathBuf::from("src/payments.rs"), 42, 60),
            doc: Some("Process a payment transaction.".into()),
            signature: Some("pub fn process_payment(&self, amount: f64) -> Result<Receipt>".into()),
            visibility: Some(Visibility::Public),
            parent: Some("PaymentService".into()),
            is_test: false,
        };

        let json = serde_json::to_string(&symbol).unwrap();
        let deserialized: Symbol = serde_json::from_str(&json).unwrap();
        assert_eq!(symbol, deserialized);

        // Verify serde rename_all works
        assert!(json.contains("\"public\""));
        assert!(json.contains("\"method\""));
    }

    #[test]
    fn symbol_round_trip_with_none_fields() {
        let symbol = Symbol {
            name: "helper".into(),
            kind: SymbolKind::Function,
            anchor: SourceAnchor::from_line_range(PathBuf::from("utils.ts"), 1, 5),
            doc: None,
            signature: None,
            visibility: None,
            parent: None,
            is_test: false,
        };

        let json = serde_json::to_string(&symbol).unwrap();
        let deserialized: Symbol = serde_json::from_str(&json).unwrap();
        assert_eq!(symbol, deserialized);
    }

    #[test]
    fn visibility_all_variants_serialization() {
        for (vis, expected) in [
            (Visibility::Public, "\"public\""),
            (Visibility::Private, "\"private\""),
            (Visibility::Protected, "\"protected\""),
            (Visibility::Internal, "\"internal\""),
        ] {
            let json = serde_json::to_string(&vis).unwrap();
            assert_eq!(json, expected);
            let back: Visibility = serde_json::from_str(&json).unwrap();
            assert_eq!(vis, back);
        }
    }

    #[test]
    fn sink_with_pii_serialization() {
        let sink = Sink {
            sink_type: SinkType::Log,
            anchor: SourceAnchor::from_line(PathBuf::from("handler.ts"), 99),
            text: "logger.info(req.body.password)".into(),
            contains_pii: true,
        };

        let json = serde_json::to_string(&sink).unwrap();
        let deserialized: Sink = serde_json::from_str(&json).unwrap();
        assert_eq!(sink, deserialized);
        assert!(deserialized.contains_pii);
    }

    // --- Knowledge graph type tests ---

    #[test]
    fn reference_round_trip_serialization() {
        let reference = Reference {
            source_symbol: "handle_request".into(),
            source_file: PathBuf::from("src/handler.rs"),
            source_line: 42,
            target_symbol: "validate".into(),
            target_file: Some(PathBuf::from("src/validation.rs")),
            target_line: Some(10),
            reference_kind: ReferenceKind::Call,
            confidence: 0.0,
            resolution_method: ResolutionMethod::Unresolved,
            is_test_reference: false,
        };

        let json = serde_json::to_string(&reference).unwrap();
        assert!(json.contains("\"call\""));
        let deserialized: Reference = serde_json::from_str(&json).unwrap();
        assert_eq!(reference, deserialized);
    }

    #[test]
    fn reference_kind_all_variants_serialization() {
        for (kind, expected) in [
            (ReferenceKind::Call, "\"call\""),
            (ReferenceKind::Extends, "\"extends\""),
            (ReferenceKind::Implements, "\"implements\""),
            (ReferenceKind::TypeUsage, "\"type_usage\""),
            (ReferenceKind::Import, "\"import\""),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, expected);
            let back: ReferenceKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn reference_with_unresolved_target() {
        let reference = Reference {
            source_symbol: "main".into(),
            source_file: PathBuf::from("src/main.ts"),
            source_line: 5,
            target_symbol: "axios.get".into(),
            target_file: None,
            target_line: None,
            reference_kind: ReferenceKind::Call,
            confidence: 0.0,
            resolution_method: ResolutionMethod::Unresolved,
            is_test_reference: false,
        };

        let json = serde_json::to_string(&reference).unwrap();
        assert!(json.contains("null"));
        let deserialized: Reference = serde_json::from_str(&json).unwrap();
        assert_eq!(reference, deserialized);
    }

    #[test]
    fn data_model_round_trip_serialization() {
        let model = DataModel {
            name: "User".into(),
            model_kind: DataModelKind::Class,
            fields: vec![
                FieldInfo {
                    name: "id".into(),
                    field_type: Some("number".into()),
                    line: 3,
                    visibility: Some(Visibility::Public),
                },
                FieldInfo {
                    name: "email".into(),
                    field_type: Some("string".into()),
                    line: 4,
                    visibility: Some(Visibility::Private),
                },
            ],
            anchor: SourceAnchor::from_line_range(PathBuf::from("src/models/user.ts"), 2, 10),
            parent_type: Some("BaseEntity".into()),
            implemented_interfaces: vec!["Serializable".into()],
        };

        let json = serde_json::to_string(&model).unwrap();
        assert!(json.contains("\"class\""));
        assert!(json.contains("BaseEntity"));
        let deserialized: DataModel = serde_json::from_str(&json).unwrap();
        assert_eq!(model, deserialized);
    }

    #[test]
    fn data_model_kind_all_variants_serialization() {
        for (kind, expected) in [
            (DataModelKind::Class, "\"class\""),
            (DataModelKind::Struct, "\"struct\""),
            (DataModelKind::Interface, "\"interface\""),
            (DataModelKind::Trait, "\"trait\""),
            (DataModelKind::Enum, "\"enum\""),
            (DataModelKind::Record, "\"record\""),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, expected);
            let back: DataModelKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn module_boundary_round_trip_serialization() {
        let module = ModuleBoundary {
            name: "payments".into(),
            files: vec![
                PathBuf::from("src/payments/handler.ts"),
                PathBuf::from("src/payments/service.ts"),
            ],
            exported_symbols: vec!["PaymentService".into(), "processPayment".into()],
            depends_on: vec!["users".into(), "orders".into()],
        };

        let json = serde_json::to_string(&module).unwrap();
        let deserialized: ModuleBoundary = serde_json::from_str(&json).unwrap();
        assert_eq!(module, deserialized);
    }

    #[test]
    fn field_info_with_no_type_or_visibility() {
        let field = FieldInfo {
            name: "data".into(),
            field_type: None,
            line: 7,
            visibility: None,
        };

        let json = serde_json::to_string(&field).unwrap();
        let deserialized: FieldInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(field, deserialized);
    }

    // --- FileRole classification tests ---

    #[test]
    fn file_role_classifies_implementation() {
        assert_eq!(
            FileRole::from_path(Path::new("src/engine.rs")),
            FileRole::Implementation
        );
        assert_eq!(
            FileRole::from_path(Path::new("lib/server.ts")),
            FileRole::Implementation
        );
        assert_eq!(
            FileRole::from_path(Path::new("app/models/user.py")),
            FileRole::Implementation
        );
    }

    #[test]
    fn file_role_classifies_tests_by_directory() {
        assert_eq!(
            FileRole::from_path(Path::new("tests/integration.rs")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("src/__tests__/app.test.ts")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("spec/models/user_spec.rb")),
            FileRole::Test
        );
    }

    #[test]
    fn file_role_classifies_tests_by_filename() {
        assert_eq!(
            FileRole::from_path(Path::new("src/app.test.ts")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("src/app.spec.js")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("main_test.go")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("test_models.py")),
            FileRole::Test
        );
    }

    #[test]
    fn file_role_classifies_generated() {
        assert_eq!(
            FileRole::from_path(Path::new("vendor/lib.rs")),
            FileRole::Generated
        );
        assert_eq!(
            FileRole::from_path(Path::new("node_modules/express/index.js")),
            FileRole::Generated
        );
        assert_eq!(
            FileRole::from_path(Path::new("api.generated.ts")),
            FileRole::Generated
        );
        assert_eq!(
            FileRole::from_path(Path::new("service.pb.go")),
            FileRole::Generated
        );
    }

    #[test]
    fn file_role_classifies_build() {
        assert_eq!(
            FileRole::from_path(Path::new("Cargo.toml")),
            FileRole::Build
        );
        assert_eq!(
            FileRole::from_path(Path::new("package.json")),
            FileRole::Build
        );
        assert_eq!(
            FileRole::from_path(Path::new("Dockerfile")),
            FileRole::Build
        );
        assert_eq!(FileRole::from_path(Path::new("Makefile")), FileRole::Build);
    }

    #[test]
    fn file_role_classifies_config() {
        assert_eq!(
            FileRole::from_path(Path::new("config.yaml")),
            FileRole::Config
        );
        assert_eq!(
            FileRole::from_path(Path::new(".env.production")),
            FileRole::Config
        );
        assert_eq!(
            FileRole::from_path(Path::new(".gitignore")),
            FileRole::Config
        );
    }

    #[test]
    fn file_role_classifies_documentation() {
        assert_eq!(
            FileRole::from_path(Path::new("docs/architecture.md")),
            FileRole::Documentation
        );
        assert_eq!(
            FileRole::from_path(Path::new("README.md")),
            FileRole::Documentation
        );
    }

    #[test]
    fn file_role_generated_takes_priority_over_test() {
        // A test file inside vendor/ should be classified as Generated, not Test
        assert_eq!(
            FileRole::from_path(Path::new("vendor/pkg/handler_test.go")),
            FileRole::Generated
        );
    }

    #[test]
    fn file_role_display_matches_as_str() {
        for role in [
            FileRole::Implementation,
            FileRole::Test,
            FileRole::Config,
            FileRole::Documentation,
            FileRole::Generated,
            FileRole::Build,
            FileRole::Other,
        ] {
            assert_eq!(role.to_string(), role.as_str());
        }
    }

    #[test]
    fn file_role_serialization_round_trip() {
        for role in [
            FileRole::Implementation,
            FileRole::Test,
            FileRole::Config,
            FileRole::Documentation,
            FileRole::Generated,
            FileRole::Build,
            FileRole::Other,
        ] {
            let json = serde_json::to_string(&role).unwrap();
            let deserialized: FileRole = serde_json::from_str(&json).unwrap();
            assert_eq!(role, deserialized);
        }
    }

    // --- Token estimation tests ---

    #[test]
    fn estimate_tokens_divides_by_four() {
        assert_eq!(estimate_tokens(400), 100);
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(3), 0); // integer division rounds down
        assert_eq!(estimate_tokens(1000), 250);
    }

    // --- CodeModel::filtered() tests ---

    /// Helper: build a minimal CodeModel with the given references.
    fn model_with_refs(refs: Vec<Reference>) -> CodeModel {
        let total = refs.len();
        let resolved = refs.iter().filter(|r| r.confidence > 0.0).count();
        let conf_sum: f64 = refs.iter().map(|r| r.confidence).sum();
        let avg = if total == 0 {
            0.0
        } else {
            conf_sum / total as f64
        };

        CodeModel {
            version: "1.0".into(),
            project_name: "test".into(),
            components: vec![Component {
                name: "default".into(),
                language: SupportedLanguage::TypeScript,
                interfaces: vec![],
                dependencies: vec![],
                sinks: vec![],
                symbols: vec![],
                imports: vec![],
                references: refs,
                data_models: vec![],
                module_boundaries: vec![],
                env_dependencies: vec![],
            }],
            stats: CodeModelStats {
                total_references: total,
                resolved_references: resolved,
                avg_resolution_confidence: avg,
                ..Default::default()
            },
        }
    }

    fn test_ref(confidence: f64) -> Reference {
        Reference {
            source_symbol: "caller".into(),
            source_file: PathBuf::from("src/a.ts"),
            source_line: 1,
            target_symbol: "callee".into(),
            target_file: Some(PathBuf::from("src/b.ts")),
            target_line: Some(1),
            reference_kind: ReferenceKind::Call,
            confidence,
            resolution_method: if confidence > 0.0 {
                ResolutionMethod::ImportBased
            } else {
                ResolutionMethod::Unresolved
            },
            is_test_reference: false,
        }
    }

    #[test]
    fn filtered_removes_low_confidence_references() {
        let model = model_with_refs(vec![test_ref(0.95), test_ref(0.40), test_ref(0.0)]);

        let filtered = model.filtered(0.5);

        assert_eq!(
            filtered.components[0].references.len(),
            1,
            "only the 0.95 ref should survive a 0.5 threshold"
        );
        assert!((filtered.components[0].references[0].confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn filtered_preserves_high_confidence_references() {
        let model = model_with_refs(vec![test_ref(0.95), test_ref(0.80), test_ref(0.50)]);

        let filtered = model.filtered(0.5);

        assert_eq!(
            filtered.components[0].references.len(),
            3,
            "all refs at or above 0.5 should be kept"
        );
    }

    #[test]
    fn filtered_recalculates_stats() {
        let model = model_with_refs(vec![test_ref(0.95), test_ref(0.40), test_ref(0.0)]);

        let filtered = model.filtered(0.5);

        assert_eq!(filtered.stats.total_references, 1);
        assert_eq!(filtered.stats.resolved_references, 1);
        assert!((filtered.stats.avg_resolution_confidence - 0.95).abs() < f64::EPSILON);
    }

    // TODO: integrate with pipeline — test depends on FileTree from file_tree module

    #[test]
    fn filtered_zero_threshold_keeps_all() {
        let model = model_with_refs(vec![test_ref(0.95), test_ref(0.40), test_ref(0.0)]);

        let filtered = model.filtered(0.0);

        assert_eq!(
            filtered.components[0].references.len(),
            3,
            "threshold 0.0 should keep everything"
        );
    }

    #[test]
    fn is_test_reference_preserved_in_serde_round_trip() {
        let mut reference = test_ref(0.90);
        reference.is_test_reference = true;

        let json = serde_json::to_string(&reference).unwrap();
        assert!(json.contains("\"is_test_reference\":true"));
        let deserialized: Reference = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_test_reference);
    }

    #[test]
    fn is_test_reference_defaults_to_false_on_deserialize() {
        // Simulate JSON from before the field existed
        let json = r#"{
            "source_symbol":"f",
            "source_file":"a.ts",
            "source_line":1,
            "target_symbol":"g",
            "target_file":null,
            "target_line":null,
            "reference_kind":"call",
            "confidence":0.0,
            "resolution_method":"unresolved"
        }"#;

        let reference: Reference = serde_json::from_str(json).unwrap();
        assert!(
            !reference.is_test_reference,
            "missing field should default to false"
        );
    }

    #[test]
    fn resolution_method_as_str_returns_snake_case() {
        assert_eq!(ResolutionMethod::ImportBased.as_str(), "import_based");
        assert_eq!(ResolutionMethod::SameFile.as_str(), "same_file");
        assert_eq!(ResolutionMethod::GlobalUnique.as_str(), "global_unique");
        assert_eq!(ResolutionMethod::GlobalSameDir.as_str(), "global_same_dir");
        assert_eq!(
            ResolutionMethod::GlobalAmbiguous.as_str(),
            "global_ambiguous"
        );
        assert_eq!(ResolutionMethod::ImportKnown.as_str(), "import_known");
        assert_eq!(ResolutionMethod::External.as_str(), "external");
        assert_eq!(ResolutionMethod::Unresolved.as_str(), "unresolved");
    }
}
