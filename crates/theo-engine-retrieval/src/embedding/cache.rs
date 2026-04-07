//! Embedding cache: pre-compute and persist file embeddings to disk.
//!
//! Avoids re-embedding on every startup (~28s for 265 files).
//! Cache is keyed by (file_path, last_modified) and versioned by model hash.
//!
//! Format: bincode-serialized `CacheData` with version header.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::embedding::neural::NeuralEmbedder;
use theo_engine_graph::model::{CodeGraph, NodeType};

// ---------------------------------------------------------------------------
// Cache data structures
// ---------------------------------------------------------------------------

/// Version header: model name hash + embedding dimension.
/// If the model changes, the cache auto-invalidates.
const CACHE_VERSION: u32 = 1;
const MODEL_NAME: &str = "AllMiniLM-L6-v2";
const EMBEDDING_DIM: usize = 384;

#[derive(Serialize, Deserialize)]
struct CacheHeader {
    version: u32,
    model_name: String,
    embedding_dim: usize,
    /// Hash of (sorted file_paths + last_modified) for staleness detection.
    graph_hash: u64,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    last_modified: f64,
    embedding: Vec<f64>,
}

#[derive(Serialize, Deserialize)]
struct CacheData {
    header: CacheHeader,
    entries: HashMap<String, CacheEntry>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Pre-computed embedding cache for file-level dense search.
pub struct EmbeddingCache {
    /// file_path -> embedding vector (384-dim f64).
    embeddings: HashMap<String, Vec<f64>>,
    graph_hash: u64,
}

impl EmbeddingCache {
    /// Build cache from CodeGraph by embedding each file's document.
    ///
    /// For each File node, constructs a document from:
    /// - File name and path
    /// - Child symbol names and signatures
    /// - First-line docstrings
    ///
    /// Then batch-embeds all documents via NeuralEmbedder.
    pub fn build(graph: &CodeGraph, embedder: &NeuralEmbedder) -> Self {
        let graph_hash = compute_graph_hash(graph);

        // Collect file documents
        let mut file_paths: Vec<String> = Vec::new();
        let mut documents: Vec<String> = Vec::new();

        for node_id in graph.node_ids() {
            let Some(node) = graph.get_node(node_id) else { continue };
            if node.node_type != NodeType::File {
                continue;
            }

            let file_path = node.file_path.as_deref().unwrap_or(&node.name);
            let doc = build_file_document(graph, node_id, node);

            file_paths.push(file_path.to_string());
            documents.push(doc);
        }

        // Batch embed (chunked for memory efficiency)
        let mut embeddings = HashMap::with_capacity(file_paths.len());
        let chunk_size = 64;

        for (chunk_idx, chunk) in documents.chunks(chunk_size).enumerate() {
            let refs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
            let batch = embedder.embed_batch(&refs);

            for (i, emb) in batch.into_iter().enumerate() {
                let idx = chunk_idx * chunk_size + i;
                if idx < file_paths.len() {
                    embeddings.insert(file_paths[idx].clone(), emb);
                }
            }
        }

        EmbeddingCache {
            embeddings,
            graph_hash,
        }
    }

    /// Load cache from disk. Returns None if file doesn't exist, is corrupted,
    /// or has a different version/graph hash.
    pub fn load(path: &Path, expected_graph_hash: u64) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let data: CacheData = bincode::deserialize(&bytes).ok()?;

        // Validate header
        if data.header.version != CACHE_VERSION
            || data.header.model_name != MODEL_NAME
            || data.header.embedding_dim != EMBEDDING_DIM
            || data.header.graph_hash != expected_graph_hash
        {
            return None; // Stale or incompatible cache
        }

        let embeddings = data
            .entries
            .into_iter()
            .map(|(path, entry)| (path, entry.embedding))
            .collect();

        Some(EmbeddingCache {
            embeddings,
            graph_hash: expected_graph_hash,
        })
    }

    /// Save cache to disk.
    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let entries: HashMap<String, CacheEntry> = self
            .embeddings
            .iter()
            .map(|(path, emb)| {
                (
                    path.clone(),
                    CacheEntry {
                        last_modified: 0.0, // Simplified: hash-based invalidation
                        embedding: emb.clone(),
                    },
                )
            })
            .collect();

        let data = CacheData {
            header: CacheHeader {
                version: CACHE_VERSION,
                model_name: MODEL_NAME.to_string(),
                embedding_dim: EMBEDDING_DIM,
                graph_hash: self.graph_hash,
            },
            entries,
        };

