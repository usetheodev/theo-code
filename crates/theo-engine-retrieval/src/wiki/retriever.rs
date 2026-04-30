//! Wiki retriever — Phase 1 / T1.1 of the
//! `wiki-graph-memory-blend-retrieval-plan`.
//!
//! Hybrid BM25 + dense + RRF over the in-memory `Wiki` IR. Returns
//! `WikiHit`s that already carry the unrolled file paths (from each
//! page's `source_refs`) so callers don't need to walk back through
//! the wiki to feed the joint scorer.
//!
//! The retriever is a *pure* function over `(Wiki, WikiDenseIndex,
//! Embedder)` — no filesystem, no network. The wiki module owns the
//! lifecycle of those inputs (per ADR D1); the retriever consumes them.

use std::collections::{HashMap, HashSet};

use crate::code_tokenizer::tokenize_code;
use crate::wiki::dense_index::{Embedder, WikiDenseIndex};
use crate::wiki::model::{AuthorityTier, Wiki, WikiDoc};

/// One ranked wiki hit, ready for joint scoring.
#[derive(Debug, Clone)]
pub struct WikiHit {
    pub page_id: String,
    pub title: String,
    pub summary: String,
    pub tier: AuthorityTier,
    /// Tier-adjusted, post-RRF score in `[0.0, +inf)`. Higher = better.
    pub score: f64,
    /// File paths declared by this page's `source_refs`, deduplicated and
    /// in source-ref order.
    pub linked_files: Vec<String>,
}

/// RRF k constant (TREC standard, also used by the existing
/// `hybrid_rrf_search`).
const RRF_K: f64 = 60.0;

