//! Hash-based incremental tracking.
//!
//! Computes SHA-256 of source files to detect changes.
//! Unchanged files = no LLM calls = no cost.
//! This is the Karpathy pattern: hash-based dirty flag.

use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::{WikiError, WikiResult};

/// Hash manifest — tracks source file hashes for incremental updates.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HashManifest {
    /// file_path → SHA-256 hex
    pub entries: HashMap<String, String>,
}

impl HashManifest {
    /// Compute SHA-256 of a file's contents.
    pub fn hash_file(path: &Path) -> WikiResult<String> {
        let content = std::fs::read(path).map_err(|e| WikiError::StoreFailed {
            path: path.display().to_string(),
            source: e,
        })?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Check if a file has changed since last manifest.
    pub fn is_dirty(&self, path: &str, current_hash: &str) -> bool {
        match self.entries.get(path) {
            Some(stored_hash) => stored_hash != current_hash,
            None => true, // new file = dirty
        }
    }

    /// Update the manifest with a new hash.
    pub fn update(&mut self, path: String, hash: String) {
        self.entries.insert(path, hash);
    }

    /// Load manifest from disk.
    pub fn load(path: &Path) -> WikiResult<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path).map_err(|e| WikiError::StoreFailed {
            path: path.display().to_string(),
            source: e,
        })?;
        serde_json::from_str(&content).map_err(|e| WikiError::HashCorrupted {
            reason: e.to_string(),
        })
    }

    /// Save manifest to disk (atomic write via temp + rename).
    pub fn save(&self, path: &Path) -> WikiResult<()> {
        let content = serde_json::to_string_pretty(self).map_err(|e| WikiError::HashCorrupted {
            reason: e.to_string(),
        })?;

        let dir = path.parent().unwrap_or(Path::new("."));
        let tmp = dir.join(".hashes.tmp");

        std::fs::write(&tmp, &content).map_err(|e| WikiError::StoreFailed {
            path: tmp.display().to_string(),
            source: e,
        })?;
        std::fs::rename(&tmp, path).map_err(|e| WikiError::StoreFailed {
            path: path.display().to_string(),
            source: e,
        })?;

        Ok(())
    }

    /// Count how many files are dirty (changed or new).
    pub fn count_dirty(&self, current_hashes: &HashMap<String, String>) -> usize {
        current_hashes
            .iter()
            .filter(|(path, hash)| self.is_dirty(path, hash))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_hash_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.rs");
        std::fs::write(&file, "fn main() {}").unwrap();

        let hash = HashManifest::hash_file(&file).unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn test_is_dirty_new_file() {
        let manifest = HashManifest::default();
        assert!(manifest.is_dirty("new_file.rs", "abc123"));
    }

    #[test]
    fn test_is_dirty_unchanged() {
        let mut manifest = HashManifest::default();
        manifest.update("file.rs".into(), "abc123".into());
        assert!(!manifest.is_dirty("file.rs", "abc123"));
    }

    #[test]
    fn test_is_dirty_changed() {
        let mut manifest = HashManifest::default();
        manifest.update("file.rs".into(), "old_hash".into());
        assert!(manifest.is_dirty("file.rs", "new_hash"));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".hashes.json");

        let mut manifest = HashManifest::default();
        manifest.update("a.rs".into(), "hash_a".into());
        manifest.update("b.rs".into(), "hash_b".into());
        manifest.save(&path).unwrap();

        let loaded = HashManifest::load(&path).unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries.get("a.rs").unwrap(), "hash_a");
    }

    #[test]
    fn test_count_dirty() {
        let mut manifest = HashManifest::default();
        manifest.update("unchanged.rs".into(), "same".into());
        manifest.update("changed.rs".into(), "old".into());

        let mut current = HashMap::new();
        current.insert("unchanged.rs".into(), "same".into());
        current.insert("changed.rs".into(), "new".into());
        current.insert("added.rs".into(), "brand_new".into());

        assert_eq!(manifest.count_dirty(&current), 2); // changed + added
    }
}
