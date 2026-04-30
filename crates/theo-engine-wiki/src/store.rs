//! Wiki store — persistence layer for wiki pages.
//!
//! Pages stored as JSON files on disk under `.theo/wiki/`.
//! Atomic writes via temp + rename to prevent corruption.
//! No database — just files, like the Karpathy Wiki pattern.

use crate::error::{WikiError, WikiResult};
use crate::page::WikiPage;
use std::path::{Path, PathBuf};

/// On-disk wiki store.
pub struct WikiStore {
    /// Root directory for wiki pages (e.g., `.theo/wiki/`)
    root: PathBuf,
}

impl WikiStore {
    /// Open or create a wiki store at the given path.
    pub fn open(root: impl Into<PathBuf>) -> WikiResult<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|e| WikiError::StoreFailed {
            path: root.display().to_string(),
            source: e,
        })?;
        Ok(Self { root })
    }

    /// Root directory path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path to a page's JSON file.
    fn page_path(&self, slug: &str) -> PathBuf {
        self.root.join(format!("{slug}.json"))
    }

    /// Path to the hash manifest.
    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(".hashes.json")
    }

    /// Load a page by slug. Returns None if not found.
    pub fn load_page(&self, slug: &str) -> WikiResult<Option<WikiPage>> {
        let path = self.page_path(slug);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path).map_err(|e| WikiError::StoreFailed {
            path: path.display().to_string(),
            source: e,
        })?;
        let page: WikiPage =
            serde_json::from_str(&content).map_err(|e| WikiError::StoreFailed {
                path: path.display().to_string(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            })?;
        Ok(Some(page))
    }

    /// Save a page (atomic write: temp file + rename).
    pub fn save_page(&self, page: &WikiPage) -> WikiResult<()> {
        let path = self.page_path(&page.slug);
        let tmp = self.root.join(format!(".{}.tmp", page.slug));

        let content = serde_json::to_string_pretty(page).map_err(|e| WikiError::StoreFailed {
            path: path.display().to_string(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        })?;

        std::fs::write(&tmp, &content).map_err(|e| WikiError::StoreFailed {
            path: tmp.display().to_string(),
            source: e,
        })?;
        std::fs::rename(&tmp, &path).map_err(|e| WikiError::StoreFailed {
            path: path.display().to_string(),
            source: e,
        })?;

        Ok(())
    }

    /// Delete a page.
    pub fn delete_page(&self, slug: &str) -> WikiResult<()> {
        let path = self.page_path(slug);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| WikiError::StoreFailed {
                path: path.display().to_string(),
                source: e,
            })?;
        }
        Ok(())
    }

    /// List all page slugs in the store.
    pub fn list_slugs(&self) -> WikiResult<Vec<String>> {
        let mut slugs = Vec::new();
        let entries = std::fs::read_dir(&self.root).map_err(|e| WikiError::StoreFailed {
            path: self.root.display().to_string(),
            source: e,
        })?;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".json") && !name.starts_with('.') {
                slugs.push(name.trim_end_matches(".json").to_string());
            }
        }

        slugs.sort();
        Ok(slugs)
    }

    /// Count total pages.
    pub fn page_count(&self) -> WikiResult<usize> {
        self.list_slugs().map(|s| s.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{PageCategory, SkeletonData};

    fn make_test_page(slug: &str) -> WikiPage {
        WikiPage::from_skeleton(
            slug.to_string(),
            format!("Test {slug}"),
            PageCategory::Module,
            SkeletonData {
                files: vec!["src/lib.rs".into()],
                symbols: vec![],
                depends_on: vec![],
                depended_by: vec![],
                primary_language: "rust".into(),
                total_lines: 42,
            },
            "test_hash".into(),
        )
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = WikiStore::open(dir.path().join("wiki")).unwrap();

        let page = make_test_page("my-module");
        store.save_page(&page).unwrap();

        let loaded = store.load_page("my-module").unwrap().unwrap();
        assert_eq!(loaded.slug, "my-module");
        assert_eq!(loaded.title, "Test my-module");
        assert_eq!(loaded.skeleton.total_lines, 42);
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = WikiStore::open(dir.path().join("wiki")).unwrap();

        let result = store.load_page("does-not-exist").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_slugs() {
        let dir = tempfile::tempdir().unwrap();
        let store = WikiStore::open(dir.path().join("wiki")).unwrap();

        store.save_page(&make_test_page("alpha")).unwrap();
        store.save_page(&make_test_page("beta")).unwrap();
        store.save_page(&make_test_page("gamma")).unwrap();

        let slugs = store.list_slugs().unwrap();
        assert_eq!(slugs, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn test_delete_page() {
        let dir = tempfile::tempdir().unwrap();
        let store = WikiStore::open(dir.path().join("wiki")).unwrap();

        store.save_page(&make_test_page("to-delete")).unwrap();
        assert_eq!(store.page_count().unwrap(), 1);

        store.delete_page("to-delete").unwrap();
        assert_eq!(store.page_count().unwrap(), 0);
    }
}
