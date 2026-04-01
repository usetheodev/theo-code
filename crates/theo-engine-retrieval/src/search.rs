/// BM25 search index and multi-signal scorer over community summaries.
///
/// # BM25 formula
/// score(q, D) = Σ_t IDF(t) * (f(t,D) * (k1+1)) / (f(t,D) + k1 * (1 - b + b * |D|/avgdl))
/// IDF(t) = ln((N - n(t) + 0.5) / (n(t) + 0.5) + 1)

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

/// A community with its BM25 (or composite) relevance score.
pub struct ScoredCommunity {
    pub community: Community,
    pub score: f64,
}

/// Pre-built BM25 inverted index over community documents.
pub struct Bm25Index {
    /// inverted index: term -> Vec<(community_idx, term_frequency)>
    postings: HashMap<String, Vec<(usize, f64)>>,
    /// Number of tokens in each document (indexed by community position).
    doc_lengths: Vec<f64>,
    avg_doc_length: f64,
    doc_count: usize,
    config: Bm25Config,
}

/// Multi-signal scorer: BM25 + semantic (neural+TurboQuant or TF-IDF fallback)
/// + PageRank centrality + recency + file-level match.
///
/// When neural embeddings are available, doc vectors are compressed via TurboQuant
/// (384-dim float → 96 bytes, 32x compression) with ~5% quality loss.
/// Query vectors stay full-precision for maximum accuracy.
pub struct MultiSignalScorer {
    bm25_index: Bm25Index,
    /// community_id -> normalized PageRank score
    centrality_scores: HashMap<String, f64>,
    /// community_id -> normalized recency score (0..=1)
    recency_scores: HashMap<String, f64>,
    /// community_id -> Vec of pre-tokenized symbol token sets (for file-level matching).
    /// Pre-tokenized during build to avoid re-tokenizing on every query.
    community_symbol_tokens: HashMap<String, Vec<std::collections::HashSet<String>>>,
    /// Neural embedder (preferred). None if model download failed.
    embedder: Option<NeuralEmbedder>,
    /// TurboQuant quantizer — used for BOTH neural embeddings (384-dim) and TF-IDF fallback (128-dim).
    /// When neural is available, quantizes 384-dim neural vectors (96 bytes each).
    /// When neural fails, quantizes 128-dim TF-IDF vectors (32 bytes each).
    quantizer: TurboQuantizer,
    /// community_id -> TurboQuant-compressed vector (neural or TF-IDF depending on mode).
    quantized_docs: HashMap<String, QuantizedVector>,
    /// TF-IDF model (fallback).
    tfidf_model: TfidfModel,
    /// True if using neural embeddings, false if TF-IDF fallback.
    #[allow(dead_code)]
    using_neural: bool,
    /// Weights: [bm25, semantic, file_boost, graph_attention, centrality, recency]
    #[allow(dead_code)]
    weights: [f64; 6],
}

// ---------------------------------------------------------------------------
// Tokenisation
// ---------------------------------------------------------------------------

/// Minimal English stemmer: normalize common suffixes so "communities" matches "community".
fn stem(word: &str) -> String {
    let w = word.to_lowercase();
    if w.len() < 4 {
        return w;
    }
    // ies → y (communities → community)
    if w.ends_with("ies") {
        return format!("{}y", &w[..w.len() - 3]);
    }
    // ing → (running → run, but keep "ring")
    if w.ends_with("ing") && w.len() > 5 {
        return w[..w.len() - 3].to_string();
    }
    // tion → t (detection → detect)
    if w.ends_with("tion") {
        return w[..w.len() - 3].to_string();
    }
    // ment → (refinement → refine)
    if w.ends_with("ment") && w.len() > 6 {
        return w[..w.len() - 4].to_string();
    }
    // es → (phases → phase)
    if w.ends_with("es") && w.len() > 4 {
        return w[..w.len() - 2].to_string();
    }
    // s → (clusters → cluster)
    if w.ends_with('s') && !w.ends_with("ss") {
        return w[..w.len() - 1].to_string();
    }
    w
}

