//! Aggregation helpers over `FileMemoryLookup` (the trait itself lives
//! in `theo-domain::memory` per the hexagonal architecture).
//!
//! Phase 0 / T0.2 of the `wiki-graph-memory-blend-retrieval-plan`.
//! `entries_from_providers` deduplicates and caps results across a set
//! of registered backends.

use std::collections::HashSet;

use theo_domain::memory::{FileMemoryLookup, MemoryEntry};

/// Aggregate over a set of registered `FileMemoryLookup` providers,
/// deduplicating across them.
///
/// Dedup key: `(provider_id, source, content_hash)` — same source
/// reporting the same content twice is one entry; different providers
/// reporting the same content (e.g. wiki insight + reflection log) are
/// independent.
pub fn entries_from_providers(
    providers: &[&dyn FileMemoryLookup],
    paths: &[String],
    cap: usize,
) -> Vec<MemoryEntry> {
    if paths.is_empty() || providers.is_empty() || cap == 0 {
        return Vec::new();
    }
    let mut seen: HashSet<(String, String, u64)> = HashSet::new();
    let mut out: Vec<MemoryEntry> = Vec::new();
    for prov in providers {
        for entry in prov.entries_for_files(paths) {
            let hash = simple_hash(&entry.content);
            let key = (prov.id().to_string(), entry.source.clone(), hash);
            if seen.insert(key) {
                out.push(entry);
                if out.len() >= cap {
                    return out;
                }
            }
        }
    }
    out
}

fn simple_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal in-memory lookup used by tests.
    struct StaticLookup {
        id: String,
        by_path: std::collections::HashMap<String, Vec<MemoryEntry>>,
    }

    impl StaticLookup {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                by_path: std::collections::HashMap::new(),
            }
        }
        fn add(&mut self, path: &str, entry: MemoryEntry) {
            self.by_path.entry(path.to_string()).or_default().push(entry);
        }
    }

    impl FileMemoryLookup for StaticLookup {
        fn id(&self) -> &str {
            &self.id
        }
        fn entries_for_files(&self, paths: &[String]) -> Vec<MemoryEntry> {
            let mut out = Vec::new();
            for p in paths {
                if let Some(es) = self.by_path.get(p) {
                    out.extend(es.iter().cloned());
                }
            }
            out
        }
    }

    fn make_entry(source: &str, content: &str) -> MemoryEntry {
        MemoryEntry {
            source: source.to_string(),
            content: content.to_string(),
            relevance_score: 1.0,
        }
    }

    #[test]
    fn test_empty_paths_returns_empty_without_iterating_providers() {
        let lookup = StaticLookup::new("lookup-1");
        let providers: Vec<&dyn FileMemoryLookup> = vec![&lookup];
        let out = entries_from_providers(&providers, &[], 10);
        assert!(out.is_empty());
    }

    #[test]
    fn test_finds_entry_by_path() {
        let mut lookup = StaticLookup::new("lookup-1");
        lookup.add("src/foo.rs", make_entry("insight", "foo had a race"));
        let providers: Vec<&dyn FileMemoryLookup> = vec![&lookup];
        let out = entries_from_providers(&providers, &["src/foo.rs".to_string()], 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].content, "foo had a race");
    }

    #[test]
    fn test_unknown_path_yields_empty() {
        let lookup = StaticLookup::new("lookup-1");
        let providers: Vec<&dyn FileMemoryLookup> = vec![&lookup];
        let out = entries_from_providers(&providers, &["nonexistent.rs".to_string()], 10);
        assert!(out.is_empty());
    }

    #[test]
    fn test_dedups_same_content_within_provider() {
        let mut lookup = StaticLookup::new("lookup-1");
        let entry = make_entry("insight", "duplicate content");
        lookup.add("src/foo.rs", entry.clone());
        lookup.add("src/foo.rs", entry); // same path, same entry → dedup
        let providers: Vec<&dyn FileMemoryLookup> = vec![&lookup];
        let out = entries_from_providers(&providers, &["src/foo.rs".to_string()], 10);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn test_does_not_dedup_across_providers() {
        let mut a = StaticLookup::new("provider-a");
        a.add("src/foo.rs", make_entry("insight", "shared content"));
        let mut b = StaticLookup::new("provider-b");
        b.add("src/foo.rs", make_entry("insight", "shared content"));
        let providers: Vec<&dyn FileMemoryLookup> = vec![&a, &b];
        let out = entries_from_providers(&providers, &["src/foo.rs".to_string()], 10);
        // Same content, different provider id → 2 entries.
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn test_cap_truncates_output() {
        let mut lookup = StaticLookup::new("lookup-1");
        for i in 0..30 {
            lookup.add(
                "src/foo.rs",
                make_entry("insight", &format!("entry-{i}")),
            );
        }
        let providers: Vec<&dyn FileMemoryLookup> = vec![&lookup];
        let out = entries_from_providers(&providers, &["src/foo.rs".to_string()], 5);
        assert_eq!(out.len(), 5);
    }

    #[test]
    fn test_zero_cap_returns_empty() {
        let mut lookup = StaticLookup::new("lookup-1");
        lookup.add("src/foo.rs", make_entry("insight", "x"));
        let providers: Vec<&dyn FileMemoryLookup> = vec![&lookup];
        let out = entries_from_providers(&providers, &["src/foo.rs".to_string()], 0);
        assert!(out.is_empty());
    }

    #[test]
    fn test_no_providers_yields_empty() {
        let providers: Vec<&dyn FileMemoryLookup> = vec![];
        let out = entries_from_providers(&providers, &["src/foo.rs".to_string()], 10);
        assert!(out.is_empty());
    }

    #[test]
    fn test_trait_object_safe() {
        // Compile-time check: must be callable behind &dyn.
        let lookup: Box<dyn FileMemoryLookup> = Box::new(StaticLookup::new("dyn"));
        assert_eq!(lookup.id(), "dyn");
    }
}
