//! Skeleton extraction — free structural data from the code graph.
//!
//! The skeleton is everything we can know WITHOUT calling an LLM:
//! files, symbols, APIs, dependencies, module groupings.
//! Extracted from theo-engine-graph (Tree-Sitter based).
//!
//! This is the foundation that enrichment builds on.

use crate::page::{PageCategory, SkeletonData, SymbolEntry, WikiPage};
use crate::hash::HashManifest;
use std::path::Path;

/// Extract skeleton data for a module (crate/directory).
///
/// Given a list of source files and their parsed symbols,
/// produces a `SkeletonData` with everything a human needs
/// to understand the module's structure — without LLM.
pub fn extract_skeleton(
    _module_name: &str,
    files: &[String],
    symbols: &[SymbolEntry],
    depends_on: &[String],
    depended_by: &[String],
    primary_language: &str,
) -> SkeletonData {
    let total_lines: usize = files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .map(|content| content.lines().count())
        .sum();

    SkeletonData {
        files: files.to_vec(),
        symbols: symbols.to_vec(),
        depends_on: depends_on.to_vec(),
        depended_by: depended_by.to_vec(),
        primary_language: primary_language.to_string(),
        total_lines,
    }
}

/// Compute source hash for a set of files.
///
/// Hash of all file hashes combined — changes when ANY file changes.
pub fn compute_source_hash(files: &[String]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    let mut sorted_files = files.to_vec();
    sorted_files.sort(); // deterministic order

    for file in &sorted_files {
        match HashManifest::hash_file(Path::new(file)) {
            Ok(h) => hasher.update(h.as_bytes()),
            Err(_) => hasher.update(b"MISSING"),
        }
    }

    format!("{:x}", hasher.finalize())
}

/// Create a wiki page from skeleton data (no enrichment yet).
pub fn page_from_skeleton(
    slug: &str,
    title: &str,
    category: PageCategory,
    skeleton: SkeletonData,
    source_hash: String,
) -> WikiPage {
    WikiPage::from_skeleton(
        slug.to_string(),
        title.to_string(),
        category,
        skeleton,
        source_hash,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_skeleton_empty() {
        let skeleton = extract_skeleton("test", &[], &[], &[], &[], "rust");
        assert_eq!(skeleton.total_lines, 0);
        assert!(skeleton.files.is_empty());
        assert!(skeleton.symbols.is_empty());
    }

    #[test]
    fn test_compute_source_hash_deterministic() {
        // Same files in different order should produce same hash
        let files_a = vec!["a.rs".to_string(), "b.rs".to_string()];
        let files_b = vec!["b.rs".to_string(), "a.rs".to_string()];

        let hash_a = compute_source_hash(&files_a);
        let hash_b = compute_source_hash(&files_b);
        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn test_page_from_skeleton_is_stale() {
        let skeleton = extract_skeleton("mod", &[], &[], &[], &[], "rust");
        let page = page_from_skeleton("mod", "Mod", PageCategory::Module, skeleton, "hash".into());
        assert!(page.is_stale);
        assert!(page.enrichment.is_none());
    }
}
