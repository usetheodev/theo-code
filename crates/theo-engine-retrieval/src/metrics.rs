//! Industry-standard Information Retrieval metrics for code search evaluation.
//!
//! Implements all metrics from RepoBench, CodeRAG-Bench, and CodeSearchNet:
//! - Recall@K: fraction of relevant docs in top-K
//! - Precision@K: fraction of top-K that are relevant
//! - Hit@K: binary — at least 1 relevant doc in top-K
//! - MRR: reciprocal rank of first relevant doc
//! - nDCG@K: normalized discounted cumulative gain (position-aware)
//! - MAP: mean average precision (precision at each relevant position)
//! - Dependency Coverage: fraction of expected deps covered by retrieved files
//! - Missing Dep Rate: 1 - dep_coverage

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Core retrieval metrics
// ---------------------------------------------------------------------------

/// Recall@K: fraction of all relevant docs found in top-K.
///
/// recall@K = |relevant ∩ top_K| / |relevant|
pub fn recall_at_k(returned: &[String], expected: &[&str], k: usize) -> f64 {
    if expected.is_empty() {
        return 0.0;
    }
    let top_k: HashSet<&str> = returned.iter().take(k).map(|s| s.as_str()).collect();
    let relevant: HashSet<&str> = expected.iter().copied().collect();
    let hits = top_k.iter().filter(|f| relevant.contains(**f)).count();
    hits as f64 / relevant.len() as f64
}

/// Precision@K: fraction of top-K that are relevant.
///
/// precision@K = |relevant ∩ top_K| / K
pub fn precision_at_k(returned: &[String], expected: &[&str], k: usize) -> f64 {
    if k == 0 {
        return 0.0;
    }
    let top_k: HashSet<&str> = returned.iter().take(k).map(|s| s.as_str()).collect();
    let relevant: HashSet<&str> = expected.iter().copied().collect();
    let hits = top_k.iter().filter(|f| relevant.contains(**f)).count();
    hits as f64 / k as f64
}

/// Hit@K: binary — 1.0 if at least 1 relevant doc in top-K, else 0.0.
///
/// Standard in CodeSearchNet and Sourcegraph benchmarks.
pub fn hit_at_k(returned: &[String], expected: &[&str], k: usize) -> f64 {
    let relevant: HashSet<&str> = expected.iter().copied().collect();
    for f in returned.iter().take(k) {
        if relevant.contains(f.as_str()) {
            return 1.0;
        }
    }
    0.0
}

/// MRR (Mean Reciprocal Rank): 1 / rank of first relevant result.
///
/// MRR = 1.0 if first result is relevant, 0.5 if second, etc.
pub fn mrr(returned: &[String], expected: &[&str]) -> f64 {
    let relevant: HashSet<&str> = expected.iter().copied().collect();
    for (i, f) in returned.iter().enumerate() {
        if relevant.contains(f.as_str()) {
            return 1.0 / (i + 1) as f64;
        }
    }
    0.0
}

/// nDCG@K: normalized Discounted Cumulative Gain.
///
/// Measures ranking quality with position-aware discounting.
/// Binary relevance: 1 if in expected set, 0 otherwise.
///
/// DCG@K = Σ_{i=1}^{K} rel_i / log2(i + 1)
/// IDCG@K = DCG of ideal ranking (all relevant first)
/// nDCG@K = DCG@K / IDCG@K
pub fn ndcg_at_k(returned: &[String], expected: &[&str], k: usize) -> f64 {
    if expected.is_empty() || k == 0 {
        return 0.0;
    }
    let relevant: HashSet<&str> = expected.iter().copied().collect();

    // DCG: actual ranking
    let mut dcg = 0.0;
    for (i, f) in returned.iter().take(k).enumerate() {
        if relevant.contains(f.as_str()) {
            dcg += 1.0 / (i as f64 + 2.0).log2(); // log2(i+2) because i is 0-indexed
        }
    }

    // IDCG: ideal ranking (all relevant docs at top)
    let ideal_count = relevant.len().min(k);
    let mut idcg = 0.0;
    for i in 0..ideal_count {
        idcg += 1.0 / (i as f64 + 2.0).log2();
    }

    if idcg == 0.0 {
        return 0.0;
    }
    dcg / idcg
}

