//! Wiki lookup: fast semantic cache for query_context.
//!
//! BM25 search over wiki pages — IDF-aware, code-tokenized.
//! This is the first retrieval layer — before the RRF pipeline.
//!
//! Latency target: <5ms for any query.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::code_tokenizer::tokenize_code;

/// Result of a wiki lookup.
pub struct WikiLookupResult {
    /// Matched wiki page content (markdown).
    pub content: String,
    /// Slug of the matched page.
    pub slug: String,
    /// Title of the matched page.
    pub title: String,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// Token count estimate.
    pub token_count: usize,
}

/// Minimum confidence to return a wiki hit (avoids false positives).
/// Tuned conservatively: better to miss and fall through to RRF
/// than to return wrong context.
const MIN_CONFIDENCE: f64 = 0.5;

/// Search wiki pages using BM25 scoring. Returns top matches above threshold.
///
/// BM25 with code-aware tokenization — IDF-weighted, title boost 3x.
/// Scans both modules/ (bootstrap) and cache/ (write-back).
///
/// Returns empty vec if no match above threshold (triggers RRF fallback).
pub fn lookup(wiki_dir: &Path, query: &str, max_results: usize) -> Vec<WikiLookupResult> {
    if query.is_empty() || !wiki_dir.exists() {
        return Vec::new();
    }

    let query_tokens = tokenize_code(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    // Collect all wiki pages
    let modules_dir = wiki_dir.join("modules");
    let cache_dir = wiki_dir.join("cache");

    let mut pages: Vec<(String, String, String)> = Vec::new(); // (slug, title, content)

    for dir in [&modules_dir, &cache_dir] {
        if !dir.exists() { continue; }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }
            if path.to_string_lossy().contains(".enriched.") { continue; }
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let slug = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let title = content.lines()
                .find(|l| l.starts_with("# "))
                .map(|l| l.trim_start_matches("# ").trim().to_string())
                .unwrap_or_else(|| slug.clone());
            pages.push((slug, title, content));
        }
    }

    if pages.is_empty() {
        return Vec::new();
    }

    // Build BM25 index
    let doc_count = pages.len();
    let mut postings: HashMap<String, Vec<(usize, f64)>> = HashMap::new();
    let mut doc_lengths: Vec<f64> = Vec::with_capacity(doc_count);

    for (idx, (_, title, content)) in pages.iter().enumerate() {
        let mut tf: HashMap<String, f64> = HashMap::new();

        // Title tokens: 3x boost
        for token in tokenize_code(title) {
            *tf.entry(token).or_default() += 3.0;
        }

        // Content tokens: 1x (first 3000 chars for speed)
        let preview = &content[..content.len().min(3000)];
        for token in tokenize_code(preview) {
            *tf.entry(token).or_default() += 1.0;
        }

        let len: f64 = tf.values().sum();
        doc_lengths.push(len);

        for (term, freq) in tf {
            postings.entry(term).or_default().push((idx, freq));
        }
    }

    let avg_dl = doc_lengths.iter().sum::<f64>() / doc_count as f64;
    let (k1, b) = (1.2f64, 0.75f64);
    let n = doc_count as f64;

    // BM25 scoring
    let mut scores = vec![0.0f64; doc_count];
    for token in &query_tokens {
        let Some(posts) = postings.get(token.as_str()) else { continue };
        let n_t = posts.len() as f64;
        let idf = ((n - n_t + 0.5) / (n_t + 0.5) + 1.0).ln();
        for &(doc_idx, tf) in posts {
            let dl = doc_lengths[doc_idx];
            let norm = tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * dl / avg_dl));
            scores[doc_idx] += idf * norm;
        }
    }

    // Normalize to 0-1 confidence
    let max_score = scores.iter().cloned().fold(0.0f64, f64::max);

    let mut results: Vec<WikiLookupResult> = Vec::new();
    for (idx, &score) in scores.iter().enumerate() {
        if score <= 0.0 { continue; }
        let confidence = if max_score > 0.0 { score / max_score } else { 0.0 };
        if confidence < MIN_CONFIDENCE { continue; }

        let (slug, title, content) = &pages[idx];
        results.push(WikiLookupResult {
            content: content.clone(),
            slug: slug.clone(),
            title: title.clone(),
            confidence,
            token_count: content.len() / 4,
        });
    }

    results.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(max_results);

    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_wiki(dir: &Path) {
        let modules = dir.join("modules");
        fs::create_dir_all(&modules).unwrap();

        fs::write(dir.join("index.md"), "# Wiki\n").unwrap();

        fs::write(modules.join("auth.md"), r#"# Authentication Module

> 3 files | rs | 15 symbols

## Entry Points

```rust
pub fn verify_jwt_token(token: &str) -> Result<Claims>
```

## Public API

```rust
pub fn verify_jwt_token(token: &str) -> Result<Claims>
pub fn refresh_token(refresh: &str) -> Result<Token>
pub struct OAuthConfig
```

## Dependencies

- → [[domain]] (Imports)
"#).unwrap();

        fs::write(modules.join("search.md"), r#"# Search Engine

> 5 files | rs | 30 symbols

## Entry Points

```rust
pub fn hybrid_rrf_search(graph: &CodeGraph, ...) -> HashMap<String, f64>
```

## Public API

```rust
pub struct FileBm25
pub struct MultiSignalScorer
pub fn tokenise(text: &str) -> Vec<String>
```
"#).unwrap();
    }

    #[test]
    fn lookup_finds_auth_page() {
        let dir = tempfile::tempdir().unwrap();
        create_wiki(dir.path());
        let results = lookup(dir.path(), "JWT token authentication verify", 5);
        assert!(!results.is_empty(), "should find auth page");
        assert_eq!(results[0].slug, "auth");
        assert!(results[0].confidence >= MIN_CONFIDENCE);
    }

    #[test]
    fn lookup_finds_search_page() {
        let dir = tempfile::tempdir().unwrap();
        create_wiki(dir.path());
        let results = lookup(dir.path(), "BM25 search scoring hybrid RRF", 5);
        assert!(!results.is_empty(), "should find search page");
        assert_eq!(results[0].slug, "search");
    }

    #[test]
    fn lookup_returns_empty_for_no_match() {
        let dir = tempfile::tempdir().unwrap();
        create_wiki(dir.path());
        let results = lookup(dir.path(), "zzzzz completely unrelated query", 5);
        assert!(results.is_empty(), "should not match anything");
    }

    #[test]
    fn lookup_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let results = lookup(dir.path(), "auth", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn lookup_empty_query() {
        let dir = tempfile::tempdir().unwrap();
        create_wiki(dir.path());
        let results = lookup(dir.path(), "", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn lookup_respects_max_results() {
        let dir = tempfile::tempdir().unwrap();
        create_wiki(dir.path());
        let results = lookup(dir.path(), "pub fn", 1);
        assert!(results.len() <= 1);
    }

    #[test]
    fn lookup_confidence_ordering() {
        let dir = tempfile::tempdir().unwrap();
        create_wiki(dir.path());
        let results = lookup(dir.path(), "authentication verify token JWT", 5);
        if results.len() >= 2 {
            assert!(results[0].confidence >= results[1].confidence);
        }
    }
}