/// Tokenise with identifier splitting and basic stemming.
///
/// Handles camelCase, PascalCase, snake_case, SCREAMING_CASE, and mixed:
///   "verifyJwtToken"     → ["verify", "jwt", "token"]
///   "parse_auth_header"  → ["parse", "auth", "header"]
///   "HTMLParser"         → ["html", "parser"]
///   "getHTTPResponse"    → ["get", "http", "response"]
///   "communities"        → ["community"] (stemmed)
pub(crate) fn tokenise(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    // First split on non-alphanumeric (handles snake_case, spaces, punctuation)
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        // Then split camelCase/PascalCase
        split_identifier(word, &mut tokens);
    }
    tokens
}

/// Split a single identifier on camelCase/PascalCase boundaries.
///
/// "verifyJwtToken" → ["verify", "jwt", "token"]
/// "HTMLParser"     → ["html", "parser"]
/// "getHTTPResponse" → ["get", "http", "response"]
fn split_identifier(word: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = word.chars().collect();
    if chars.is_empty() {
        return;
    }

    let mut start = 0;
    let len = chars.len();

    for i in 1..len {
        let prev = chars[i - 1];
        let curr = chars[i];
        let split = if prev.is_lowercase() && curr.is_uppercase() {
            true
        } else if prev.is_uppercase() && curr.is_uppercase() && i + 1 < len && chars[i + 1].is_lowercase() {
            true
        } else {
            false
        };

        if split {
            let part: String = chars[start..i].iter().collect();
            if !part.is_empty() {
                out.push(stem(&part));
            }
            start = i;
        }
    }

    let part: String = chars[start..].iter().collect();
    if !part.is_empty() {
        out.push(stem(&part));
    }
}

// ---------------------------------------------------------------------------
// BM25 helpers
// ---------------------------------------------------------------------------

/// Build the text document for a community.
///
/// For large communities (>5 files), generates a COMPACT document with
/// file names + top symbol names only (avoids dilution in BM25).
/// For small communities, includes full signatures and docstrings.
fn community_document(community: &Community, graph: &CodeGraph) -> String {
    use theo_engine_graph::model::NodeType;

    let is_large = community.node_ids.len() > 5;
    let mut parts = vec![community.name.clone()];

    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            // Always include the node name (file path for File nodes)
            parts.push(node.name.clone());

            if !is_large {
                if let Some(sig) = &node.signature {
                    parts.push(sig.clone());
                }
                if let Some(doc) = &node.doc {
                    if let Some(first_line) = doc.lines().next() {
                        parts.push(first_line.to_string());
                    }
                }
            }

            // Follow CONTAINS edges to get child symbols — O(degree) via index
            if matches!(node.node_type, NodeType::File) {
                for child_id in graph.contains_children(node_id) {
                    if let Some(child) = graph.get_node(child_id) {
                        // Always include symbol names (critical for BM25 matching)
                        parts.push(child.name.clone());
                        if !is_large {
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
                }
            }
        }
    }

    parts.join(" ")
}

// ---------------------------------------------------------------------------
// Bm25Index implementation
// ---------------------------------------------------------------------------

impl Bm25Index {
    /// Build an index from `communities` using node data from `graph`.
    pub fn build(communities: &[Community], graph: &CodeGraph) -> Self {
        let doc_count = communities.len();
        let mut postings: HashMap<String, Vec<(usize, f64)>> = HashMap::new();
        let mut doc_lengths: Vec<f64> = Vec::with_capacity(doc_count);

        for (idx, community) in communities.iter().enumerate() {
            let doc = community_document(community, graph);
            let tokens = tokenise(&doc);
            let len = tokens.len() as f64;
            doc_lengths.push(len);

            // Count term frequencies in this document.
            let mut tf: HashMap<String, f64> = HashMap::new();
            for token in tokens {
                *tf.entry(token).or_insert(0.0) += 1.0;
            }

            // Append to postings list.
            for (term, freq) in tf {
                postings.entry(term).or_default().push((idx, freq));
            }
        }

        let avg_doc_length = if doc_count == 0 {
            0.0
        } else {
            doc_lengths.iter().sum::<f64>() / doc_count as f64
        };

        Bm25Index {
            postings,
            doc_lengths,
            avg_doc_length,
            doc_count,
            config: Bm25Config::default(),
        }
    }