/// MAP (Mean Average Precision): average of precision at each relevant position.
///
/// For a single query:
///   AP = (1/|relevant|) * Σ_{k: rel_k=1} precision@k
///
/// MAP = mean of AP across queries (computed externally).
pub fn average_precision(returned: &[String], expected: &[&str]) -> f64 {
    if expected.is_empty() {
        return 0.0;
    }
    let relevant: HashSet<&str> = expected.iter().copied().collect();
    let mut hits = 0;
    let mut sum_precision = 0.0;

    for (i, f) in returned.iter().enumerate() {
        if relevant.contains(f.as_str()) {
            hits += 1;
            sum_precision += hits as f64 / (i + 1) as f64;
        }
    }

    if relevant.is_empty() {
        0.0
    } else {
        sum_precision / relevant.len() as f64
    }
}

// ---------------------------------------------------------------------------
// Dependency metrics
// ---------------------------------------------------------------------------

/// A dependency edge between two files.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DepEdge {
    pub source: String,
    pub target: String,
    pub edge_type: String,
}

/// Dependency Coverage: fraction of expected deps where BOTH source and target
/// are in the retrieved file set.
///
/// dep_coverage = |covered_deps| / |expected_deps|
/// where covered = source ∈ retrieved AND target ∈ retrieved
pub fn dep_coverage(expected_deps: &[DepEdge], retrieved_files: &[String]) -> f64 {
    if expected_deps.is_empty() {
        return 1.0; // No deps expected → fully covered
    }
    let retrieved: HashSet<&str> = retrieved_files.iter().map(|s| s.as_str()).collect();
    let covered = expected_deps.iter().filter(|dep| {
        retrieved.contains(dep.source.as_str()) && retrieved.contains(dep.target.as_str())
    }).count();
    covered as f64 / expected_deps.len() as f64
}

/// Missing Dependency Rate: fraction of expected deps NOT covered.
///
/// missing_dep_rate = 1.0 - dep_coverage
pub fn missing_dep_rate(expected_deps: &[DepEdge], retrieved_files: &[String]) -> f64 {
    1.0 - dep_coverage(expected_deps, retrieved_files)
}

// ---------------------------------------------------------------------------
// Aggregate metrics struct
// ---------------------------------------------------------------------------

/// All retrieval metrics for a single query or aggregated across queries.
#[derive(Debug, Clone, Default)]
pub struct RetrievalMetrics {
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub precision_at_5: f64,
    pub mrr: f64,
    pub hit_rate_at_5: f64,
    pub hit_rate_at_10: f64,
    pub ndcg_at_5: f64,
    pub ndcg_at_10: f64,
    pub average_precision: f64,
    pub dep_coverage: f64,
    pub missing_dep_rate: f64,
}

impl RetrievalMetrics {
    /// Compute all metrics for a single query result.
    pub fn compute(
        returned_files: &[String],
        expected_files: &[&str],
        expected_deps: &[DepEdge],
    ) -> Self {
        let r5 = recall_at_k(returned_files, expected_files, 5);
        let r10 = recall_at_k(returned_files, expected_files, 10);
        let p5 = precision_at_k(returned_files, expected_files, 5);
        let m = mrr(returned_files, expected_files);
        let h5 = hit_at_k(returned_files, expected_files, 5);
        let h10 = hit_at_k(returned_files, expected_files, 10);
        let n5 = ndcg_at_k(returned_files, expected_files, 5);
        let n10 = ndcg_at_k(returned_files, expected_files, 10);
        let ap = average_precision(returned_files, expected_files);
        let dc = dep_coverage(expected_deps, returned_files);
        let mdr = missing_dep_rate(expected_deps, returned_files);

        RetrievalMetrics {
            recall_at_5: r5,
            recall_at_10: r10,
            precision_at_5: p5,
            mrr: m,
            hit_rate_at_5: h5,
            hit_rate_at_10: h10,
            ndcg_at_5: n5,
            ndcg_at_10: n10,
            average_precision: ap,
            dep_coverage: dc,
            missing_dep_rate: mdr,
        }
    }

