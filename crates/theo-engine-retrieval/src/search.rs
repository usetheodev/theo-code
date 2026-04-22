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
    #[allow(dead_code)] // Superseded by FileBm25 (file-level indexing)
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
    /// Configurable scoring weights for non-neural scoring path.
    pub scoring_weights: ScoringWeights,
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
        // Also index the unsplit form (lowercased) for substring-like matching.
        // "OpenAIAuth" → tokens: ["open", "ai", "auth", "openaiauth"]
        // This helps "oauth" match files containing "oauth_client" etc.
        let lower = word.to_lowercase();
        if lower.len() >= 3 {
            tokens.push(lower);
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
        } else { prev.is_uppercase()
            && curr.is_uppercase()
            && i + 1 < len && chars[i + 1].is_lowercase() };

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
/// Build a text document for BM25 indexing from a community's nodes.
///
/// ALWAYS includes signatures and symbol names — these are the terms that
/// queries match against. Without them, BM25 returns 0 and query-independent
/// signals (centrality, recency) dominate, producing a fixed ranking.
fn community_document(community: &Community, graph: &CodeGraph) -> String {
    use theo_engine_graph::model::NodeType;

    let mut parts = vec![community.name.clone()];

    for node_id in &community.node_ids {
        if let Some(node) = graph.get_node(node_id) {
            parts.push(node.name.clone());

            if let Some(sig) = &node.signature {
                parts.push(sig.clone());
            }
            if let Some(doc) = &node.doc
                && let Some(first_line) = doc.lines().next() {
                    parts.push(first_line.to_string());
                }

            // Follow CONTAINS edges to get child symbols
            if matches!(node.node_type, NodeType::File) {
                for child_id in graph.contains_children(node_id) {
                    if let Some(child) = graph.get_node(child_id) {
                        parts.push(child.name.clone());
                        if let Some(sig) = &child.signature {
                            parts.push(sig.clone());
                        }
                        if let Some(doc) = &child.doc
                            && let Some(first_line) = doc.lines().next() {
                                parts.push(first_line.to_string());
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

        result.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
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
        if let (Some(s), Some(t)) = (src_idx, tgt_idx)
            && s != t {
                *adj_raw.entry((s, t)).or_insert(0.0) += edge.weight;
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
        let dangling_mass: f64 = pr
            .iter()
            .enumerate()
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

    let max_ts = raw
        .iter()
        .map(|(_, ts)| *ts)
        .fold(f64::NEG_INFINITY, f64::max);
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
// Rust stop words — filter high-frequency code tokens from BM25
// ---------------------------------------------------------------------------

const RUST_STOP_WORDS: &[&str] = &[
    "fn", "pub", "let", "mut", "use", "mod", "impl", "struct", "enum", "trait", "type", "const",
    "static", "self", "super", "crate", "where", "for", "in", "if", "else", "match", "return",
    "async", "await", "move", "ref", "as", "str", "string", "bool", "i32", "i64", "u8", "u32",
    "u64", "usize", "f64", "option", "result", "ok", "err", "some", "none", "vec", "box", "arc",
    "true", "false", "new", "default",
];

fn is_stop_word(token: &str) -> bool {
    RUST_STOP_WORDS.contains(&token)
}

// ---------------------------------------------------------------------------
// File-level BM25 with BM25F boosts (CodeCompass/Zoekt pattern)
// ---------------------------------------------------------------------------

/// File-level BM25 search with BM25F field boosts.
///
/// Indexes each file as a separate document (not community).
/// Boosts: filename 5x, symbol name 3x, signature 1x, doc 1x.
pub struct FileBm25;

impl FileBm25 {
    /// Search with Pseudo-Relevance Feedback (PRF).
    ///
    /// Stage 1: Initial BM25 search.
    /// Stage 2: If top result is confident, extract its symbol names as expansion
    ///          terms and merge with original scores.
    ///
    /// PRF is a classic IR technique (Rocchio, 1971) that uses the top result's
    /// vocabulary to find related documents. Unlike graph expansion, PRF is purely
    /// lexical and doesn't suffer from dense import noise.
    pub fn search(graph: &CodeGraph, query: &str) -> HashMap<String, f64> {
        let initial = Self::search_inner(graph, query);

        // Only expand if we have a confident top result
        let mut sorted: Vec<_> = initial.iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

        if sorted.len() >= 2 && *sorted[0].1 > sorted[1].1 * 2.0 {
            let top_file = sorted[0].0.as_str();
            let file_id = format!("file:{}", top_file);

            // Extract top-5 most unique symbol names from the top file
            let mut expansion: Vec<String> = Vec::new();
            for child_id in graph.contains_children(&file_id) {
                if let Some(child) = graph.get_node(child_id) {
                    // Only expand with meaningful symbol names (not common ones)
                    if child.name.len() >= 5 && !is_stop_word(&child.name.to_lowercase()) {
                        expansion.push(child.name.clone());
                    }
                }
            }
            expansion.truncate(5);

            if !expansion.is_empty() {
                // Run second BM25 with expansion terms only (not original query)
                let expansion_query = expansion.join(" ");
                let expanded = Self::search_inner(graph, &expansion_query);

                // Merge: original scores + 0.3x expanded scores
                let mut merged = initial;
                for (path, exp_score) in expanded {
                    merged
                        .entry(path)
                        .and_modify(|s| *s += exp_score * 0.3)
                        .or_insert(exp_score * 0.3);
                }
                return merged;
            }
        }

        initial
    }

    /// Inner BM25 search (single pass).
    fn search_inner(graph: &CodeGraph, query: &str) -> HashMap<String, f64> {
        let query_tokens: Vec<String> = tokenise(query)
            .into_iter()
            .filter(|t| !is_stop_word(t))
            .collect();

        if query_tokens.is_empty() {
            return HashMap::new();
        }

        let file_nodes: Vec<(&str, &str)> = graph
            .node_ids()
            .filter_map(|id| {
                let n = graph.get_node(id)?;
                if n.node_type == NodeType::File {
                    Some((id, n.file_path.as_deref().unwrap_or(&n.name)))
                } else {
                    None
                }
            })
            .collect();

        let doc_count = file_nodes.len();
        if doc_count == 0 {
            return HashMap::new();
        }

        let mut postings: HashMap<String, Vec<(usize, f64)>> = HashMap::new();
        let mut doc_lengths: Vec<f64> = Vec::with_capacity(doc_count);

        for (idx, (file_id, _)) in file_nodes.iter().enumerate() {
            let Some(file_node) = graph.get_node(file_id) else {
                continue;
            };
            let mut weighted_tf: HashMap<String, f64> = HashMap::new();

            // Filename: 5x boost (BM25F — Zoekt pattern)
            for token in tokenise(&file_node.name) {
                if !is_stop_word(&token) {
                    *weighted_tf.entry(token).or_default() += 5.0;
                }
            }

            // Path segments: 3x boost. Directory names like "routing", "extract", "middleware"
            // disambiguate files with generic names (mod.rs, lib.rs).
            // Increased from 2x to 3x after multi-repo benchmark showed axum mod.rs confusion.
            if let Some(fp) = &file_node.file_path {
                for segment in fp.split('/') {
                    for token in tokenise(segment) {
                        if !is_stop_word(&token) {
                            *weighted_tf.entry(token).or_default() += 3.0;
                        }
                    }
                }
            }

            // Children via Contains edges
            for child_id in graph.contains_children(file_id) {
                if let Some(child) = graph.get_node(child_id) {
                    // Symbol name: 3x boost
                    for token in tokenise(&child.name) {
                        if !is_stop_word(&token) {
                            *weighted_tf.entry(token).or_default() += 3.0;
                        }
                    }
                    // Signature: 1x
                    if let Some(sig) = &child.signature {
                        for token in tokenise(sig) {
                            if !is_stop_word(&token) {
                                *weighted_tf.entry(token).or_default() += 1.0;
                            }
                        }
                    }
                    // Doc first line: 1x
                    if let Some(doc) = &child.doc
                        && let Some(fl) = doc.lines().next() {
                            for token in tokenise(fl) {
                                if !is_stop_word(&token) {
                                    *weighted_tf.entry(token).or_default() += 1.0;
                                }
                            }
                        }
                    // 2-hop import enrichment: symbols this child CALLS/IMPORTS.
                    // Low boost (0.15x) to minimize IDF dilution in BM25.
                    // Higher values tested (0.3x, 0.5x) hurt BM25 baseline.
                    for target_id in graph.neighbors(child_id) {
                        if let Some(target) = graph.get_node(target_id)
                            && target.node_type == NodeType::Symbol {
                                for token in tokenise(&target.name) {
                                    if !is_stop_word(&token) {
                                        *weighted_tf.entry(token).or_default() += 0.15;
                                    }
                                }
                            }
                    }
                }
            }

            let len: f64 = weighted_tf.values().sum();
            doc_lengths.push(len);
            for (term, freq) in weighted_tf {
                postings.entry(term).or_default().push((idx, freq));
            }
        }

        let avg_dl = if doc_count > 0 {
            doc_lengths.iter().sum::<f64>() / doc_count as f64
        } else {
            1.0
        };
        let (k1, b) = (1.2f64, 0.75f64);
        let n = doc_count as f64;

        let mut scores = vec![0.0f64; doc_count];
        for term in &query_tokens {
            let Some(posts) = postings.get(term.as_str()) else {
                continue;
            };
            let n_t = posts.len() as f64;
            let idf = ((n - n_t + 0.5) / (n_t + 0.5) + 1.0).ln();
            for &(doc_idx, tf) in posts {
                let dl = doc_lengths[doc_idx];
                let norm = tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * dl / avg_dl));
                scores[doc_idx] += idf * norm;
            }
        }

        let mut result = HashMap::new();
        for (idx, (_, file_path)) in file_nodes.iter().enumerate() {
            if scores[idx] > 0.0 {
                result.insert(file_path.to_string(), scores[idx]);
            }
        }
        result
    }

    /// Aggregate file scores to community level via max.
    pub fn community_scores(
        file_scores: &HashMap<String, f64>,
        communities: &[Community],
        graph: &CodeGraph,
    ) -> HashMap<String, f64> {
        communities
            .iter()
            .map(|comm| {
                let max_score = comm
                    .node_ids
                    .iter()
                    .filter_map(|nid| {
                        graph
                            .get_node(nid)
                            .and_then(|n| n.file_path.as_deref())
                            .and_then(|fp| file_scores.get(fp))
                    })
                    .copied()
                    .fold(0.0f64, f64::max);
                (comm.id.clone(), max_score)
            })
            .collect()
    }
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
        let mut community_symbol_tokens: HashMap<String, Vec<std::collections::HashSet<String>>> =
            HashMap::new();
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

        // Neural embeddings are opt-in via THEO_NEURAL=1 env var.
        // Default = BM25/TF-IDF only (80% of signals). Neural adds 20% semantic quality
        // but costs ~28s for model load + embedding. Disabled by default for responsiveness.
        let neural_enabled = std::env::var("THEO_NEURAL").is_ok();
        let embedder_result: Result<
            crate::embedding::neural::NeuralEmbedder,
            Box<dyn std::error::Error>,
        > = if neural_enabled {
            NeuralEmbedder::new()
        } else {
            Err("Neural embeddings disabled (set THEO_NEURAL=1 to enable)".into())
        };

        let (embedder, quantizer, quantized_docs, using_neural) = match embedder_result {
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
            scoring_weights: ScoringWeights::default(),
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
            eprintln!(
                "[tiered] fast path: {} communities > 500, skipping neural + graph attention",
                communities.len()
            );
        }

        // 1. BM25 scores — FILE-LEVEL with max-aggregation to community.
        // This follows the CodeCompass/Zoekt pattern: every production code search
        // system indexes at file level. Community-level BM25 dilutes TF/IDF.
        let file_scores = FileBm25::search(graph, query);
        let community_file_scores = FileBm25::community_scores(&file_scores, communities, graph);
        let bm25_map: HashMap<&str, f64> = community_file_scores
            .iter()
            .map(|(id, score)| (id.as_str(), *score))
            .collect();
        let bm25_max = bm25_map.values().cloned().fold(f64::NEG_INFINITY, f64::max);
        let bm25_min = bm25_map.values().cloned().fold(f64::INFINITY, f64::min);
        let bm25_range = bm25_max - bm25_min;

        // 2. Semantic scores — skip for large repos (expensive)
        let (_semantic_raw, _sem_range, _sem_min): (HashMap<&str, f64>, f64, f64) = if large_repo {
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
        let _graph_attention_scores: HashMap<String, f64> = if large_repo {
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

        // Signal weights depend on whether neural embeddings are active.
        // With neural ON:  BM25 25%, Semantic 20%, File boost 20%, Graph 15%, Centrality 10%, Recency 10%
        // With neural OFF: BM25 30%, File boost 25%, Graph attention 25%, Centrality 10%, Recency 10%
        //
        // THEO_NO_GRAPH_ATTENTION=1 disables graph attention signal (for A/B benchmarking).
        // Signal weights. Graph attention REMOVED — benchmark proved 0% impact
        // on top-3 rankings across 20 queries (eval_graph_attention_ab test).
        // Weights sum to 1.0. BM25 is the primary query-dependent signal.
        // Weighted linear combination — calibrated via eval suite.
        // BM25 (file-level) is the dominant query-dependent signal (55%).
        // File boost provides precision on symbol name matches (30%).
        // Centrality/recency are minimal (15% total) to avoid query-independent noise.
        let (w_bm25, w_file, w_cent, w_rec) = (
            self.scoring_weights.bm25,
            self.scoring_weights.file_boost,
            self.scoring_weights.centrality,
            self.scoring_weights.recency,
        );

        let mut result: Vec<ScoredCommunity> = communities
            .iter()
            .map(|comm| {
                // Normalize BM25 to [0,1]
                let raw_bm25 = *bm25_map.get(comm.id.as_str()).unwrap_or(&0.0);
                let norm_bm25 = if bm25_range > 0.0 {
                    (raw_bm25 - bm25_min) / bm25_range
                } else {
                    0.0
                };

                // Centrality and recency (already [0,1])
                let centrality = *self.centrality_scores.get(&comm.id).unwrap_or(&0.0);
                let recency = *self.recency_scores.get(&comm.id).unwrap_or(&0.0);

                // Per-file symbol match boost
                let file_boost =
                    if let Some(token_sets) = self.community_symbol_tokens.get(&comm.id) {
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

                let score = w_bm25 * norm_bm25
                    + w_file * file_boost
                    + w_cent * centrality
                    + w_rec * recency;

                ScoredCommunity {
                    community: comm.clone(),
                    score,
                }
            })
            .collect();

        result.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
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
        let tokens = tokenise("parse_auth_header");
        assert!(tokens.contains(&"parse".to_string()));
        assert!(tokens.contains(&"auth".to_string()));
        assert!(tokens.contains(&"header".to_string()));
    }

    #[test]
    fn test_split_camel_case() {
        let tokens = tokenise("verifyJwtToken");
        assert!(tokens.contains(&"verify".to_string()));
        assert!(tokens.contains(&"jwt".to_string()));
        assert!(tokens.contains(&"token".to_string()));
        // Unsplit form also present
        assert!(tokens.contains(&"verifyjwttoken".to_string()));
    }

    #[test]
    fn test_split_pascal_case() {
        let tokens = tokenise("AuthService");
        assert!(tokens.contains(&"auth".to_string()));
        assert!(tokens.contains(&"service".to_string()));
    }

    #[test]
    fn test_split_screaming_snake() {
        let tokens = tokenise("MAX_RETRY_COUNT");
        assert!(tokens.contains(&"max".to_string()));
        assert!(tokens.contains(&"retry".to_string()));
        assert!(tokens.contains(&"count".to_string()));
    }

    #[test]
    fn test_split_acronym_prefix() {
        let tokens = tokenise("HTMLParser");
        assert!(tokens.contains(&"html".to_string()));
        assert!(tokens.contains(&"parser".to_string()) || tokens.contains(&"pars".to_string()));
    }

    #[test]
    fn test_split_acronym_middle() {
        let tokens = tokenise("getHTTPResponse");
        assert!(tokens.contains(&"get".to_string()));
        assert!(tokens.contains(&"http".to_string()));
        assert!(
            tokens.contains(&"response".to_string()) || tokens.contains(&"respons".to_string())
        );
    }

    #[test]
    fn test_split_mixed_separators() {
        let tokens = tokenise("fn verify_token(jwt: &str)");
        assert!(tokens.contains(&"verify".to_string()));
        assert!(tokens.contains(&"token".to_string()));
        assert!(tokens.contains(&"jwt".to_string()));
    }

    #[test]
    fn test_split_single_word() {
        let tokens = tokenise("auth");
        assert!(tokens.contains(&"auth".to_string()));
    }

    #[test]
    fn test_split_empty() {
        let result: Vec<String> = tokenise("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_all_lowercase() {
        let tokens = tokenise("already lowercase");
        assert!(tokens.contains(&"already".to_string()));
        assert!(tokens.contains(&"lowercase".to_string()));
    }

    /// Debug test: verify BM25 actually works with a simple community.
    #[test]
    fn debug_bm25_community_document() {
        use theo_engine_graph::cluster::Community;
        use theo_engine_graph::model::*;

        let mut graph = CodeGraph::new();

        // Create a File node with Symbol children
        graph.add_node(Node {
            id: "file:crates/auth/src/lib.rs".to_string(),
            node_type: NodeType::File,
            name: "crates/auth/src/lib.rs".to_string(),
            file_path: Some("crates/auth/src/lib.rs".to_string()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:verify_token".to_string(),
            node_type: NodeType::Symbol,
            name: "verify_token".to_string(),
            file_path: Some("crates/auth/src/lib.rs".to_string()),
            signature: Some("pub fn verify_token(token: &str) -> Result<Claims>".to_string()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(25),
            last_modified: 0.0,
            doc: Some("Verify a JWT token and extract claims.".to_string()),
        });
        graph.add_edge(Edge {
            source: "file:crates/auth/src/lib.rs".to_string(),
            target: "sym:verify_token".to_string(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        let community = Community {
            id: "comm-auth".to_string(),
            name: "authentication".to_string(),
            level: 0,
            node_ids: vec![
                "file:crates/auth/src/lib.rs".to_string(),
                "sym:verify_token".to_string(),
            ],
            parent_id: None,
            version: 1,
        };

        // Check community_document output
        let doc = community_document(&community, &graph);
        eprintln!("COMMUNITY DOCUMENT:\n{}\n", doc);
        assert!(
            doc.contains("verify_token"),
            "Document should contain symbol name"
        );
        assert!(
            doc.contains("verify"),
            "Document should contain 'verify' after tokenization"
        );

        // Check tokenization
        let tokens = tokenise(&doc);
        eprintln!("TOKENS: {:?}\n", tokens);
        assert!(
            tokens.contains(&"verify".to_string()),
            "Tokens should contain 'verify'"
        );
        assert!(
            tokens.contains(&"token".to_string()),
            "Tokens should contain 'token'"
        );

        // Build BM25 index and search
        let communities = vec![community];
        let bm25 = Bm25Index::build(&communities, &graph);
        let results = bm25.search("verify_token", &communities);

        eprintln!("BM25 RESULTS for 'verify_token':");
        for r in &results {
            eprintln!("  {} score={:.4}", r.community.name, r.score);
        }

        assert!(!results.is_empty(), "BM25 should return results");
        assert!(
            results[0].score > 0.0,
            "Top result should have positive score, got {}",
            results[0].score
        );
    }

    // --- S3-T3: ScoringWeights tests ---

    #[test]
    fn scoring_weights_default_sums_to_one() {
        let w = ScoringWeights::default();
        let sum = w.bm25 + w.file_boost + w.centrality + w.recency;
        assert!(
            (sum - 1.0).abs() < 0.001,
            "Default weights must sum to 1.0, got {}",
            sum
        );
    }

    #[test]
    fn scoring_weights_custom_normalizes() {
        let w = ScoringWeights::new(2.0, 1.0, 0.5, 0.5);
        let sum = w.bm25 + w.file_boost + w.centrality + w.recency;
        assert!(
            (sum - 1.0).abs() < 0.001,
            "Custom weights must be normalized to 1.0, got {}",
            sum
        );
        assert!((w.bm25 - 0.5).abs() < 0.001, "2.0/4.0 = 0.5");
        assert!((w.file_boost - 0.25).abs() < 0.001, "1.0/4.0 = 0.25");
    }

    #[test]
    fn scoring_weights_zero_input_uses_default() {
        let w = ScoringWeights::new(0.0, 0.0, 0.0, 0.0);
        assert_eq!(w.bm25, ScoringWeights::default().bm25);
    }

    #[test]
    fn scoring_weights_on_scorer() {
        // Verify ScoringWeights is accessible on the scorer struct
        let graph = CodeGraph::new();
        let communities: Vec<Community> = vec![];
        let scorer = MultiSignalScorer::build(&communities, &graph);
        assert!((scorer.scoring_weights.bm25 - 0.55).abs() < 0.001);
    }
}