    /// Score all communities against `query`. Returns results sorted descending.
    pub fn search(&self, query: &str, communities: &[Community]) -> Vec<ScoredCommunity> {
        let query_tokens = tokenise(query);
        let mut scores = vec![0.0f64; communities.len()];

        if !query_tokens.is_empty() {
            for term in &query_tokens {
                let postings = match self.postings.get(term) {
                    Some(p) => p,
                    None => continue,
                };

                // n(t) = number of documents containing this term
                let n_t = postings.len() as f64;
                let n = self.doc_count as f64;
                let idf = ((n - n_t + 0.5) / (n_t + 0.5) + 1.0).ln();

                for &(doc_idx, tf) in postings {
                    if doc_idx >= communities.len() {
                        continue;
                    }
                    let dl = self.doc_lengths[doc_idx];
                    let avgdl = self.avg_doc_length;
                    let k1 = self.config.k1;
                    let b = self.config.b;

                    let norm = if avgdl == 0.0 {
                        tf
                    } else {
                        tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * dl / avgdl))
                    };
                    scores[doc_idx] += idf * norm;
                }
            }
        }

        let mut result: Vec<ScoredCommunity> = communities
            .iter()
            .enumerate()
            .map(|(i, c)| ScoredCommunity {
                community: c.clone(),
                score: scores[i],
            })
            .collect();

        result.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        result
    }
}

// ---------------------------------------------------------------------------
// PageRank helper (community-level)
// ---------------------------------------------------------------------------

