//! `MemoryWikiBackend` — DIP interface for the *memory* wiki mount.
//!
//! Deliberately distinct from the existing `theo_domain::wiki_backend::WikiBackend`
//! (which models the code wiki: `insights`, `ingest`, `generate` over source
//! code). Memory wiki pages are produced by compacting past sessions and
//! lessons; their shape, hash manifest, and lint rules have nothing to do
//! with code ingestion. Keeping traits separate avoids overloading a single
//! surface with two unrelated lifecycles.
//!
//! Plan: `outputs/agent-memory-plan.md` §RM5a decision §"MemoryWikiBackend
//! trait separado".

use serde::{Deserialize, Serialize};

/// One entry in the compiled memory wiki. The layout mirrors Obsidian:
/// frontmatter (YAML) + markdown body, linked by `[[slug]]` shortcodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryWikiPage {
    /// Filesystem slug. Stable over time. Must match
    /// `^[a-z0-9][a-z0-9_-]*$` per lint rule.
    pub slug: String,
    /// Namespace: always `"memory"` for entries produced by RM5a/RM5b.
    /// Cross-namespace links (e.g. `[[code:foo]]`) reference pages in the
    /// separate code wiki mount.
    pub namespace: String,
    /// Human-readable title (frontmatter `title:`).
    pub title: String,
    /// Raw markdown body after the frontmatter block.
    pub body: String,
    /// Links discovered during parse: both in-namespace (`[[slug]]`) and
    /// cross-namespace (`[[ns:slug]]`).
    pub links: Vec<String>,
}

/// Errors surfaced by lint / hash manifest operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MemoryWikiLintError {
    /// Page body missing `namespace:` frontmatter entry.
    MissingNamespace { slug: String },
    /// `[[target]]` with no matching slug in either mount.
    BrokenLink { from: String, to: String },
    /// Slug uses forbidden chars.
    InvalidSlug { slug: String },
    /// Two pages share the same slug in the same namespace.
    DuplicateSlug { slug: String },
}

impl MemoryWikiLintError {
    pub fn describe(&self) -> String {
        match self {
            Self::MissingNamespace { slug } => {
                format!("page `{slug}` missing `namespace:` frontmatter")
            }
            Self::BrokenLink { from, to } => {
                format!("`{from}` links to missing page `{to}`")
            }
            Self::InvalidSlug { slug } => format!("invalid slug: `{slug}`"),
            Self::DuplicateSlug { slug } => format!("duplicate slug in namespace: `{slug}`"),
        }
    }
}

/// Backend trait for memory wiki operations. Implementations live in
/// `theo-infra-memory::wiki`.
#[async_trait::async_trait]
pub trait MemoryWikiBackend: Send + Sync {
    /// Unique identifier for logs / metrics (e.g. `"fs-memory"`).
    fn name(&self) -> &str;

    /// Return every compiled page in the memory namespace.
    async fn list_pages(&self) -> Vec<MemoryWikiPage>;

    /// Persist a page, overwriting any existing entry with the same slug.
    async fn upsert_page(&self, page: MemoryWikiPage) -> Result<(), String>;

    /// Run the full lint pass. Empty Vec = wiki is healthy.
    async fn lint(&self) -> Vec<MemoryWikiLintError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_error_describe_messages_are_human_readable() {
        let err = MemoryWikiLintError::MissingNamespace {
            slug: "foo".into(),
        };
        assert!(err.describe().contains("foo"));
        assert!(err.describe().contains("namespace"));
    }

    #[test]
    fn lint_error_broken_link_describes_both_ends() {
        let err = MemoryWikiLintError::BrokenLink {
            from: "a".into(),
            to: "b".into(),
        };
        let msg = err.describe();
        assert!(msg.contains("a"));
        assert!(msg.contains("b"));
    }

    #[test]
    fn memory_wiki_page_serde_roundtrip() {
        let page = MemoryWikiPage {
            slug: "lessons-learned".into(),
            namespace: "memory".into(),
            title: "Lessons Learned".into(),
            body: "hello [[code:theo-domain]]".into(),
            links: vec!["code:theo-domain".into()],
        };
        let j = serde_json::to_string(&page).unwrap();
        let back: MemoryWikiPage = serde_json::from_str(&j).unwrap();
        assert_eq!(back, page);
    }
}
