//! Hash manifest for incremental memory-wiki compilation.
//!
//! The compiler (RM5b) consults `HashManifest::is_dirty` on every source
//! (lesson / journal file / etc.) before invoking the LLM. Unchanged
//! sources → zero work. Changed SHA → marker persisted so the next run
//! knows to regenerate downstream pages.
//!
//! Plan: `outputs/agent-memory-plan.md` §RM5a.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use theo_domain::memory::MemoryError;

use crate::fs_util::atomic_write;

/// Record for a single source file's SHA + last-compile timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceHash {
    pub sha256_hex: String,
    pub last_compile_unix: u64,
}

/// Full manifest — keyed by stable source id (e.g. `"lesson:l-0042"` or
/// `"journal:2026-04-20"`). Persisted as JSON at
/// `.theo/wiki/memory/.hashes.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HashManifest {
    pub entries: HashMap<String, SourceHash>,
}

impl HashManifest {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute the canonical SHA256 over a source blob. Hex-encoded so
    /// manifests stay human-readable.
    pub fn sha256_hex(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        let digest = hasher.finalize();
        let mut out = String::with_capacity(64);
        for b in digest {
            out.push_str(&format!("{:02x}", b));
        }
        out
    }

    /// True when `id` is missing from the manifest OR its stored hash
    /// differs from `content`'s hash. Caller should then recompile and
    /// `mark_compiled()` the entry.
    pub fn is_dirty(&self, id: &str, content: &[u8]) -> bool {
        let fresh = Self::sha256_hex(content);
        match self.entries.get(id) {
            None => true,
            Some(sh) => sh.sha256_hex != fresh,
        }
    }

    /// Record that `id` was just compiled against `content`. Overwrites
    /// any previous entry. `now_unix` is accepted as a parameter so tests
    /// can inject a deterministic clock.
    pub fn mark_compiled(&mut self, id: &str, content: &[u8], now_unix: u64) {
        self.entries.insert(
            id.to_string(),
            SourceHash {
                sha256_hex: Self::sha256_hex(content),
                last_compile_unix: now_unix,
            },
        );
    }

    /// Persist to disk at the given path. Uses the shared
    /// `atomic_write` (temp+rename) so an interrupted compile never
    /// leaves a torn manifest.
    pub async fn save(&self, path: &std::path::Path) -> Result<(), MemoryError> {
        let body = serde_json::to_vec_pretty(self).map_err(|e| MemoryError::CompileFailed {
            reason: format!("manifest serialize: {e}"),
        })?;
        atomic_write(path, &body).await
    }

    /// Load a manifest from disk. Missing file → empty manifest (first
    /// compile). Malformed JSON → `CompileFailed`.
    pub async fn load(path: &std::path::Path) -> Result<Self, MemoryError> {
        match tokio::fs::read(path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| MemoryError::CompileFailed {
                reason: format!("manifest parse: {e}"),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(MemoryError::CompileFailed {
                reason: format!("manifest read: {e}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempfile(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "theo-wiki-hash-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(".hashes.json")
    }

    // ── RM5a-AC-1 ───────────────────────────────────────────────
    #[test]
    fn test_rm5a_ac_1_unchanged_source_skip_recompile() {
        let mut m = HashManifest::new();
        m.mark_compiled("lesson:l-1", b"hello", 1000);
        // Same body → not dirty.
        assert!(!m.is_dirty("lesson:l-1", b"hello"));
    }

    // ── RM5a-AC-2 ───────────────────────────────────────────────
    #[test]
    fn test_rm5a_ac_2_dirty_source_marks_for_recompile() {
        let mut m = HashManifest::new();
        m.mark_compiled("lesson:l-1", b"hello", 1000);
        // Different body → dirty.
        assert!(m.is_dirty("lesson:l-1", b"hello world"));
    }

    #[test]
    fn unknown_id_is_dirty() {
        let m = HashManifest::new();
        assert!(m.is_dirty("new-thing", b"x"));
    }

    #[test]
    fn sha256_hex_is_deterministic_and_64_chars() {
        let a = HashManifest::sha256_hex(b"rust");
        let b = HashManifest::sha256_hex(b"rust");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(a, HashManifest::sha256_hex(b"rust!"));
    }

    #[tokio::test]
    async fn save_load_roundtrip_preserves_entries() {
        let path = tempfile("roundtrip");
        let mut m = HashManifest::new();
        m.mark_compiled("a", b"body-a", 10);
        m.mark_compiled("b", b"body-b", 20);
        m.save(&path).await.unwrap();

        let loaded = HashManifest::load(&path).await.unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries["a"].last_compile_unix, 10);
        assert_eq!(loaded.entries["b"].last_compile_unix, 20);
    }

    #[tokio::test]
    async fn load_missing_file_returns_empty_manifest() {
        let path = std::env::temp_dir().join(format!(
            "theo-wiki-hash-no-file-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let m = HashManifest::load(&path).await.unwrap();
        assert!(m.entries.is_empty());
    }
}
