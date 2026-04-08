//! Wiki lookup: fast semantic cache for query_context.
//!
//! Searches wiki pages by token overlap with the query.
//! Returns matching pages as ContextBlocks if confidence exceeds threshold.
//! This is the first retrieval layer — before the RRF pipeline.
//!
//! Latency target: <5ms for any query.

use std::collections::HashSet;
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

/// Search wiki pages for a query. Returns top matches above confidence threshold.
///
/// Scoring: token overlap between query tokens and page title + symbol names.
/// Fast: reads pre-generated markdown files from disk, tokenizes and matches.
///
/// Returns empty vec if no match above threshold (triggers RRF fallback).
pub fn lookup(wiki_dir: &Path, query: &str, max_results: usize) -> Vec<WikiLookupResult> {
    if query.is_empty() || !wiki_dir.exists() {
        return Vec::new();
    }

    let query_tokens: HashSet<String> = tokenize_code(query).into_iter().collect();
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let modules_dir = wiki_dir.join("modules");
    let cache_dir = wiki_dir.join("cache");

    let mut results: Vec<WikiLookupResult> = Vec::new();

    // Scan both modules/ (bootstrap) and cache/ (write-back from previous queries)
    let dirs_to_scan: Vec<std::path::PathBuf> = [modules_dir, cache_dir]
        .into_iter()
        .filter(|d| d.exists())
        .collect();

    if dirs_to_scan.is_empty() {
        return Vec::new();
    }

    for scan_dir in &dirs_to_scan {
    let entries = match std::fs::read_dir(scan_dir) {
        Ok(e) => e,
        Err(_) => continue,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if path.to_string_lossy().contains(".enriched.") {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(&path) else { continue };

        let slug = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        // Extract title from first heading
        let title = content.lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l.trim_start_matches("# ").trim().to_string())
            .unwrap_or_else(|| slug.clone());

        // Score: token overlap between query and page content
        // Weight: title tokens 3x, code block tokens 2x, body tokens 1x
        let mut page_tokens: HashSet<String> = HashSet::new();
        let mut title_tokens: HashSet<String> = HashSet::new();
        let mut code_tokens: HashSet<String> = HashSet::new();

        // Tokenize title
        for token in tokenize_code(&title) {
            title_tokens.insert(token.clone());
            page_tokens.insert(token);
        }

        // Tokenize content (first 2000 chars for speed)
        let content_preview = &content[..content.len().min(2000)];
        let mut in_code_block = false;
        for line in content_preview.lines() {
            if line.starts_with("```") {
                in_code_block = !in_code_block;
                continue;
            }
            for token in tokenize_code(line) {
                if in_code_block {
                    code_tokens.insert(token.clone());
                }
                page_tokens.insert(token);
            }
        }

        // Compute weighted overlap
        let title_hits = query_tokens.iter().filter(|qt| title_tokens.contains(*qt)).count();
        let code_hits = query_tokens.iter().filter(|qt| code_tokens.contains(*qt)).count();
        let body_hits = query_tokens.iter().filter(|qt| page_tokens.contains(*qt)).count();

        let weighted_score = (title_hits as f64 * 3.0)
            + (code_hits as f64 * 2.0)
            + (body_hits as f64 * 1.0);

        let max_possible = query_tokens.len() as f64 * 3.0; // All tokens match title
        let confidence = if max_possible > 0.0 {
            (weighted_score / max_possible).min(1.0)
        } else {
            0.0
        };

        if confidence >= MIN_CONFIDENCE {
            let token_count = content.len() / 4; // Rough estimate: 4 chars per token
            results.push(WikiLookupResult {
                content,
                slug,
                title,
                confidence,
                token_count,
            });
        }
    }
    } // end for scan_dir

    // Sort by confidence descending
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
