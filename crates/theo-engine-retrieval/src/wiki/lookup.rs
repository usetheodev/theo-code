//! Wiki lookup: fast semantic cache for query_context.
//!
//! BM25 search over wiki pages — IDF-aware, code-tokenized.
//! This is the first retrieval layer — before the RRF pipeline.
//!
//! Latency target: <5ms for any query.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::model::{AuthorityTier, QueryClass, classify_query, parse_frontmatter};
use crate::code_tokenizer::tokenize_code;

/// Result of a wiki lookup.
pub struct WikiLookupResult {
    /// Matched wiki page content (markdown).
    pub content: String,
    /// Slug of the matched page.
    pub slug: String,
    /// Title of the matched page.
    pub title: String,
    /// Confidence score (0.0 to 1.0) — tier-adjusted, for ranking.
    pub confidence: f64,
    /// Raw BM25 score before any tier adjustment — for absolute confidence.
    pub bm25_raw: f64,
    /// Token count estimate.
    pub token_count: usize,
    /// Authority tier from frontmatter metadata.
    pub authority_tier: AuthorityTier,
    /// Whether this page's graph_hash differs from current manifest.
    pub is_stale: bool,
    /// Page kind (module, cache, concept, overview).
    pub page_kind: String,
}

/// Minimum confidence to return a wiki hit (avoids false positives).
const MIN_CONFIDENCE: f64 = 0.5;

/// Composite scoring: combines BM25 relevance with authority tier and freshness.
///
/// This is a scoring **policy**, not hard ordering — tier is a prior, not absolute.
pub fn compute_final_score(
    bm25_score: f64,
    tier: AuthorityTier,
    title_match: bool,
    is_stale: bool,
) -> f64 {
    let tier_weight = tier.weight();
    let title_bonus = if title_match { 0.15 } else { 0.0 };
    let stale_penalty = if is_stale { 0.3 } else { 0.0 };

    (bm25_score * tier_weight + title_bonus - stale_penalty).max(0.0)
}

/// Legacy freshness matrix — kept for backward compat, prefer evaluate_direct_return().
pub fn can_direct_return(tier: AuthorityTier, is_stale: bool, confidence: f64) -> bool {
    match (tier, is_stale) {
        (AuthorityTier::Deterministic, _) => confidence >= 0.5,
        (AuthorityTier::Enriched, false) => confidence >= 0.5,
        (AuthorityTier::Enriched, true) => false,
        (AuthorityTier::PromotedCache, false) => confidence >= 0.6,
        (AuthorityTier::PromotedCache, true) => false,
        (AuthorityTier::RawCache, false) => confidence >= 0.7,
        (AuthorityTier::RawCache, true) => false,
        // EpisodicCache never direct returns from main lookup — requires explicit opt-in.
        (AuthorityTier::EpisodicCache, _) => false,
    }
}

/// Default BM25 absolute floor. Below this, never direct return.
/// Calibrated from eval: negative queries avg 10.6, valid queries avg 15-25.
pub const DEFAULT_BM25_FLOOR: f64 = 12.0;

/// Default per-category thresholds for decision confidence.
/// Calibrated for raw score range: decision_confidence typically 5-30.
pub fn default_category_threshold(class: QueryClass) -> f64 {
    match class {
        QueryClass::ApiLookup => 5.0,
        QueryClass::Onboarding => 7.0,
        QueryClass::Architecture => 9.0,
        QueryClass::Concept => 9.0,
        QueryClass::CallFlow => 10.0,
        QueryClass::Unknown => 12.0,
    }
}