    /// Average multiple metric sets (for aggregation across queries).
    pub fn average(metrics: &[RetrievalMetrics]) -> Self {
        if metrics.is_empty() {
            return Self::default();
        }
        let n = metrics.len() as f64;
        RetrievalMetrics {
            recall_at_5: metrics.iter().map(|m| m.recall_at_5).sum::<f64>() / n,
            recall_at_10: metrics.iter().map(|m| m.recall_at_10).sum::<f64>() / n,
            precision_at_5: metrics.iter().map(|m| m.precision_at_5).sum::<f64>() / n,
            mrr: metrics.iter().map(|m| m.mrr).sum::<f64>() / n,
            hit_rate_at_5: metrics.iter().map(|m| m.hit_rate_at_5).sum::<f64>() / n,
            hit_rate_at_10: metrics.iter().map(|m| m.hit_rate_at_10).sum::<f64>() / n,
            ndcg_at_5: metrics.iter().map(|m| m.ndcg_at_5).sum::<f64>() / n,
            ndcg_at_10: metrics.iter().map(|m| m.ndcg_at_10).sum::<f64>() / n,
            average_precision: metrics.iter().map(|m| m.average_precision).sum::<f64>() / n,
            dep_coverage: metrics.iter().map(|m| m.dep_coverage).sum::<f64>() / n,
            missing_dep_rate: metrics.iter().map(|m| m.missing_dep_rate).sum::<f64>() / n,
        }
    }

