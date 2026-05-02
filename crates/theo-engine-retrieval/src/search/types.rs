//! BM25 search index and multi-signal scorer over community summaries.
//!
//! # BM25 formula
//! score(q, D) = Σ_t IDF(t) * (f(t,D) * (k1+1)) / (f(t,D) + k1 * (1 - b + b * |D|/avgdl))
//! IDF(t) = ln((N - n(t) + 0.5) / (n(t) + 0.5) + 1)

#![allow(unused_imports, dead_code)]

use std::collections::HashMap;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::graph_attention::propagate_attention;
use crate::neural::NeuralEmbedder;
use crate::tfidf::{TfidfConfig, TfidfModel};
use crate::turboquant::{QuantizedVector, TurboQuantizer};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// BM25 tuning parameters.
pub struct Bm25Config {
    /// Term-frequency saturation (default 1.2).
    pub k1: f64,
    /// Document-length normalization (default 0.75).
    pub b: f64,
}

impl Default for Bm25Config {
    fn default() -> Self {
        Bm25Config { k1: 1.2, b: 0.75 }
    }
}

/// Configurable scoring weights for multi-signal ranking.
///
/// Allows tuning the relative importance of each retrieval signal
/// without recompilation. Weights are normalized to sum to 1.0.
///
/// Default: BM25-dominant (55%), file boost (30%), centrality (5%), recency (10%).
#[derive(Debug, Clone)]
pub struct ScoringWeights {
    /// BM25 text relevance (default 0.55).
    pub bm25: f64,
    /// File-level symbol name match (default 0.30).
    pub file_boost: f64,
    /// PageRank centrality (default 0.05).
    pub centrality: f64,
    /// Git recency (default 0.10).
    pub recency: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        ScoringWeights {
            bm25: 0.55,
            file_boost: 0.30,
            centrality: 0.05,
            recency: 0.10,
        }
    }
}

impl ScoringWeights {
    /// Create custom weights. Normalizes to sum to 1.0.
    pub fn new(bm25: f64, file_boost: f64, centrality: f64, recency: f64) -> Self {
        let sum = bm25 + file_boost + centrality + recency;
        if sum == 0.0 {
            return Self::default();
        }
        ScoringWeights {
            bm25: bm25 / sum,
            file_boost: file_boost / sum,
            centrality: centrality / sum,
            recency: recency / sum,
        }
    }
}

/// A community with its BM25 (or composite) relevance score.
pub struct ScoredCommunity {
    pub community: Community,
    pub score: f64,
}

/// Pre-built BM25 inverted index over community documents.
pub struct Bm25Index {
    /// inverted index: term -> Vec<(community_idx, term_frequency)>
    pub postings: HashMap<String, Vec<(usize, f64)>>,
    /// Number of tokens in each document (indexed by community position).
    pub doc_lengths: Vec<f64>,
    pub avg_doc_length: f64,
    pub doc_count: usize,
    pub config: Bm25Config,
}

/// Multi-signal scorer: BM25 + semantic (neural+TurboQuant or TF-IDF fallback)
/// + PageRank centrality + recency + file-level match.
///
/// When neural embeddings are available, doc vectors are compressed via TurboQuant
/// (384-dim float → 96 bytes, 32x compression) with ~5% quality loss.
/// Query vectors stay full-precision for maximum accuracy.
pub struct MultiSignalScorer {
    #[allow(dead_code)] // Superseded by FileBm25 (file-level indexing)
    pub bm25_index: Bm25Index,
    /// community_id -> normalized PageRank score
    pub centrality_scores: HashMap<String, f64>,
    /// community_id -> normalized recency score (0..=1)
    pub recency_scores: HashMap<String, f64>,
    /// community_id -> Vec of pre-tokenized symbol token sets (for file-level matching).
    /// Pre-tokenized during build to avoid re-tokenizing on every query.
    pub community_symbol_tokens: HashMap<String, Vec<std::collections::HashSet<String>>>,
    /// Neural embedder (preferred). None if model download failed.
    pub embedder: Option<NeuralEmbedder>,
    /// TurboQuant quantizer — used for BOTH neural embeddings (384-dim) and TF-IDF fallback (128-dim).
    /// When neural is available, quantizes 384-dim neural vectors (96 bytes each).
    /// When neural fails, quantizes 128-dim TF-IDF vectors (32 bytes each).
    pub quantizer: TurboQuantizer,
    /// community_id -> TurboQuant-compressed vector (neural or TF-IDF depending on mode).
    pub quantized_docs: HashMap<String, QuantizedVector>,
    /// TF-IDF model (fallback).
    pub tfidf_model: TfidfModel,
    /// True if using neural embeddings, false if TF-IDF fallback.
    #[allow(dead_code)]
    pub using_neural: bool,
    /// Weights: [bm25, semantic, file_boost, graph_attention, centrality, recency]
    #[allow(dead_code)]
    pub weights: [f64; 6],
    /// Configurable scoring weights for non-neural scoring path.
    pub scoring_weights: ScoringWeights,
}

// ---------------------------------------------------------------------------
// Tokenisation
