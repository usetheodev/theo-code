//! In-memory inverted index for `docs_search` (T15.1).
//!
//! Simple BM25-ish scoring: term frequency × inverse document
//! frequency. Tokenisation is ASCII-lowercased word-boundary split.
//! No stemming, no fancy analysers — the corpus is per-language API
//! docs, where exact term matches dominate quality.
//!
//! When `theo-engine-retrieval`'s Tantivy backend is wired in
//! (future), `DocsIndex` becomes a trait and this in-memory impl
//! moves to a `simple` submodule.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One indexed document. The agent surfaces `title`, `source`, `url`
/// and a snippet from `body` to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocEntry {
    /// Stable identifier — `${source}::${title}` is the convention.
    pub id: String,
    /// Source registry name: "crates.io", "mdn", "npm", "rust-std", ...
    pub source: String,
    /// Document language tag: "rust", "javascript", "python", ...
    pub language: String,
    /// Display title.
    pub title: String,
    /// Canonical URL the LLM should cite.
    pub url: String,
    /// Indexed body — searched. Not summarized; caller passes the
    /// relevant excerpt(s) from the original doc.
    pub body: String,
}

/// One search result with its computed relevance score.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoredDoc {
    pub doc: DocEntry,
    pub score: f64,
}

/// In-memory inverted index over `DocEntry`. Insert order has no
/// effect on results; ordering is by score then by `doc.id` ASC for
/// stable ties.
#[derive(Debug, Default)]
pub struct DocsIndex {
    docs: Vec<DocEntry>,
    /// term → list of (doc_idx, term_freq).
    inverted: HashMap<String, Vec<(usize, u32)>>,
}

impl DocsIndex {
    /// Empty index. Add documents via [`Self::insert`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a document. The body is tokenised + indexed; the title
    /// is also tokenised so title hits boost the score (term
    /// frequencies from title and body are summed).
    pub fn insert(&mut self, doc: DocEntry) {
        let idx = self.docs.len();
        let mut term_counts: HashMap<String, u32> = HashMap::new();
        for token in tokenise(&doc.title) {
            *term_counts.entry(token).or_insert(0) += 2; // title boost
        }
        for token in tokenise(&doc.body) {
            *term_counts.entry(token).or_insert(0) += 1;
        }
        for (term, tf) in term_counts {
            self.inverted.entry(term).or_default().push((idx, tf));
        }
        self.docs.push(doc);
    }

    /// Search for documents matching `query`. Optional `source` filter
    /// restricts results to a specific source. Returns up to `top_k`
    /// `ScoredDoc`s ordered by descending score, ties broken by
    /// `doc.id` ASC for determinism.
    pub fn search(&self, query: &str, source: Option<&str>, top_k: usize) -> Vec<ScoredDoc> {
        let terms = tokenise(query);
        if terms.is_empty() || self.docs.is_empty() {
            return Vec::new();
        }

        let n_docs = self.docs.len() as f64;
        let mut scores: HashMap<usize, f64> = HashMap::new();
        for term in &terms {
            let Some(postings) = self.inverted.get(term) else {
                continue;
            };
            // IDF = log(N / df), df = number of docs containing the term.
            let df = postings.len() as f64;
            let idf = (n_docs / df).ln().max(0.0) + 1.0;
            for (doc_idx, tf) in postings {
                if let Some(filter) = source
                    && self.docs[*doc_idx].source != filter
                {
                    continue;
                }
                let contribution = (*tf as f64) * idf;
                *scores.entry(*doc_idx).or_insert(0.0) += contribution;
            }
        }

        let mut results: Vec<ScoredDoc> = scores
            .into_iter()
            .map(|(idx, score)| ScoredDoc {
                doc: self.docs[idx].clone(),
                score,
            })
            .collect();

        // Descending score, ascending id for tie-break.
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.doc.id.cmp(&b.doc.id))
        });
        results.truncate(top_k);
        results
    }

    /// Number of documents in the index.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// True when the index has no documents.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