    /// Format as a summary string.
    pub fn summary(&self) -> String {
        format!(
            "R@5={:.3} R@10={:.3} P@5={:.3} MRR={:.3} Hit@5={:.3} Hit@10={:.3} nDCG@5={:.3} nDCG@10={:.3} MAP={:.3} DepCov={:.3} MissDep={:.3}",
            self.recall_at_5, self.recall_at_10, self.precision_at_5, self.mrr,
            self.hit_rate_at_5, self.hit_rate_at_10, self.ndcg_at_5, self.ndcg_at_10,
            self.average_precision, self.dep_coverage, self.missing_dep_rate,
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: create file list from strings
    fn files(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // --- Recall@K ---

    #[test]
    fn recall_at_k_perfect() {
        let returned = files(&["a.rs", "b.rs", "c.rs"]);
        assert!((recall_at_k(&returned, &["a.rs", "b.rs"], 5) - 1.0).abs() < 0.001);
    }

    #[test]
    fn recall_at_k_partial() {
        let returned = files(&["a.rs", "x.rs", "y.rs"]);
        assert!((recall_at_k(&returned, &["a.rs", "b.rs"], 5) - 0.5).abs() < 0.001);
    }

    #[test]
    fn recall_at_k_zero() {
        let returned = files(&["x.rs", "y.rs"]);
        assert!((recall_at_k(&returned, &["a.rs", "b.rs"], 5) - 0.0).abs() < 0.001);
    }

    #[test]
    fn recall_at_k_empty_expected() {
        let returned = files(&["a.rs"]);
        assert!((recall_at_k(&returned, &[], 5) - 0.0).abs() < 0.001);
    }

    // --- Precision@K ---

    #[test]
    fn precision_at_k_perfect() {
        let returned = files(&["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"]);
        // 5 returned, 2 relevant in top-5 → 2/5 = 0.4
        assert!((precision_at_k(&returned, &["a.rs", "b.rs"], 5) - 0.4).abs() < 0.001);
    }

    #[test]
    fn precision_at_k_zero() {
        assert!((precision_at_k(&files(&["x.rs"]), &["a.rs"], 5) - 0.0).abs() < 0.001);
    }

    // --- Hit@K ---

    #[test]
    fn hit_at_k_found() {
        let returned = files(&["x.rs", "a.rs"]);
        assert!((hit_at_k(&returned, &["a.rs"], 5) - 1.0).abs() < 0.001);
    }

    #[test]
    fn hit_at_k_not_found() {
        let returned = files(&["x.rs", "y.rs"]);
        assert!((hit_at_k(&returned, &["a.rs"], 5) - 0.0).abs() < 0.001);
    }

    #[test]
    fn hit_at_k_outside_k() {
        // Relevant at position 6, K=5 → miss
        let returned = files(&["x1", "x2", "x3", "x4", "x5", "a.rs"]);
        assert!((hit_at_k(&returned, &["a.rs"], 5) - 0.0).abs() < 0.001);
    }

    // --- MRR ---

    #[test]
    fn mrr_first_position() {
        let returned = files(&["a.rs", "b.rs"]);
        assert!((mrr(&returned, &["a.rs"]) - 1.0).abs() < 0.001);
    }

    #[test]
    fn mrr_second_position() {
        let returned = files(&["x.rs", "a.rs"]);
        assert!((mrr(&returned, &["a.rs"]) - 0.5).abs() < 0.001);
    }

    #[test]
    fn mrr_not_found() {
        let returned = files(&["x.rs", "y.rs"]);
        assert!((mrr(&returned, &["a.rs"]) - 0.0).abs() < 0.001);
    }

    // --- nDCG@K ---
    // Hand-computed: binary relevance, log2(i+2) discount

    #[test]
    fn ndcg_perfect_single() {
        // 1 relevant at position 0: DCG = 1/log2(2) = 1.0, IDCG = 1.0 → nDCG = 1.0
        let returned = files(&["a.rs", "x.rs"]);
        assert!((ndcg_at_k(&returned, &["a.rs"], 5) - 1.0).abs() < 0.001);
    }

    #[test]
    fn ndcg_second_position() {
        // 1 relevant at position 1: DCG = 1/log2(3) = 0.6309
        // IDCG = 1/log2(2) = 1.0
        // nDCG = 0.6309
        let returned = files(&["x.rs", "a.rs"]);
        assert!((ndcg_at_k(&returned, &["a.rs"], 5) - 0.6309).abs() < 0.01);
    }

    #[test]
    fn ndcg_two_relevant_perfect_order() {
        // 2 relevant at positions 0, 1
        // DCG = 1/log2(2) + 1/log2(3) = 1.0 + 0.6309 = 1.6309
        // IDCG = same (ideal order = actual)
        // nDCG = 1.0
        let returned = files(&["a.rs", "b.rs", "x.rs"]);
        assert!((ndcg_at_k(&returned, &["a.rs", "b.rs"], 5) - 1.0).abs() < 0.001);
    }

    #[test]
    fn ndcg_two_relevant_reversed() {
        // 2 relevant expected, positions 1 and 2 in returned (0-indexed)
        // DCG = 0 + 1/log2(3) + 1/log2(4) = 0 + 0.6309 + 0.5 = 1.1309
        // IDCG = 1/log2(2) + 1/log2(3) = 1.0 + 0.6309 = 1.6309
        // nDCG = 1.1309 / 1.6309 = 0.6934
        let returned = files(&["x.rs", "a.rs", "b.rs"]);
        assert!((ndcg_at_k(&returned, &["a.rs", "b.rs"], 5) - 0.6934).abs() < 0.01);
    }

    #[test]
    fn ndcg_zero() {
        let returned = files(&["x.rs", "y.rs"]);
        assert!((ndcg_at_k(&returned, &["a.rs"], 5) - 0.0).abs() < 0.001);
    }

    #[test]
    fn ndcg_empty_expected() {
        let returned = files(&["a.rs"]);
        assert!((ndcg_at_k(&returned, &[], 5) - 0.0).abs() < 0.001);
    }

    // --- Average Precision ---

    #[test]
    fn ap_perfect_two() {
        // 2 relevant at positions 0, 1
        // AP = (1/2) * (precision@1 + precision@2) = (1/2) * (1/1 + 2/2) = 1.0
        let returned = files(&["a.rs", "b.rs", "x.rs"]);
        assert!((average_precision(&returned, &["a.rs", "b.rs"]) - 1.0).abs() < 0.001);
    }

    #[test]
    fn ap_one_at_second() {
        // 1 relevant at position 1
        // AP = (1/1) * (precision@2) = 1/2 = 0.5
        let returned = files(&["x.rs", "a.rs"]);
        assert!((average_precision(&returned, &["a.rs"]) - 0.5).abs() < 0.001);
    }

    #[test]
    fn ap_two_interleaved() {
        // 2 relevant at positions 0 and 2
        // precision@1 = 1/1, precision@3 = 2/3
        // AP = (1/2) * (1.0 + 0.6667) = 0.8333
        let returned = files(&["a.rs", "x.rs", "b.rs"]);
        assert!((average_precision(&returned, &["a.rs", "b.rs"]) - 0.8333).abs() < 0.01);
    }

    #[test]
    fn ap_zero() {
        let returned = files(&["x.rs", "y.rs"]);
        assert!((average_precision(&returned, &["a.rs"]) - 0.0).abs() < 0.001);
    }

    // --- Dependency Coverage ---

    #[test]
    fn dep_coverage_full() {
        let deps = vec![
            DepEdge { source: "a.rs".into(), target: "b.rs".into(), edge_type: "Imports".into() },
        ];
        let retrieved = files(&["a.rs", "b.rs", "c.rs"]);
        assert!((dep_coverage(&deps, &retrieved) - 1.0).abs() < 0.001);
    }

    #[test]
    fn dep_coverage_partial() {
        let deps = vec![
            DepEdge { source: "a.rs".into(), target: "b.rs".into(), edge_type: "Imports".into() },
            DepEdge { source: "a.rs".into(), target: "c.rs".into(), edge_type: "Calls".into() },
        ];
        // Only a.rs and b.rs retrieved, not c.rs → 1/2 = 0.5
        let retrieved = files(&["a.rs", "b.rs"]);
        assert!((dep_coverage(&deps, &retrieved) - 0.5).abs() < 0.001);
    }

    #[test]
    fn dep_coverage_zero() {
        let deps = vec![
            DepEdge { source: "a.rs".into(), target: "b.rs".into(), edge_type: "Imports".into() },
        ];
        let retrieved = files(&["x.rs"]);
        assert!((dep_coverage(&deps, &retrieved) - 0.0).abs() < 0.001);
    }

    #[test]
    fn dep_coverage_no_deps() {
        let retrieved = files(&["a.rs"]);
        assert!((dep_coverage(&[], &retrieved) - 1.0).abs() < 0.001);
    }

    // --- Missing Dep Rate ---

    #[test]
    fn missing_dep_rate_zero() {
        let deps = vec![
            DepEdge { source: "a.rs".into(), target: "b.rs".into(), edge_type: "Imports".into() },
        ];
        let retrieved = files(&["a.rs", "b.rs"]);
        assert!((missing_dep_rate(&deps, &retrieved) - 0.0).abs() < 0.001);
    }

    #[test]
    fn missing_dep_rate_full() {
        let deps = vec![
            DepEdge { source: "a.rs".into(), target: "b.rs".into(), edge_type: "Imports".into() },
        ];
        let retrieved = files(&["x.rs"]);
        assert!((missing_dep_rate(&deps, &retrieved) - 1.0).abs() < 0.001);
    }

    // --- RetrievalMetrics ---

    #[test]
    fn metrics_compute() {
        let returned = files(&["a.rs", "x.rs", "b.rs"]);
        let expected = &["a.rs", "b.rs"];
        let deps = vec![
            DepEdge { source: "a.rs".into(), target: "b.rs".into(), edge_type: "Imports".into() },
        ];
        let m = RetrievalMetrics::compute(&returned, expected, &deps);
        assert!(m.recall_at_5 > 0.9); // both found in top-3
        assert!(m.hit_rate_at_5 > 0.9); // at least 1 found
        assert!(m.mrr > 0.9); // first at position 0
        assert!(m.dep_coverage > 0.9); // both a.rs and b.rs retrieved
    }

    #[test]
    fn metrics_average() {
        let m1 = RetrievalMetrics { recall_at_5: 1.0, mrr: 1.0, ..Default::default() };
        let m2 = RetrievalMetrics { recall_at_5: 0.5, mrr: 0.5, ..Default::default() };
        let avg = RetrievalMetrics::average(&[m1, m2]);
        assert!((avg.recall_at_5 - 0.75).abs() < 0.001);
        assert!((avg.mrr - 0.75).abs() < 0.001);
    }
}
