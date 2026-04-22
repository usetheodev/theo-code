//! Cross-session keyword search over persisted episodes (Phase 1 T1.4).
//!
//! Plan: `docs/plans/PLAN_MEMORY_SUPERIORITY.md` §Task 1.4.
//! Meeting: `.claude/meetings/20260420-221947-memory-superiority-plan.md` #5.
//!
//! The pragmatic baseline (per conflict-resolution #5): keyword match
//! over objective + key_actions + learned_constraints, ranked by
//! `keyword_overlap * 0.6 + recency * 0.4`. No embeddings, no BM25 —
//! deferred to the `MemoryRetrieval` trait when T3.3 is unblocked by
//! the eval dataset.
//!
//! Dependency direction: this trait lives in `theo-domain` so that the
//! agent runtime can accept any implementation without depending on
//! `theo-infra-memory`.

use serde::{Deserialize, Serialize};

use crate::episode::EpisodeSummary;

/// One search hit — an episode that matched the query plus its score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSearchHit {
    pub summary_id: String,
    pub run_id: String,
    pub objective: String,
    pub rank_score: f32,
    pub matched_terms: Vec<String>,
}

/// Trait for cross-session episode search. Implementations live in
/// infrastructure crates (`theo-infra-memory`).
pub trait SessionSearch {
    /// Return up to 3 hits ranked by
    /// `keyword_overlap * 0.6 + recency * 0.4`.
    fn search(&self, query: &str, max_results: usize) -> Vec<SessionSearchHit>;
}

/// Extract a normalized keyword set from a query or episode field.
/// Lower-cased, punctuation-free, whitespace-split, minimum length 2.
pub fn extract_keywords(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                buf.push(lc);
            }
        } else if !buf.is_empty() {
            if buf.len() >= 2 {
                out.push(std::mem::take(&mut buf));
            } else {
                buf.clear();
            }
        }
    }
    if buf.len() >= 2 {
        out.push(buf);
    }
    out
}

/// Compute the keyword-overlap score in `[0, 1]`:
/// `|query ∩ doc| / |query|`. Empty query → 0.
pub fn keyword_overlap(query_terms: &[String], doc_terms: &[String]) -> f32 {
    if query_terms.is_empty() {
        return 0.0;
    }
    let doc_set: std::collections::HashSet<&String> = doc_terms.iter().collect();
    let hits = query_terms.iter().filter(|t| doc_set.contains(t)).count();
    hits as f32 / query_terms.len() as f32
}

/// Recency score in `[0, 1]`: newer episodes score higher.
/// Uses exponential decay with a half-life of 24 hours (episode TTL
/// default). `now_ms` is wall-clock ms (typically
/// `SystemTime::now().as_millis()`).
pub fn recency_score(episode_created_at_ms: u64, now_ms: u64) -> f32 {
    let age_ms = now_ms.saturating_sub(episode_created_at_ms);
    let half_life_ms: f64 = 24.0 * 3600.0 * 1000.0;
    let score = 2_f64.powf(-(age_ms as f64) / half_life_ms);
    score.clamp(0.0, 1.0) as f32
}

/// Pure ranking function — exposed so implementers can share the math
/// without duplicating it. `keyword_overlap * 0.6 + recency * 0.4`.
pub fn rank_episode(
    query_terms: &[String],
    episode: &EpisodeSummary,
    now_ms: u64,
) -> (f32, Vec<String>) {
    let mut doc_terms: Vec<String> = Vec::new();
    doc_terms.extend(extract_keywords(&episode.machine_summary.objective));
    for a in &episode.machine_summary.key_actions {
        doc_terms.extend(extract_keywords(a));
    }
    for c in &episode.machine_summary.learned_constraints {
        doc_terms.extend(extract_keywords(c));
    }

    let overlap = keyword_overlap(query_terms, &doc_terms);
    let recency = recency_score(episode.created_at, now_ms);
    let score = overlap * 0.6 + recency * 0.4;

    let doc_set: std::collections::HashSet<&String> = doc_terms.iter().collect();
    let matched: Vec<String> = query_terms
        .iter()
        .filter(|t| doc_set.contains(t))
        .cloned()
        .collect();

    (score, matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::EpisodeSummary;

    #[test]
    fn extract_keywords_strips_punctuation_and_normalizes() {
        let kws = extract_keywords("Fix the login BUG! (auth.rs)");
        assert_eq!(kws, vec!["fix", "the", "login", "bug", "auth", "rs"]);
    }

    #[test]
    fn extract_keywords_drops_singletons() {
        let kws = extract_keywords("a b cd");
        assert_eq!(kws, vec!["cd"]);
    }

    #[test]
    fn keyword_overlap_full_match_is_one() {
        let q = vec!["login".into(), "bug".into()];
        let d = vec!["login".into(), "bug".into(), "extra".into()];
        assert!((keyword_overlap(&q, &d) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn keyword_overlap_partial_match() {
        let q = vec!["login".into(), "bug".into()];
        let d = vec!["login".into()];
        assert!((keyword_overlap(&q, &d) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn keyword_overlap_empty_query_is_zero() {
        let q: Vec<String> = vec![];
        let d = vec!["x".into()];
        assert_eq!(keyword_overlap(&q, &d), 0.0);
    }

    #[test]
    fn recency_score_now_is_one() {
        let s = recency_score(1_000, 1_000);
        assert!((s - 1.0).abs() < 1e-6);
    }

    #[test]
    fn recency_score_one_day_is_half() {
        let one_day_ms = 24 * 3600 * 1000;
        let s = recency_score(0, one_day_ms);
        assert!((s - 0.5).abs() < 1e-3);
    }

    #[test]
    fn rank_episode_combines_keyword_and_recency() {
        let ep = EpisodeSummary::from_events("r", None, "fix login bug", &[]);
        let q = extract_keywords("login");
        let now = ep.created_at;
        let (score, matched) = rank_episode(&q, &ep, now);
        // overlap = 1.0 (login ∈ objective keywords), recency ≈ 1.0.
        assert!(score > 0.9);
        assert_eq!(matched, vec!["login"]);
    }
}
