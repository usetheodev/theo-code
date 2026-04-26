//! Pluggable document sources for `docs_search` (T15.1).
//!
//! A `DocSource` produces `DocEntry`s that get loaded into the
//! [`DocsIndex`]. Real-world sources (crates.io, MDN, npm,
//! ReadTheDocs) need network + caching + HTML cleaning — those are
//! deferred. This module ships:
//!
//! - The trait surface (`DocSource`).
//! - `StaticDocSource`: a hard-coded test corpus that lets us test
//!   the index + tool integration WITHOUT network.
//!
//! Future implementors live alongside (e.g., `CratesIoSource`,
//! `MdnSource`) and can be feature-gated.

use std::collections::HashMap;

use crate::docs_search::index::DocEntry;

/// Trait for fetching documents to index.
///
/// `name()` is the short label used in `DocEntry.source` and as the
/// CLI/tool source filter (e.g., "crates.io").
pub trait DocSource: Send + Sync {
    /// Stable name of this source (e.g. "crates.io").
    fn name(&self) -> &str;

    /// Fetch a documentation entry by `key` (interpretation depends
    /// on the source — for crates.io it's `crate-name`, for MDN it's
    /// the API path). Returns `None` when nothing matches.
    fn fetch(&self, key: &str) -> Option<DocEntry>;

    /// Bulk export of every available document. Used for full-index
    /// rebuilds. Default empty so simple sources can opt out.
    fn list_all(&self) -> Vec<DocEntry> {
        Vec::new()
    }
}

/// Static, in-memory source. Useful for tests and for shipping a
/// curated baseline corpus before network sources are wired up.
pub struct StaticDocSource {
    name: String,
    docs: HashMap<String, DocEntry>,
}

impl StaticDocSource {
    /// Build a static source with the given label and documents
    /// keyed by `key` (the lookup string passed to `fetch`).
    pub fn new(name: impl Into<String>, docs: HashMap<String, DocEntry>) -> Self {
        Self {
            name: name.into(),
            docs,
        }
    }

    /// Builder helper: starts empty, append via `with_doc`.
    pub fn empty(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            docs: HashMap::new(),
        }
    }

    /// Fluent insert.
    #[must_use]
    pub fn with_doc(mut self, key: impl Into<String>, doc: DocEntry) -> Self {
        self.docs.insert(key.into(), doc);
        self
    }
}

impl DocSource for StaticDocSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetch(&self, key: &str) -> Option<DocEntry> {
        self.docs.get(key).cloned()
    }

    fn list_all(&self) -> Vec<DocEntry> {
        self.docs.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, src: &str) -> DocEntry {
        DocEntry {
            id: id.into(),
            source: src.into(),
            language: "rust".into(),
            title: format!("Title of {id}"),
            url: format!("https://example.com/{id}"),
            body: format!("Body content for {id}"),
        }
    }

    #[test]
    fn t151_static_source_name_returned() {
        let src = StaticDocSource::empty("test-source");
        assert_eq!(src.name(), "test-source");
    }

    #[test]
    fn t151_static_source_fetch_returns_inserted_doc() {
        let src = StaticDocSource::empty("s").with_doc("vec", doc("vec_id", "s"));
        let r = src.fetch("vec");
        assert_eq!(r.unwrap().id, "vec_id");
    }

    #[test]
    fn t151_static_source_fetch_returns_none_for_unknown_key() {
        let src = StaticDocSource::empty("s");
        assert!(src.fetch("any").is_none());
    }

    #[test]
    fn t151_static_source_list_all_yields_all_docs() {
        let src = StaticDocSource::empty("s")
            .with_doc("a", doc("a", "s"))
            .with_doc("b", doc("b", "s"));
        let all = src.list_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn t151_static_source_with_hashmap_constructor() {
        let mut map = HashMap::new();
        map.insert("k".to_string(), doc("d", "s"));
        let src = StaticDocSource::new("s", map);
        assert_eq!(src.fetch("k").unwrap().id, "d");
    }
}
