//! Wiki page — the unit of knowledge in the wiki.
//!
//! Each page represents one module, decision, or concept.
//! Pages have two layers:
//! - **Skeleton** (free, from tree-sitter): files, symbols, APIs, deps
//! - **Enrichment** (LLM): what it does, why, how, what breaks

use serde::{Deserialize, Serialize};

/// A single wiki page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPage {
    /// URL-safe identifier (e.g., "theo-engine-retrieval")
    pub slug: String,

    /// Human-readable title
    pub title: String,

    /// Page category
    pub category: PageCategory,

    /// Skeleton data (free, from code graph)
    pub skeleton: SkeletonData,

    /// LLM-enriched content (the actual wiki text)
    pub enrichment: Option<EnrichmentData>,

    /// Content hash of the source files this page covers
    pub source_hash: String,

    /// When this page was last updated
    pub updated_at: String,

    /// Whether enrichment is stale (source changed since last enrichment)
    pub is_stale: bool,
}

/// Page categories
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PageCategory {
    /// A crate/module page
    Module,
    /// An architectural decision
    Decision,
    /// A cross-cutting concept
    Concept,
    /// The index/overview page
    Index,
}

/// Data extracted from code graph (free, no LLM needed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkeletonData {
    /// Files covered by this page
    pub files: Vec<String>,

    /// Public symbols (functions, structs, traits, enums)
    pub symbols: Vec<SymbolEntry>,

    /// Dependencies (what this module depends on)
    pub depends_on: Vec<String>,

    /// Dependents (what depends on this module)
    pub depended_by: Vec<String>,

    /// Primary language
    pub primary_language: String,

    /// Line count across all files
    pub total_lines: usize,
}

/// A symbol extracted from the code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolEntry {
    pub name: String,
    pub kind: String, // "fn", "struct", "trait", "enum", "impl"
    pub file: String,
    pub line: usize,
    pub signature: Option<String>,
    pub doc_first_line: Option<String>,
    pub is_public: bool,
}

/// LLM-generated enrichment (the valuable part)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentData {
    /// One-paragraph summary for the index page
    pub summary: String,

    /// What this module does (2-3 paragraphs)
    pub what_it_does: String,

    /// Why this module exists (motivation, problem it solves)
    pub why_it_exists: String,

    /// How it works (key algorithms, data flow)
    pub how_it_works: String,

    /// What breaks if you change it (invariants, dependents, gotchas)
    pub what_breaks: String,

    /// Key decisions and trade-offs
    pub decisions: Vec<String>,

    /// Cross-references to other wiki pages
    pub see_also: Vec<String>,

    /// Which LLM model generated this enrichment
    pub enriched_by: String,

    /// When enrichment was generated
    pub enriched_at: String,
}

impl WikiPage {
    /// Create a new page with skeleton only (no enrichment yet).
    pub fn from_skeleton(slug: String, title: String, category: PageCategory, skeleton: SkeletonData, source_hash: String) -> Self {
        Self {
            slug,
            title,
            category,
            skeleton,
            enrichment: None,
            source_hash,
            updated_at: chrono_now(),
            is_stale: true, // no enrichment = stale
        }
    }

    /// Check if this page needs re-enrichment.
    pub fn needs_enrichment(&self, current_source_hash: &str) -> bool {
        self.enrichment.is_none() || self.source_hash != current_source_hash
    }
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without chrono dependency
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_from_skeleton_is_stale() {
        let skeleton = SkeletonData {
            files: vec!["src/lib.rs".into()],
            symbols: vec![],
            depends_on: vec![],
            depended_by: vec![],
            primary_language: "rust".into(),
            total_lines: 100,
        };

        let page = WikiPage::from_skeleton(
            "test-module".into(),
            "Test Module".into(),
            PageCategory::Module,
            skeleton,
            "abc123".into(),
        );

        assert!(page.is_stale);
        assert!(page.enrichment.is_none());
        assert!(page.needs_enrichment("abc123"));
    }

    #[test]
    fn test_needs_enrichment_when_hash_changes() {
        let skeleton = SkeletonData {
            files: vec![],
            symbols: vec![],
            depends_on: vec![],
            depended_by: vec![],
            primary_language: "rust".into(),
            total_lines: 0,
        };

        let mut page = WikiPage::from_skeleton(
            "m".into(),
            "M".into(),
            PageCategory::Module,
            skeleton,
            "hash1".into(),
        );

        page.enrichment = Some(EnrichmentData {
            summary: "test".into(),
            what_it_does: "test".into(),
            why_it_exists: "test".into(),
            how_it_works: "test".into(),
            what_breaks: "test".into(),
            decisions: vec![],
            see_also: vec![],
            enriched_by: "test-model".into(),
            enriched_at: "now".into(),
        });
        page.is_stale = false;

        // Same hash → no re-enrichment needed
        assert!(!page.needs_enrichment("hash1"));

        // Different hash → re-enrichment needed
        assert!(page.needs_enrichment("hash2"));
    }
}
