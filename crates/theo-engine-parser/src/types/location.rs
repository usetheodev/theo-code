//! Core data types for the CodeModel intermediate representation.
//!
//! These types model a codebase at a semantic level — services, APIs,
//! dependencies, and observable sinks — rather than at the file/line level.
#![allow(unused_imports, dead_code)]

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
