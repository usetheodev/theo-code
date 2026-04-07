//! Canonical data model for wiki pages (IR layer).
//!
//! Separates structured data from markdown rendering.
//! Every claim has provenance (SourceRef) tracing back to file + symbol + lines.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// Source provenance: traces a wiki claim back to code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub file_path: String,
    pub symbol_name: Option<String>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
}

impl SourceRef {
    pub fn file(path: &str) -> Self {
        SourceRef {
            file_path: path.to_string(),
            symbol_name: None,
            line_start: None,
            line_end: None,
        }
    }

    pub fn symbol(path: &str, name: &str, start: Option<usize>, end: Option<usize>) -> Self {
        SourceRef {
            file_path: path.to_string(),
            symbol_name: Some(name.to_string()),
            line_start: start,
            line_end: end,
        }
    }

    /// Format as `file.rs:10-30` for display.
    pub fn display(&self) -> String {
        let mut s = self.file_path.clone();
        if let Some(start) = self.line_start {
            s += &format!(":{}", start);
            if let Some(end) = self.line_end {
                s += &format!("-{}", end);
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Page sections
// ---------------------------------------------------------------------------

/// A file listed in the wiki page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub symbol_count: usize,
    pub source_ref: SourceRef,
}

/// A public API symbol (entry point or exported function/struct/trait).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEntry {
    pub name: String,
    pub signature: String,
    pub doc: Option<String>,
    pub kind: String, // Function, Method, Struct, Trait, Enum
    pub source_ref: SourceRef,
}

/// A cross-community dependency link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepEntry {
    pub target_slug: String,
    pub target_name: String,
    pub edge_type: String, // Imports, Calls, TypeDepends
}

/// A step in a call flow (A calls B).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStep {
    pub from_symbol: String,
    pub to_symbol: String,
    pub edge_type: String,
    pub source_ref: SourceRef,
}

/// Test coverage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCoverage {
    pub tested: usize,
    pub total: usize,
    pub percentage: f64,
    pub untested: Vec<String>,
}

// ---------------------------------------------------------------------------
// WikiDoc (canonical IR for one page)
// ---------------------------------------------------------------------------

/// A single wiki document representing one community/module.
///
/// Every section carries provenance via SourceRef.
/// This struct is the canonical IR — rendering to markdown is a separate step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiDoc {
    pub slug: String,
    pub title: String,
    pub community_id: String,

    // Summary stats
    pub file_count: usize,
    pub symbol_count: usize,
    pub primary_language: String,

    // Sections (all with provenance)
    pub files: Vec<FileEntry>,
    pub entry_points: Vec<ApiEntry>,
    pub public_api: Vec<ApiEntry>,
    pub dependencies: Vec<DepEntry>,
    pub call_flow: Vec<FlowStep>,
    pub test_coverage: TestCoverage,

    // Aggregate provenance
    pub source_refs: Vec<SourceRef>,

    // Metadata
    pub generated_at: String,
    pub enriched: bool,
}

// ---------------------------------------------------------------------------
// Wiki (complete output)
// ---------------------------------------------------------------------------

/// The complete wiki: all pages + manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wiki {
    pub docs: Vec<WikiDoc>,
    pub manifest: WikiManifest,
}

/// Manifest for cache invalidation and versioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiManifest {
    /// Bump on breaking template changes.
    pub schema_version: u32,
    /// Generator identifier.
    pub generator_version: String,
    /// Hash of graph state (file paths + mtimes).
    pub graph_hash: u64,
    /// ISO 8601 timestamp.
    pub generated_at: String,
    /// Number of pages generated.
    pub page_count: usize,
}

impl WikiManifest {
    pub const SCHEMA_VERSION: u32 = 1;
    pub const GENERATOR_VERSION: &'static str = "wiki-bootstrap-v1";
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ref_display_file_only() {
        let sr = SourceRef::file("src/main.rs");
        assert_eq!(sr.display(), "src/main.rs");
    }

    #[test]
    fn source_ref_display_with_lines() {
        let sr = SourceRef::symbol("src/auth.rs", "verify_token", Some(10), Some(30));
        assert_eq!(sr.display(), "src/auth.rs:10-30");
    }

    #[test]
    fn source_ref_display_start_only() {
        let sr = SourceRef {
            file_path: "lib.rs".into(),
            symbol_name: None,
            line_start: Some(5),
            line_end: None,
        };
        assert_eq!(sr.display(), "lib.rs:5");
    }

    #[test]
    fn test_coverage_default() {
        let tc = TestCoverage {
            tested: 0,
            total: 0,
            percentage: 0.0,
            untested: vec![],
        };
        assert_eq!(tc.percentage, 0.0);
    }
}