/// Compute simple PageRank over communities using inter-community edges.
///
/// Two communities are connected if any node in one has an edge to any node in
/// the other. Returns a map community_id -> normalized PageRank score.
fn community_pagerank(communities: &[Community], graph: &CodeGraph) -> HashMap<String, f64> {
    let n = communities.len();
    if n == 0 {
        return HashMap::new();
    }

    // Map node_id -> community index.
    let mut node_to_idx: HashMap<&str, usize> = HashMap::new();
    for (i, comm) in communities.iter().enumerate() {
        for nid in &comm.node_ids {
            node_to_idx.insert(nid.as_str(), i);
        }
    }

    // Build SPARSE weighted adjacency between communities.
    // sparse_adj[i] = Vec<(j, weight)> for non-zero weights only.
    let mut adj_raw: HashMap<(usize, usize), f64> = HashMap::new();
    for edge in graph.all_edges() {
        let src_idx = node_to_idx.get(edge.source.as_str()).copied();
        let tgt_idx = node_to_idx.get(edge.target.as_str()).copied();
        if let (Some(s), Some(t)) = (src_idx, tgt_idx) {
            if s != t {
                *adj_raw.entry((s, t)).or_insert(0.0) += edge.weight;
            }
        }
    }

    // Row-normalize to get sparse transition: transition[i] = Vec<(j, prob)>.
    let mut row_sums = vec![0.0f64; n];
    for (&(s, _), &w) in &adj_raw {
        row_sums[s] += w;
    }

    // Group by source community.
    let mut sparse_transition: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for (&(s, t), &w) in &adj_raw {
        if row_sums[s] > 0.0 {
            sparse_transition[s].push((t, w / row_sums[s]));
        }
    }

    // Also build reverse index: for each target j, list of (source_i, prob).
    // This is what PageRank needs: pr[j] += damping * sum_i(transition[i][j] * pr[i]).
    let mut reverse_transition: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for (i, neighbors) in sparse_transition.iter().enumerate() {
        for &(j, prob) in neighbors {
            reverse_transition[j].push((i, prob));
        }
    }

    // Dangling node handling: if a community has no outgoing edges,
    // its PageRank mass is distributed uniformly (teleport).
    let dangling: Vec<bool> = (0..n).map(|i| row_sums[i] == 0.0).collect();

    // Iterative PageRank (20 iterations, damping = 0.85).
    let damping = 0.85f64;
    let teleport = (1.0 - damping) / n as f64;
    let uniform = 1.0 / n as f64;
    let mut pr = vec![uniform; n];

    for _ in 0..20 {
        // Compute dangling mass: sum of PR of dangling nodes.
        let dangling_mass: f64 = pr.iter().enumerate()
            .filter(|(i, _)| dangling[*i])
            .map(|(_, &p)| p)
            .sum();
        let dangling_contribution = damping * dangling_mass * uniform;

        let mut new_pr = vec![teleport + dangling_contribution; n];
        for j in 0..n {
            let mut sum = 0.0;
            for &(i, prob) in &reverse_transition[j] {
                sum += prob * pr[i];
            }
            new_pr[j] += damping * sum;
        }
        pr = new_pr;
    }

    // Normalize to [0, 1].
    let max_pr = pr.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min_pr = pr.iter().cloned().fold(f64::INFINITY, f64::min);
    let range = max_pr - min_pr;

    communities
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let normalized = if range > 0.0 {
                (pr[i] - min_pr) / range
            } else {
                0.0
            };
            (c.id.clone(), normalized)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Recency helper
// ---------------------------------------------------------------------------

/// For each community: max(node.last_modified for node in community.node_ids).
/// Normalizes to [0, 1] across all communities.
fn community_recency(communities: &[Community], graph: &CodeGraph) -> HashMap<String, f64> {
    let raw: Vec<(String, f64)> = communities
        .iter()
        .map(|comm| {
            let max_ts = comm
                .node_ids
                .iter()
                .filter_map(|nid| graph.get_node(nid))
                .map(|n| n.last_modified)
                .fold(f64::NEG_INFINITY, f64::max);
            let ts = if max_ts.is_finite() { max_ts } else { 0.0 };
            (comm.id.clone(), ts)
        })
        .collect();

    let max_ts = raw.iter().map(|(_, ts)| *ts).fold(f64::NEG_INFINITY, f64::max);
    let min_ts = raw.iter().map(|(_, ts)| *ts).fold(f64::INFINITY, f64::min);
    let range = max_ts - min_ts;

    raw.into_iter()
        .map(|(id, ts)| {
            let normalized = if range > 0.0 {
                (ts - min_ts) / range
            } else {
                0.0
            };
            (id, normalized)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// MultiSignalScorer implementation
// ---------------------------------------------------------------------------

impl MultiSignalScorer {
    /// Build the scorer from communities and graph data.
    pub fn build(communities: &[Community], graph: &CodeGraph) -> Self {
        let bm25_index = Bm25Index::build(communities, graph);
        let centrality_scores = community_pagerank(communities, graph);
        let recency_scores = community_recency(communities, graph);

        // Pre-compute and pre-tokenize symbol names per community for file-level matching.
        // Uses contains_children index for O(degree) instead of O(total_edges).
        // Pre-tokenizes to avoid re-tokenizing on every query.
        let mut community_symbol_tokens: HashMap<String, Vec<std::collections::HashSet<String>>> = HashMap::new();
        for comm in communities {
            let mut token_sets = Vec::new();
            for node_id in &comm.node_ids {
                if let Some(node) = graph.get_node(node_id) {
                    token_sets.push(tokenise(&node.name).into_iter().collect());
                    if matches!(node.node_type, NodeType::File) {
                        for child_id in graph.contains_children(node_id) {
                            if let Some(child) = graph.get_node(child_id) {
                                token_sets.push(tokenise(&child.name).into_iter().collect());
                            }
                        }
                    }
                }
            }
            community_symbol_tokens.insert(comm.id.clone(), token_sets);
        }

        // Build community documents (shared by both neural and TF-IDF paths).
        let community_docs: Vec<String> = communities
            .iter()
            .map(|c| community_document(c, graph))
            .collect();

        // Try neural embeddings first; fall back to TF-IDF if model init fails.
        // In BOTH cases, doc vectors are compressed via TurboQuant for memory efficiency.
        let tfidf_config = TfidfConfig::default();
        let tfidf_model = TfidfModel::build(&community_docs, &tfidf_config);

        let (embedder, quantizer, quantized_docs, using_neural) = match NeuralEmbedder::new() {
            Ok(emb) => {
                // Neural path: embed → TurboQuant compress (384-dim → 96 bytes)
                let doc_refs: Vec<&str> = community_docs.iter().map(|s| s.as_str()).collect();
                let embeddings = emb.embed_batch(&doc_refs);
                let quantizer = TurboQuantizer::new(emb.dim(), 42);
                let mut qmap = HashMap::with_capacity(communities.len());
                for (comm, emb_vec) in communities.iter().zip(embeddings.iter()) {
                    let qv = quantizer.quantize(emb_vec);
                    qmap.insert(comm.id.clone(), qv);
                }
                (Some(emb), quantizer, qmap, true)
            }
            Err(_) => {
                // TF-IDF fallback: TF-IDF → TurboQuant compress (128-dim → 32 bytes)
                let quantizer = TurboQuantizer::new(tfidf_config.target_dim, 42);
                let mut qmap = HashMap::with_capacity(communities.len());
                for (i, comm) in communities.iter().enumerate() {
                    let dense = tfidf_model.transform_normalized(&community_docs[i]);
                    let qv = quantizer.quantize(&dense);
                    qmap.insert(comm.id.clone(), qv);
                }
                (None, quantizer, qmap, false)
            }
        };

        MultiSignalScorer {
            bm25_index,
            centrality_scores,
            recency_scores,
            community_symbol_tokens,
            embedder,
            quantizer,
            quantized_docs,
            tfidf_model,
            using_neural,
            weights: [0.25, 0.20, 0.20, 0.15, 0.10, 0.10],
        }
    }

    /// Score communities using normalized weighted sum of 6 signals.
    ///
    /// Weights: BM25 (25%) + Semantic (20%) + File boost (20%) + Graph attention (15%)
    ///          + Centrality (10%) + Recency (10%)
    ///
    /// Graph attention captures transitive relevance through the code graph,
    /// discovering related code that BM25 and embeddings miss.
    pub fn score(
        &self,
        query: &str,
        communities: &[Community],
        graph: &CodeGraph,
    ) -> Vec<ScoredCommunity> {
        let query_tokens: std::collections::HashSet<String> = tokenise(query).into_iter().collect();

        if communities.is_empty() {
            return Vec::new();
        }

        // TIERED SEARCH: For large repos (>500 communities), use fast path
        // Fast path: BM25 + file boost only (skip neural + graph attention)
        // Full path: all 6 signals
        let large_repo = communities.len() > 500;
        if large_repo {
            eprintln!("[tiered] fast path: {} communities > 500, skipping neural + graph attention", communities.len());
        }

        // 1. BM25 scores (always — fast for any size)
        let bm25_results = self.bm25_index.search(query, communities);
        let bm25_map: HashMap<&str, f64> = bm25_results
            .iter()
            .map(|r| (r.community.id.as_str(), r.score))
            .collect();
        let bm25_max = bm25_map.values().cloned().fold(f64::NEG_INFINITY, f64::max);
        let bm25_min = bm25_map.values().cloned().fold(f64::INFINITY, f64::min);
        let bm25_range = bm25_max - bm25_min;

        // 2. Semantic scores — skip for large repos (expensive)
        let (semantic_raw, sem_range, sem_min): (HashMap<&str, f64>, f64, f64) = if large_repo {
            (HashMap::new(), 0.0, 0.0)
        } else {
            let query_vec: Vec<f64> = if let Some(ref embedder) = self.embedder {
                embedder.embed(query)
            } else {
                self.tfidf_model.transform_normalized(query)
            };
            let raw: HashMap<&str, f64> = communities
                .iter()
                .map(|comm| {
                    let sim = if let Some(qv) = self.quantized_docs.get(&comm.id) {
                        self.quantizer.cosine_similarity(&query_vec, qv)
                    } else {
                        0.0
                    };
                    (comm.id.as_str(), sim)
                })
                .collect();
            let max = raw.values().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min = raw.values().cloned().fold(f64::INFINITY, f64::min);
            (raw, max - min, min)
        };

        // 3-4. Graph attention — skip for large repos (O(N) per node is too slow for 60K+ nodes)
        let graph_attention_scores: HashMap<String, f64> = if large_repo {
            HashMap::new()
        } else {
            let initial_node_scores: HashMap<String, f64> = {
                let mut scores = HashMap::new();
                for nid in graph.node_ids() {
                    if let Some(node) = graph.get_node(nid) {
                        let text = match &node.signature {
                            Some(sig) => format!("{} {}", node.name, sig),
                            None => node.name.clone(),
                        };
                        let toks: std::collections::HashSet<String> =
                            tokenise(&text).into_iter().collect();
                        let overlap = if query_tokens.is_empty() {
                            0.0
                        } else {
                            let m = query_tokens.iter().filter(|qt| toks.contains(*qt)).count();
                            m as f64 / query_tokens.len() as f64
                        };
                        if overlap > 0.0 {
                            scores.insert(nid.to_string(), overlap);
                        }
                    }
                }
                scores
            };
            propagate_attention(&initial_node_scores, graph, communities, 2, 0.5)
        };

        // Weights: BM25 25%, Semantic 20%, File boost 20%, Graph attention 15%,
        //          Centrality 10%, Recency 10%
        let (w_bm25, w_sem, w_file, w_graph, w_cent, w_rec) =
            (0.25, 0.20, 0.20, 0.15, 0.10, 0.10);

        let mut result: Vec<ScoredCommunity> = communities
            .iter()
            .map(|comm| {
                // Normalize BM25
                let raw_bm25 = *bm25_map.get(comm.id.as_str()).unwrap_or(&0.0);
                let norm_bm25 = if bm25_range > 0.0 { (raw_bm25 - bm25_min) / bm25_range } else { 0.0 };

                // Normalize semantic
                let raw_sem = *semantic_raw.get(comm.id.as_str()).unwrap_or(&0.0);
                let norm_sem = if sem_range > 0.0 { (raw_sem - sem_min) / sem_range } else { 0.0 };

                // Centrality and recency (already normalized)
                let centrality = *self.centrality_scores.get(&comm.id).unwrap_or(&0.0);
                let recency = *self.recency_scores.get(&comm.id).unwrap_or(&0.0);

                // Per-file symbol match boost (best single symbol match ratio)
                // Uses pre-tokenized symbol sets — no tokenization at query time.
                let file_boost = if let Some(token_sets) = self.community_symbol_tokens.get(&comm.id) {
                    let mut best = 0.0f64;
                    for toks in token_sets {
                        let m = query_tokens.iter().filter(|qt| toks.contains(*qt)).count();
                        if !query_tokens.is_empty() {
                            best = best.max(m as f64 / query_tokens.len() as f64);
                        }
                    }
                    best
                } else {
                    0.0
                };

                // Graph attention (already normalized to [0, 1])
                let graph_att = graph_attention_scores
                    .get(&comm.id)
                    .copied()
                    .unwrap_or(0.0);

                let score = w_bm25 * norm_bm25
                    + w_sem * norm_sem
                    + w_file * file_boost
                    + w_graph * graph_att
                    + w_cent * centrality
                    + w_rec * recency;

                ScoredCommunity {
                    community: comm.clone(),
                    score,
                }
            })
            .collect();

        result.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_snake_case() {
        assert_eq!(tokenise("parse_auth_header"), vec!["parse", "auth", "header"]);
    }

    #[test]
    fn test_split_camel_case() {
        assert_eq!(tokenise("verifyJwtToken"), vec!["verify", "jwt", "token"]);
    }

    #[test]
    fn test_split_pascal_case() {
        assert_eq!(tokenise("AuthService"), vec!["auth", "service"]);
    }

    #[test]
    fn test_split_screaming_snake() {
        assert_eq!(tokenise("MAX_RETRY_COUNT"), vec!["max", "retry", "count"]);
    }

    #[test]
    fn test_split_acronym_prefix() {
        assert_eq!(tokenise("HTMLParser"), vec!["html", "parser"]);
    }

    #[test]
    fn test_split_acronym_middle() {
        assert_eq!(tokenise("getHTTPResponse"), vec!["get", "http", "response"]);
    }

    #[test]
    fn test_split_mixed_separators() {
        assert_eq!(
            tokenise("fn verify_token(jwt: &str)"),
            vec!["fn", "verify", "token", "jwt", "str"]
        );
    }

    #[test]
    fn test_split_single_word() {
        assert_eq!(tokenise("auth"), vec!["auth"]);
    }

    #[test]
    fn test_split_empty() {
        let result: Vec<String> = tokenise("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_all_lowercase() {
        assert_eq!(tokenise("already lowercase"), vec!["already", "lowercase"]);
    }
}