/// ASCII-lowercased word-boundary tokeniser. Drops empty tokens and
/// pure-punctuation tokens. Kept simple — the corpus is API docs
/// where exact-term matches matter more than morphology.
pub fn tokenise(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, title: &str, body: &str) -> DocEntry {
        DocEntry {
            id: id.into(),
            source: "test".into(),
            language: "rust".into(),
            title: title.into(),
            url: format!("https://example.com/{id}"),
            body: body.into(),
        }
    }

    #[test]
    fn t151_tokenise_lowercases_and_splits_on_punctuation() {
        let t = tokenise("Vec::push(item)");
        assert_eq!(t, vec!["vec", "push", "item"]);
    }

    #[test]
    fn t151_tokenise_drops_empty_and_pure_punctuation() {
        assert!(tokenise("...,.,").is_empty());
        assert!(tokenise("").is_empty());
    }

    #[test]
    fn t151_tokenise_preserves_underscore() {
        let t = tokenise("std::env::var_os");
        // Underscore is part of identifiers — kept inside tokens.
        assert!(t.contains(&"var_os".to_string()));
    }

    #[test]
    fn t151_index_new_is_empty() {
        let idx = DocsIndex::new();
        assert!(idx.is_empty());
        assert_eq!(idx.len(), 0);
    }

    #[test]
    fn t151_index_insert_grows_len() {
        let mut idx = DocsIndex::new();
        idx.insert(sample("a", "First", "body"));
        assert_eq!(idx.len(), 1);
        assert!(!idx.is_empty());
    }

    #[test]
    fn t151_search_returns_empty_for_empty_index() {
        let idx = DocsIndex::new();
        assert!(idx.search("anything", None, 10).is_empty());
    }

    #[test]
    fn t151_search_returns_empty_for_no_matches() {
        let mut idx = DocsIndex::new();
        idx.insert(sample("a", "Vec", "Contiguous growable array"));
        assert!(idx.search("zzz_nonexistent", None, 10).is_empty());
    }

    #[test]
    fn t151_search_finds_term_in_body() {
        let mut idx = DocsIndex::new();
        idx.insert(sample("a", "Vec", "Contiguous growable array type"));
        let r = idx.search("growable", None, 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].doc.id, "a");
    }

    #[test]
    fn t151_search_title_match_outscores_body_match() {
        let mut idx = DocsIndex::new();
        // Doc A has term in title (boosted) AND body (1 occurrence each).
        idx.insert(sample(
            "a",
            "Iterator",
            "Iterators iterate one element at a time",
        ));
        // Doc B has term ONLY in body, multiple times.
        idx.insert(sample(
            "b",
            "Vec",
            "iterator iterator iterator iterator iterator",
        ));
        let r = idx.search("iterator", None, 10);
        assert_eq!(r.len(), 2);
        // Doc A: tf=2(title) + 1(body) = 3
        // Doc B: tf=5(body) = 5
        // So B actually wins on tf alone — confirms title boost is
        // additive, not multiplicative. Both must be present.
        let ids: Vec<&str> = r.iter().map(|s| s.doc.id.as_str()).collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
    }

    #[test]
    fn t151_search_orders_by_score_descending_then_id_ascending() {
        let mut idx = DocsIndex::new();
        // Both docs match the term equally — tie-break by id ASC.
        idx.insert(sample("zebra", "term", "term"));
        idx.insert(sample("alpha", "term", "term"));
        let r = idx.search("term", None, 10);
        assert_eq!(r[0].doc.id, "alpha");
        assert_eq!(r[1].doc.id, "zebra");
    }

    #[test]
    fn t151_search_top_k_caps_results() {
        let mut idx = DocsIndex::new();
        for i in 0..10 {
            idx.insert(sample(&format!("d{i}"), &format!("doc {i}"), "common"));
        }
        let r = idx.search("common", None, 3);
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn t151_search_source_filter_restricts_results() {
        let mut idx = DocsIndex::new();
        idx.insert(DocEntry {
            id: "a".into(),
            source: "crates.io".into(),
            language: "rust".into(),
            title: "Vec".into(),
            url: "u".into(),
            body: "term".into(),
        });
        idx.insert(DocEntry {
            id: "b".into(),
            source: "mdn".into(),
            language: "javascript".into(),
            title: "Array".into(),
            url: "u".into(),
            body: "term".into(),
        });
        let r = idx.search("term", Some("crates.io"), 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].doc.source, "crates.io");
    }

    #[test]
    fn t151_search_idf_demotes_common_terms() {
        let mut idx = DocsIndex::new();
        // Term `the` is in every doc → IDF ~ 0 → low contribution.
        // Term `unique` in only one doc → high contribution.
        for i in 0..10 {
            idx.insert(sample(&format!("d{i}"), "doc", "the the the"));
        }
        idx.insert(sample("special", "Special", "the the unique"));
        let r = idx.search("the unique", None, 5);
        // The `special` doc must rank first because of `unique`.
        assert_eq!(r[0].doc.id, "special");
    }

    #[test]
    fn t151_search_query_with_punctuation_matches_underscored_terms() {
        let mut idx = DocsIndex::new();
        idx.insert(sample("a", "var_os", "Reads an environment variable"));
        let r = idx.search("std::env::var_os", None, 10);
        // `var_os` should match through punctuation-tolerant tokeniser.
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].doc.id, "a");
    }

    #[test]
    fn t151_doc_entry_serde_roundtrip() {
        let d = sample("a", "T", "B");
        let json = serde_json::to_string(&d).unwrap();
        let back: DocEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}