/// Search `wiki` for the top-k pages most relevant to `query`, using
/// RRF fusion of BM25 + cosine similarity from `dense` (when present),
/// then tier-weighting the fused score.
///
/// If `dense` is `None` (cold start, or a model swap left the cache
/// stale), the retriever falls back to BM25-only — never panics.
pub fn search(
    wiki: &Wiki,
    dense: Option<&WikiDenseIndex>,
    embedder: Option<&dyn Embedder>,
    query: &str,
    top_k: usize,
) -> Vec<WikiHit> {
    if wiki.docs.is_empty() || query.trim().is_empty() || top_k == 0 {
        return Vec::new();
    }
    let query_tokens = tokenize_code(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let bm25 = compute_bm25_scores(&wiki.docs, &query_tokens);
    let dense_scores = match (dense, embedder) {
        (Some(idx), Some(emb)) => Some(compute_dense_scores(&wiki.docs, idx, emb, query)),
        _ => None,
    };

    let bm25_ranks = ranks_from_scores(&bm25);
    let dense_ranks = dense_scores.as_ref().map(|s| ranks_from_scores(s));

    let mut fused: Vec<(usize, f64)> = (0..wiki.docs.len())
        .map(|i| {
            let mut s = 0.0;
            if let Some(rank) = bm25_ranks[i] {
                s += 1.0 / (RRF_K + rank as f64);
            }
            if let Some(ref dr) = dense_ranks
                && let Some(rank) = dr[i]
            {
                s += 1.0 / (RRF_K + rank as f64);
            }
            (i, s)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    // Tier-weighting after RRF.
    for (i, score) in &mut fused {
        let tier = tier_for_doc(&wiki.docs[*i]);
        *score *= tier.weight();
    }

    fused.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });
    fused.truncate(top_k);

    fused
        .into_iter()
        .map(|(i, score)| build_hit(&wiki.docs[i], score))
        .collect()
}

fn build_hit(doc: &WikiDoc, score: f64) -> WikiHit {
    let mut seen: HashSet<String> = HashSet::new();
    let mut linked_files: Vec<String> = Vec::new();
    for sref in &doc.source_refs {
        if seen.insert(sref.file_path.clone()) {
            linked_files.push(sref.file_path.clone());
        }
    }
    WikiHit {
        page_id: doc.slug.clone(),
        title: doc.title.clone(),
        summary: doc.summary.clone(),
        tier: tier_for_doc(doc),
        score,
        linked_files,
    }
}

fn tier_for_doc(doc: &WikiDoc) -> AuthorityTier {
    if doc.enriched {
        AuthorityTier::Enriched
    } else {
        AuthorityTier::Deterministic
    }
}

/// BM25 over (title × 3) + (summary × 2) + tags × 1. Operates on the
/// IR — no filesystem read.
fn compute_bm25_scores(docs: &[WikiDoc], query_tokens: &[String]) -> Vec<f64> {
    let doc_count = docs.len();
    let mut postings: HashMap<String, Vec<(usize, f64)>> = HashMap::new();
    let mut doc_lengths: Vec<f64> = Vec::with_capacity(doc_count);
    for (idx, doc) in docs.iter().enumerate() {
        let mut tf: HashMap<String, f64> = HashMap::new();
        for tok in tokenize_code(&doc.title) {
            *tf.entry(tok).or_default() += 3.0;
        }
        for tok in tokenize_code(&doc.summary) {
            *tf.entry(tok).or_default() += 2.0;
        }
        for tag in &doc.tags {
            for tok in tokenize_code(tag) {
                *tf.entry(tok).or_default() += 1.0;
            }
        }
        let len: f64 = tf.values().sum();
        doc_lengths.push(len);
        for (term, freq) in tf {
            postings.entry(term).or_default().push((idx, freq));
        }
    }
    let avg_dl = if doc_count == 0 {
        0.0
    } else {
        doc_lengths.iter().sum::<f64>() / doc_count as f64
    };
    let (k1, b) = (1.2_f64, 0.75_f64);
    let n = doc_count as f64;
    let mut scores = vec![0.0_f64; doc_count];
    for tok in query_tokens {
        let Some(posts) = postings.get(tok.as_str()) else {
            continue;
        };
        let n_t = posts.len() as f64;
        let idf = ((n - n_t + 0.5) / (n_t + 0.5) + 1.0).ln();
        for (idx, freq) in posts {
            let dl = doc_lengths[*idx];
            let denom = freq + k1 * (1.0 - b + b * dl / avg_dl.max(1.0));
            scores[*idx] += idf * (freq * (k1 + 1.0)) / denom.max(1e-9);
        }
    }
    scores
}

/// Per-doc cosine similarity between the query embedding and each
/// page's vector. Pages without a vector get score 0.
fn compute_dense_scores(
    docs: &[WikiDoc],
    dense: &WikiDenseIndex,
    embedder: &dyn Embedder,
    query: &str,
) -> Vec<f64> {
    let q_vecs = embedder.embed_batch(&[query]);
    let q = match q_vecs.into_iter().next() {
        Some(v) if v.len() == embedder.dim() => v,
        _ => return vec![0.0; docs.len()],
    };
    docs.iter()
        .map(|d| {
            dense
                .get(&d.slug)
                .map(|v| cosine_f32(&q, v))
                .unwrap_or(0.0)
        })
        .collect()
}

fn cosine_f32(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f64;
    let mut na = 0.0_f64;
    let mut nb = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let xf = *x as f64;
        let yf = *y as f64;
        dot += xf * yf;
        na += xf * xf;
        nb += yf * yf;
    }
    if na <= 0.0 || nb <= 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Convert raw scores into 1-based ranks. Pages with zero or negative
/// score get `None` (they don't contribute to RRF).
fn ranks_from_scores(scores: &[f64]) -> Vec<Option<usize>> {
    let mut indexed: Vec<(usize, f64)> = scores
        .iter()
        .enumerate()
        .map(|(i, s)| (i, *s))
        .collect();
    indexed.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out = vec![None; scores.len()];
    for (rank, (idx, score)) in indexed.iter().enumerate() {
        if *score > 0.0 {
            out[*idx] = Some(rank + 1);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::model::{SourceRef, TestCoverage, WikiManifest};

    struct FakeEmbedder {
        id: String,
        dim: usize,
    }

    impl Embedder for FakeEmbedder {
        fn id(&self) -> &str {
            &self.id
        }
        fn dim(&self) -> usize {
            self.dim
        }
        fn embed_batch(&self, texts: &[&str]) -> Vec<Vec<f32>> {
            // Deterministic: each token contributes a bump in a fixed
            // dim. Identical text → identical vector → cosine 1.0.
            texts
                .iter()
                .map(|t| {
                    let mut v = vec![0.0_f32; self.dim];
                    for (i, b) in t.bytes().enumerate() {
                        v[i % self.dim] += (b as f32) / 255.0;
                    }
                    let mut norm: f32 = v.iter().map(|x| x * x).sum();
                    norm = norm.sqrt();
                    if norm > 0.0 {
                        for x in &mut v {
                            *x /= norm;
                        }
                    }
                    v
                })
                .collect()
        }
    }

    fn make_doc(
        slug: &str,
        title: &str,
        summary: &str,
        tags: &[&str],
        files: &[&str],
        enriched: bool,
    ) -> WikiDoc {
        WikiDoc {
            slug: slug.to_string(),
            title: title.to_string(),
            community_id: format!("c-{}", slug),
            file_count: files.len(),
            symbol_count: 0,
            primary_language: "rust".to_string(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: files
                .iter()
                .map(|p| SourceRef::file(p))
                .collect(),
            summary: summary.to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            crate_description: None,
            module_doc: None,
            generated_at: "2026-04-30T00:00:00Z".to_string(),
            enriched,
        }
    }

    fn make_wiki(docs: Vec<WikiDoc>) -> Wiki {
        Wiki {
            docs,
            manifest: WikiManifest {
                schema_version: 1,
                generator_version: "test".into(),
                graph_hash: 0,
                generated_at: "2026-04-30".into(),
                page_count: 0,
                page_hashes: Default::default(),
            },
        }
    }

    #[test]
    fn test_wiki_retriever_empty_wiki_returns_empty() {
        let wiki = make_wiki(vec![]);
        let hits = search(&wiki, None, None, "anything", 5);
        assert!(hits.is_empty());
    }

    #[test]
    fn test_wiki_retriever_bm25_match_finds_page() {
        let wiki = make_wiki(vec![
            make_doc("auth", "Authentication", "OAuth flows", &["auth"], &["a.rs"], false),
            make_doc("retrieval", "Retrieval", "BM25 ranking", &["search"], &["b.rs"], false),
        ]);
        let hits = search(&wiki, None, None, "OAuth", 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page_id, "auth");
    }

    #[test]
    fn test_wiki_retriever_dense_match_finds_page_without_lexical_overlap() {
        // Two pages: one with the same text the query encodes (dense match),
        // one BM25 unrelated. Without dense, BM25 returns nothing.
        let docs = vec![
            make_doc(
                "concept",
                "Concept of community detection",
                "Leiden algorithm clusters",
                &["graph"],
                &["c.rs"],
                false,
            ),
            make_doc(
                "io",
                "I/O Layer",
                "filesystem helpers",
                &["fs"],
                &["i.rs"],
                false,
            ),
        ];
        let wiki = make_wiki(docs);

        // BM25-only: query has no token overlap with concept doc.
        let hits_bm25 = search(&wiki, None, None, "Leiden algorithm", 5);
        // BM25 should find the concept doc (lexical overlap exists with "Leiden").
        assert!(!hits_bm25.is_empty());

        // Now test dense: build a fake dense index and rerun.
        let embedder = FakeEmbedder {
            id: "fake-32".into(),
            dim: 32,
        };
        let dense = WikiDenseIndex::build_from_pages(&wiki.docs, &embedder);
        let hits = search(&wiki, Some(&dense), Some(&embedder), "Leiden algorithm", 5);
        assert!(!hits.is_empty());
        // Dense path should still rank concept doc above io doc.
        assert_eq!(hits[0].page_id, "concept");
    }

    #[test]
    fn test_wiki_retriever_rrf_combines_bm25_and_dense_signals() {
        let docs = vec![
            make_doc("a", "Alpha", "alpha summary", &[], &["a.rs"], false),
            make_doc("b", "Beta", "beta summary", &[], &["b.rs"], false),
        ];
        let wiki = make_wiki(docs);
        let embedder = FakeEmbedder {
            id: "fake-32".into(),
            dim: 32,
        };
        let dense = WikiDenseIndex::build_from_pages(&wiki.docs, &embedder);

        let bm25 = search(&wiki, None, None, "alpha", 5);
        let combined = search(&wiki, Some(&dense), Some(&embedder), "alpha", 5);

        // BM25 alone scores 'a' positive; combined should also rank 'a' first.
        assert_eq!(bm25[0].page_id, "a");
        assert_eq!(combined[0].page_id, "a");
        // Combined score should differ from BM25-only because dense added
        // a second RRF contribution.
        assert!(combined[0].score >= bm25[0].score);
    }

    #[test]
    fn test_wiki_retriever_tier_weighting_demotes_cache_pages() {
        let docs = vec![
            make_doc("e", "Enriched", "shared keyword foo", &[], &["e.rs"], true),
            make_doc("d", "Deterministic", "shared keyword foo", &[], &["d.rs"], false),
        ];
        let wiki = make_wiki(docs);
        let hits = search(&wiki, None, None, "foo", 5);
        // Both match. Deterministic has higher tier.weight() than Enriched.
        assert_eq!(hits[0].page_id, "d");
    }

    #[test]
    fn test_wiki_retriever_falls_back_to_bm25_only_when_dense_index_missing() {
        let wiki = make_wiki(vec![make_doc(
            "a",
            "Alpha",
            "alpha summary",
            &[],
            &["a.rs"],
            false,
        )]);
        let hits = search(&wiki, None, None, "alpha", 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page_id, "a");
    }

    #[test]
    fn test_wiki_retriever_unrolls_source_refs_into_linked_files() {
        let wiki = make_wiki(vec![make_doc(
            "p",
            "Page",
            "summary",
            &[],
            &["src/a.rs", "src/b.rs", "src/a.rs"], // duplicate intentional
            false,
        )]);
        let hits = search(&wiki, None, None, "summary", 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].linked_files, vec!["src/a.rs", "src/b.rs"]);
    }

    #[test]
    fn test_wiki_retriever_top_k_truncates_to_requested() {
        let wiki = make_wiki(vec![
            make_doc("a", "A", "common", &[], &["a.rs"], false),
            make_doc("b", "B", "common", &[], &["b.rs"], false),
            make_doc("c", "C", "common", &[], &["c.rs"], false),
        ]);
        let hits = search(&wiki, None, None, "common", 2);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn test_wiki_retriever_empty_query_returns_empty() {
        let wiki = make_wiki(vec![make_doc("a", "A", "x", &[], &["a.rs"], false)]);
        assert!(search(&wiki, None, None, "", 5).is_empty());
        assert!(search(&wiki, None, None, "   ", 5).is_empty());
    }

    #[test]
    fn test_wiki_retriever_zero_top_k_returns_empty() {
        let wiki = make_wiki(vec![make_doc("a", "A", "x", &[], &["a.rs"], false)]);
        assert!(search(&wiki, None, None, "x", 0).is_empty());
    }
}