        let bytes = bincode::serialize(&data)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Get embedding for a file path. Returns None if not cached.
    pub fn get(&self, file_path: &str) -> Option<&Vec<f64>> {
        self.embeddings.get(file_path)
    }

    /// Number of cached embeddings.
    pub fn len(&self) -> usize {
        self.embeddings.len()
    }

    /// Whether cache is empty.
    pub fn is_empty(&self) -> bool {
        self.embeddings.is_empty()
    }

    /// Graph hash this cache was built for.
    pub fn graph_hash(&self) -> u64 {
        self.graph_hash
    }

    /// Iterate over all (file_path, embedding) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[f64])> {
        self.embeddings
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a text document for embedding from a file's CodeGraph data.
fn build_file_document(
    graph: &CodeGraph,
    file_id: &str,
    file_node: &theo_engine_graph::model::Node,
) -> String {
    let mut parts = Vec::new();

    // File name and path
    parts.push(file_node.name.clone());
    if let Some(fp) = &file_node.file_path {
        // Path segments as context
        for seg in fp.split('/') {
            if seg.len() >= 3 {
                parts.push(seg.to_string());
            }
        }
    }

    // Child symbols: names, signatures, docstrings
    for child_id in graph.contains_children(file_id) {
        if let Some(child) = graph.get_node(child_id) {
            parts.push(child.name.clone());
            if let Some(sig) = &child.signature {
                parts.push(sig.clone());
            }
            if let Some(doc) = &child.doc {
                if let Some(first_line) = doc.lines().next() {
                    parts.push(first_line.to_string());
                }
            }
        }
    }

    parts.join(" ")
}

/// Compute a hash of the graph's file structure for cache invalidation.
///
/// Uses file paths + last_modified timestamps. If any file changes,
/// the hash changes and the cache is invalidated.
fn compute_graph_hash(graph: &CodeGraph) -> u64 {
    use std::collections::BTreeMap;
    use std::hash::{Hash, Hasher};

    // BTreeMap for deterministic ordering
    let mut file_info: BTreeMap<String, u64> = BTreeMap::new();

    for node_id in graph.node_ids() {
        if let Some(node) = graph.get_node(node_id) {
            if node.node_type == NodeType::File {
                let path = node.file_path.as_deref().unwrap_or(&node.name);
                file_info.insert(path.to_string(), node.last_modified.to_bits());
            }
        }
    }

    let mut hasher = std::hash::DefaultHasher::new();
    for (path, mtime) in &file_info {
        path.hash(&mut hasher);
        mtime.hash(&mut hasher);
    }
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_hash_deterministic() {
        let graph = CodeGraph::new();
        let h1 = compute_graph_hash(&graph);
        let h2 = compute_graph_hash(&graph);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_cache_empty_graph() {
        let graph = CodeGraph::new();
        let embedder = match NeuralEmbedder::new() {
            Ok(e) => e,
            Err(_) => return, // Skip if model unavailable
        };
        let cache = EmbeddingCache::build(&graph, &embedder);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_save_load_roundtrip() {
        let graph = CodeGraph::new();
        let embedder = match NeuralEmbedder::new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let cache = EmbeddingCache::build(&graph, &embedder);
        let hash = cache.graph_hash();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("embeddings.bin");

        cache.save(&path).unwrap();
        let loaded = EmbeddingCache::load(&path, hash);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().len(), cache.len());
    }

    #[test]
    fn test_cache_invalidation_on_hash_mismatch() {
        let graph = CodeGraph::new();
        let embedder = match NeuralEmbedder::new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let cache = EmbeddingCache::build(&graph, &embedder);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("embeddings.bin");
        cache.save(&path).unwrap();

        // Load with different hash → should fail
        let loaded = EmbeddingCache::load(&path, 999999);
        assert!(loaded.is_none());
    }

    #[test]
    fn test_build_file_document_empty() {
        let graph = CodeGraph::new();
        let node = theo_engine_graph::model::Node {
            id: "file:test.rs".into(),
            name: "test.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/test.rs".into()),
            line_start: None,
            line_end: None,
            signature: None,
            doc: None,
            kind: None,
            last_modified: 0.0,
        };
        let doc = build_file_document(&graph, "file:test.rs", &node);
        assert!(doc.contains("test.rs"));
        assert!(doc.contains("src"));
    }
}