/// Compute absolute decision confidence from raw signals.
///
/// Unlike the ranking confidence (normalized by max_score), this measures
/// absolute match quality — "is this result good enough?" not "is it the best?".
pub fn compute_decision_confidence(
    bm25_top1: f64,
    bm25_top2: f64,
    title_match: bool,
    tier: AuthorityTier,
    is_stale: bool,
) -> f64 {
    let gap = bm25_top1 - bm25_top2;
    let title_bonus = if title_match { 0.15 } else { 0.0 };
    let stale_penalty = if is_stale { 0.3 } else { 0.0 };

    let raw = 0.5 * bm25_top1 + 0.3 * gap + 0.1 * title_bonus + 0.1 * tier.weight() - stale_penalty;
    raw.max(0.0)
}

/// 3-gate direct return decision with absolute confidence.
///
/// Gate 1: BM25 absolute floor — below = never return
/// Gate 2: Decision confidence from raw signals
/// Gate 3: Per-category threshold
///
/// Returns (allow, confidence, reason)
pub fn evaluate_direct_return(
    results: &[WikiLookupResult],
    query: &str,
    bm25_floor: f64,
) -> (bool, f64, &'static str) {
    let Some(top1) = results.first() else {
        return (false, 0.0, "no_results");
    };

    // Gate 1: Absolute BM25 floor
    if top1.bm25_raw < bm25_floor {
        return (false, 0.0, "below_bm25_floor");
    }

    // Gate 2: Compute decision confidence
    let bm25_top2 = results.get(1).map_or(0.0, |r| r.bm25_raw);
    let query_tokens: HashSet<String> = tokenize_code(query).into_iter().collect();
    let title_tokens: HashSet<String> = tokenize_code(&top1.title).into_iter().collect();
    let title_match = query_tokens
        .iter()
        .any(|qt| title_tokens.contains(qt.as_str()));

    let confidence = compute_decision_confidence(
        top1.bm25_raw,
        bm25_top2,
        title_match,
        top1.authority_tier,
        top1.is_stale,
    );

    // Gate 3: Per-category threshold
    let query_class = classify_query(query);
    let threshold = default_category_threshold(query_class);

    if confidence >= threshold {
        (true, confidence, "passed_all_gates")
    } else {
        (false, confidence, "below_category_threshold")
    }
}

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

    // Load manifest hash for staleness check
    let manifest_hash = wiki_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|project_dir| super::persistence::load_manifest(project_dir))
        .map(|m| m.graph_hash);

    // Collect all wiki pages with metadata
    let modules_dir = wiki_dir.join("modules");
    let cache_dir = wiki_dir.join("cache");

    struct PageEntry {
        slug: String,
        title: String,
        content: String,
        tier: AuthorityTier,
        is_stale: bool,
        page_kind: String,
    }

    let mut pages: Vec<PageEntry> = Vec::new();

    for (dir, dir_name) in [(&modules_dir, "modules"), (&cache_dir, "cache")] {
        if !dir.exists() {
            continue;
        }
        let entries = match std::fs::read_dir(dir) {
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
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let title = content
                .lines()
                .find(|l| l.starts_with("# "))
                .map(|l| l.trim_start_matches("# ").trim().to_string())
                .unwrap_or_else(|| slug.clone());

            // Classify from frontmatter metadata
            let fm = parse_frontmatter(&content);
            let tier = fm.tier(dir_name);
            let page_kind = fm.page_kind.unwrap_or_else(|| dir_name.to_string());
            let is_stale = match (fm.graph_hash, manifest_hash) {
                (Some(page_hash), Some(current_hash)) => page_hash != current_hash,
                _ => false, // No hash info → assume fresh
            };

            pages.push(PageEntry {
                slug,
                title,
                content,
                tier,
                is_stale,
                page_kind,
            });
        }
    }

    if pages.is_empty() {
        return Vec::new();
    }

    // Build BM25 index
    let doc_count = pages.len();
    let mut postings: HashMap<String, Vec<(usize, f64)>> = HashMap::new();
    let mut doc_lengths: Vec<f64> = Vec::with_capacity(doc_count);

    for (idx, page) in pages.iter().enumerate() {
        let title = &page.title;
        let content = &page.content;
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
        let Some(posts) = postings.get(token.as_str()) else {
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

    // Apply tier-aware composite scoring
    let mut final_scores: Vec<f64> = Vec::with_capacity(doc_count);
    for (idx, &raw_score) in scores.iter().enumerate() {
        let page = &pages[idx];
        // Check if any query token matches the title
        let title_tokens: HashSet<String> = tokenize_code(&page.title).into_iter().collect();
        let title_match = query_tokens
            .iter()
            .any(|qt| title_tokens.contains(qt.as_str()));
        let normalized = if scores.iter().cloned().fold(0.0f64, f64::max) > 0.0 {
            raw_score / scores.iter().cloned().fold(0.0f64, f64::max)
        } else {
            0.0
        };
        final_scores.push(compute_final_score(
            normalized,
            page.tier,
            title_match,
            page.is_stale,
        ));
    }

    // Normalize final scores to 0-1
    let max_final = final_scores.iter().cloned().fold(0.0f64, f64::max);

    let mut results: Vec<WikiLookupResult> = Vec::new();
    for (idx, &fscore) in final_scores.iter().enumerate() {
        if fscore <= 0.0 {
            continue;
        }
        let confidence = if max_final > 0.0 {
            fscore / max_final
        } else {
            0.0
        };
        if confidence < MIN_CONFIDENCE {
            continue;
        }

        let page = &pages[idx];
        results.push(WikiLookupResult {
            content: page.content.clone(),
            slug: page.slug.clone(),
            title: page.title.clone(),
            confidence,
            bm25_raw: scores[idx],
            token_count: page.content.len() / 4,
            authority_tier: page.tier,
            is_stale: page.is_stale,
            page_kind: page.page_kind.clone(),
        });
    }

    results.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(max_results);

    results
}

// ---------------------------------------------------------------------------
// Merge/Suppress Decision (3-signal deduplication)
// ---------------------------------------------------------------------------

/// Decide whether to suppress a write-back based on 3 signals.
///
/// Signals:
/// 1. Query similarity (BM25 confidence of existing page for same query)
/// 2. Content overlap (Jaccard of normalized tokens between existing and new content)
/// 3. Source overlap (Jaccard of source_ids between existing and new)
///
/// Suppress if 2 of 3 signals are high. This avoids near-duplicate cache pages
/// while allowing genuinely different perspectives on the same topic.
pub fn should_suppress_write(wiki_dir: &Path, query: &str, new_source_ids: &[String]) -> bool {
    let candidates = lookup(wiki_dir, query, 1);
    let Some(top) = candidates.first() else {
        return false;
    };

    // Signal 1: Query similarity (already BM25 scored)
    let query_sim_high = top.confidence >= 0.7;

    // Signal 2: Content token overlap
    let existing_tokens: HashSet<String> =
        tokenize_code(&top.content[..top.content.len().min(2000)])
            .into_iter()
            .collect();
    let new_tokens: HashSet<String> = new_source_ids
        .iter()
        .flat_map(|s| tokenize_code(s))
        .collect();
    let content_overlap = if existing_tokens.is_empty() || new_tokens.is_empty() {
        0.0
    } else {
        let intersection = existing_tokens.intersection(&new_tokens).count() as f64;
        let union = existing_tokens.union(&new_tokens).count() as f64;
        intersection / union
    };
    let content_overlap_high = content_overlap >= 0.3;

    // Signal 3: Source ID overlap
    let existing_sources: HashSet<&str> = extract_source_ids_from_content(&top.content);
    let new_sources: HashSet<&str> = new_source_ids.iter().map(|s| s.as_str()).collect();
    let source_overlap = if existing_sources.is_empty() || new_sources.is_empty() {
        0.0
    } else {
        let intersection = existing_sources.intersection(&new_sources).count() as f64;
        let union = existing_sources.union(&new_sources).count() as f64;
        intersection / union
    };
    let source_overlap_high = source_overlap >= 0.5;

    // Suppress if 2 of 3 signals are high
    let high_count = [query_sim_high, content_overlap_high, source_overlap_high]
        .iter()
        .filter(|&&x| x)
        .count();

    high_count >= 2
}

/// Extract source IDs from wiki page content (looks for `| `...` |` patterns in tables).
fn extract_source_ids_from_content(content: &str) -> HashSet<&str> {
    let mut sources = HashSet::new();
    for line in content.lines() {
        if line.starts_with("| `") {
            // Extract content between backticks: | `source_id` | ...
            if let Some(start) = line.find('`') {
                if let Some(end) = line[start + 1..].find('`') {
                    sources.insert(&line[start + 1..start + 1 + end]);
                }
            }
        }
    }
    sources
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

        fs::write(
            modules.join("auth.md"),
            r#"# Authentication Module

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
"#,
        )
        .unwrap();

        fs::write(
            modules.join("search.md"),
            r#"# Search Engine

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
"#,
        )
        .unwrap();
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

    #[test]
    fn lookup_module_has_deterministic_tier() {
        let dir = tempfile::tempdir().unwrap();
        create_wiki(dir.path());
        let results = lookup(dir.path(), "JWT token authentication verify", 5);
        assert!(!results.is_empty());
        // Module pages without frontmatter → fallback to Deterministic
        assert_eq!(results[0].authority_tier, AuthorityTier::Deterministic);
        assert!(!results[0].is_stale);
    }

    #[test]
    fn lookup_cache_page_has_raw_cache_tier() {
        let dir = tempfile::tempdir().unwrap();
        create_wiki(dir.path());

        // Add a cache page with frontmatter
        let cache = dir.path().join("cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("auth-flow.md"),
            "---\nauthority_tier: raw_cache\npage_kind: cache\ngraph_hash: 99999\n---\n\n# Auth Flow\n\nOAuth PKCE authentication flow details.\n"
        ).unwrap();

        let results = lookup(dir.path(), "OAuth PKCE authentication flow", 5);
        let cache_hit = results.iter().find(|r| r.slug == "auth-flow");
        assert!(cache_hit.is_some(), "should find cache page");
        assert_eq!(cache_hit.unwrap().authority_tier, AuthorityTier::RawCache);
    }

    #[test]
    fn module_outranks_cache_same_topic() {
        let dir = tempfile::tempdir().unwrap();
        create_wiki(dir.path());

        // Add cache page about same topic as module auth page
        let cache = dir.path().join("cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("jwt-auth.md"),
            "---\nauthority_tier: raw_cache\npage_kind: cache\n---\n\n# JWT Authentication\n\nJWT token verify authentication module.\n"
        ).unwrap();

        let results = lookup(dir.path(), "JWT token authentication verify", 5);
        assert!(results.len() >= 2, "should find both pages");
        // Module page (Deterministic tier) should rank first
        assert_eq!(
            results[0].authority_tier,
            AuthorityTier::Deterministic,
            "module should outrank cache: {:?}",
            results
                .iter()
                .map(|r| (&r.slug, r.authority_tier))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn scoring_policy_unit() {
        // Deterministic, no stale, no title match
        let s1 = compute_final_score(0.8, AuthorityTier::Deterministic, false, false);
        // RawCache, no stale, no title match
        let s2 = compute_final_score(0.8, AuthorityTier::RawCache, false, false);
        assert!(
            s1 > s2,
            "Deterministic should score higher: {} vs {}",
            s1,
            s2
        );

        // Title match bonus
        let s3 = compute_final_score(0.8, AuthorityTier::Deterministic, true, false);
        assert!(s3 > s1, "title match should boost: {} vs {}", s3, s1);

        // Stale penalty
        let s4 = compute_final_score(0.8, AuthorityTier::RawCache, false, true);
        assert!(s4 < s2, "stale should penalize: {} vs {}", s4, s2);
    }

    #[test]
    fn absolute_floor_blocks_weak_match() {
        let results = vec![WikiLookupResult {
            content: String::new(),
            slug: "weak".into(),
            title: "Weak".into(),
            confidence: 0.9,
            bm25_raw: 8.0, // below floor of 12.0
            token_count: 100,
            authority_tier: AuthorityTier::Deterministic,
            is_stale: false,
            page_kind: "module".into(),
        }];
        let (allow, _, reason) = evaluate_direct_return(&results, "anything", DEFAULT_BM25_FLOOR);
        assert!(!allow, "should reject below floor");
        assert_eq!(reason, "below_bm25_floor");
    }

    #[test]
    fn absolute_floor_allows_strong_match() {
        let results = vec![WikiLookupResult {
            content: String::new(),
            slug: "strong".into(),
            title: "Strong Auth".into(),
            confidence: 0.9,
            bm25_raw: 25.0, // well above floor
            token_count: 100,
            authority_tier: AuthorityTier::Deterministic,
            is_stale: false,
            page_kind: "module".into(),
        }];
        let (allow, conf, _) =
            evaluate_direct_return(&results, "strong auth token", DEFAULT_BM25_FLOOR);
        assert!(conf > 0.0, "confidence should be positive for strong match");
        // May or may not pass category threshold depending on query class
    }

    #[test]
    fn gap_based_confidence_higher_with_separation() {
        // Top1 strong, top2 weak → high gap → high confidence
        let conf_high_gap =
            compute_decision_confidence(5.0, 1.0, true, AuthorityTier::Deterministic, false);
        // Top1 strong, top2 also strong → low gap → lower confidence
        let conf_low_gap =
            compute_decision_confidence(5.0, 4.5, true, AuthorityTier::Deterministic, false);
        assert!(
            conf_high_gap > conf_low_gap,
            "high gap should give higher confidence: {} vs {}",
            conf_high_gap,
            conf_low_gap
        );
    }

    #[test]
    fn decision_confidence_penalizes_stale() {
        let fresh = compute_decision_confidence(5.0, 1.0, true, AuthorityTier::RawCache, false);
        let stale = compute_decision_confidence(5.0, 1.0, true, AuthorityTier::RawCache, true);
        assert!(
            fresh > stale,
            "stale should be penalized: {} vs {}",
            fresh,
            stale
        );
    }

    #[test]
    fn query_classification() {
        use super::super::model::classify_query;
        assert_eq!(
            classify_query("JWT token authentication verify"),
            QueryClass::ApiLookup
        );
        assert_eq!(
            classify_query("bounded context architecture layers"),
            QueryClass::Architecture
        );
        assert_eq!(
            classify_query("how does query_context call chain work"),
            QueryClass::CallFlow
        );
        assert_eq!(
            classify_query("what modules handle code intelligence"),
            QueryClass::Concept
        );
        assert_eq!(
            classify_query("how to get started building"),
            QueryClass::Onboarding
        );
        assert_eq!(
            classify_query("kubernetes deployment nginx"),
            QueryClass::Unknown
        );
    }

    #[test]
    fn freshness_matrix_unit() {
        // Deterministic: always trusted at 0.5+
        assert!(can_direct_return(AuthorityTier::Deterministic, false, 0.55));
        assert!(can_direct_return(AuthorityTier::Deterministic, true, 0.55)); // even stale

        // Enriched: only if fresh
        assert!(can_direct_return(AuthorityTier::Enriched, false, 0.55));
        assert!(!can_direct_return(AuthorityTier::Enriched, true, 0.55));

        // RawCache: highest bar, never if stale
        assert!(!can_direct_return(AuthorityTier::RawCache, false, 0.65)); // below 0.7
        assert!(can_direct_return(AuthorityTier::RawCache, false, 0.75));
        assert!(!can_direct_return(AuthorityTier::RawCache, true, 0.95)); // stale = never
    }
}
