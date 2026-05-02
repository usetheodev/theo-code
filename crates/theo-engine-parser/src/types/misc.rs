//! Single-purpose slice extracted from `types.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tree_sitter::SupportedLanguage;

// ---------------------------------------------------------------------------

use super::*;

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
pub fn is_false(b: &bool) -> bool {
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
