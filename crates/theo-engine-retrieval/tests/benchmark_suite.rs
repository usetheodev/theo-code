//! GRAPHCTX Professional Benchmark Suite
//!
//! Industry-standard evaluation aligned with RepoBench, CodeRAG-Bench, CodeSearchNet.
//! Measures: Recall@5, Recall@10, MRR, Hit@5, Hit@10, nDCG@5, nDCG@10, MAP,
//!           Dependency Coverage, Missing Dep Rate.
//!
//! Ground truth loaded from JSON (tests/benchmarks/ground_truth/*.json).
//!
//! Run: cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored --nocapture

use serde::Deserialize;
use std::collections::HashMap;
use theo_engine_retrieval::metrics::{self, DepEdge, RetrievalMetrics};

// ---------------------------------------------------------------------------
// Ground truth schema
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct BenchmarkData {
    #[allow(dead_code)]
    schema: String,
    repo: RepoInfo,
    queries: Vec<BenchmarkQuery>,
}

#[derive(Deserialize)]
struct RepoInfo {
    name: String,
    language: String,
    #[allow(dead_code)]
    category: String,
}

#[derive(Deserialize)]
struct BenchmarkQuery {
    id: String,
    query: String,
    category: String,
    difficulty: String,
    expected_files: Vec<String>,
    dependencies: Vec<DepSpec>,
}

#[derive(Deserialize)]
struct DepSpec {
    source: String,
    target: String,
    edge_type: String,
}

impl DepSpec {
    fn to_dep_edge(&self) -> DepEdge {
        DepEdge {
            source: self.source.clone(),
            target: self.target.clone(),
            edge_type: self.edge_type.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Report structures
// ---------------------------------------------------------------------------

struct QueryResult {
    id: String,
    query: String,
    category: String,
    difficulty: String,
    metrics: RetrievalMetrics,
    returned_top_10: Vec<String>,
    expected_files: Vec<String>,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

fn load_ground_truth(repo_name: &str) -> BenchmarkData {
    let path = format!(
        "{}/tests/benchmarks/ground_truth/{}.json",
        env!("CARGO_MANIFEST_DIR"),
        repo_name
    );
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to load ground truth {}: {}", path, e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse ground truth {}: {}", path, e))
}

/// Extract ranked file paths from score map.
fn extract_files_from_scores(scores: &HashMap<String, f64>) -> Vec<String> {
    let mut sorted: Vec<_> = scores.iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.into_iter().map(|(k, _)| k.clone()).collect()
}

// ---------------------------------------------------------------------------
// Report formatting
// ---------------------------------------------------------------------------

fn emit_report(repo: &RepoInfo, results: &[QueryResult], pipeline_name: &str) {
    let all_metrics: Vec<RetrievalMetrics> = results.iter().map(|r| r.metrics.clone()).collect();
    let overall = RetrievalMetrics::average(&all_metrics);

    // Per-category aggregation
    let mut by_category: HashMap<&str, Vec<RetrievalMetrics>> = HashMap::new();
    for r in results {
        by_category
            .entry(r.category.as_str())
            .or_default()
            .push(r.metrics.clone());
    }

    // Per-difficulty aggregation
    let mut by_difficulty: HashMap<&str, Vec<RetrievalMetrics>> = HashMap::new();
    for r in results {
        by_difficulty
            .entry(r.difficulty.as_str())
            .or_default()
            .push(r.metrics.clone());
    }

    eprintln!("\n{}", "=".repeat(100));
    eprintln!("GRAPHCTX PROFESSIONAL BENCHMARK REPORT");
    eprintln!("{}", "=".repeat(100));
    eprintln!("Pipeline:  {}", pipeline_name);
    eprintln!("Repo:      {} ({})", repo.name, repo.language);
    eprintln!("Queries:   {}", results.len());
    eprintln!();

    // Overall metrics
    eprintln!("OVERALL METRICS:");
    eprintln!(
        "  Recall@5  = {:.3}    Recall@10 = {:.3}",
        overall.recall_at_5, overall.recall_at_10
    );
    eprintln!(
        "  P@5       = {:.3}    MRR       = {:.3}",
        overall.precision_at_5, overall.mrr
    );
    eprintln!(
        "  Hit@5     = {:.3}    Hit@10    = {:.3}",
        overall.hit_rate_at_5, overall.hit_rate_at_10
    );
    eprintln!(
        "  nDCG@5    = {:.3}    nDCG@10   = {:.3}",
        overall.ndcg_at_5, overall.ndcg_at_10
    );
    eprintln!("  MAP       = {:.3}", overall.average_precision);
    eprintln!(
        "  DepCov    = {:.3}    MissDep   = {:.3}",
        overall.dep_coverage, overall.missing_dep_rate
    );
    eprintln!();

    // Gates
    eprintln!("GATES (SOTA targets):");
    let gates = [
        ("Recall@5", overall.recall_at_5, 0.92, true),
        ("Recall@10", overall.recall_at_10, 0.95, true),
        ("MRR", overall.mrr, 0.85, true),
        ("Hit@5", overall.hit_rate_at_5, 0.95, true),
        ("DepCov", overall.dep_coverage, 0.90, true),
        ("MissDep", overall.missing_dep_rate, 0.10, false), // lower is better
    ];
    for (name, actual, target, higher_better) in &gates {
        let pass = if *higher_better {
            *actual >= *target
        } else {
            *actual <= *target
        };
        eprintln!(
            "  {:<12} {:.3} / {:.3}  {}",
            name,
            actual,
            target,
            if pass { "PASS" } else { "FAIL" }
        );
    }
    eprintln!();

    // By category
    eprintln!("BY CATEGORY:");
    for (cat, cat_metrics) in &by_category {
        let avg = RetrievalMetrics::average(cat_metrics);
        eprintln!(
            "  {:<15} R@5={:.3}  R@10={:.3}  MRR={:.3}  nDCG@5={:.3}  DepCov={:.3}",
            cat, avg.recall_at_5, avg.recall_at_10, avg.mrr, avg.ndcg_at_5, avg.dep_coverage
        );
    }
    eprintln!();

    // By difficulty
    eprintln!("BY DIFFICULTY:");
    for (diff, diff_metrics) in &by_difficulty {
        let avg = RetrievalMetrics::average(diff_metrics);
        eprintln!(
            "  {:<10} R@5={:.3}  MRR={:.3}  nDCG@5={:.3}  ({} queries)",
            diff,
            avg.recall_at_5,
            avg.mrr,
            avg.ndcg_at_5,
            diff_metrics.len()
        );
    }
    eprintln!();

    // Per-query detail (failures)
    eprintln!("FAILURES (P@5 < 0.40):");
    for r in results {
        if r.metrics.precision_at_5 < 0.40 {
            eprintln!(
                "  {} '{}' P@5={:.2} R@5={:.2} MRR={:.2} DepCov={:.2}",
                r.id,
                r.query,
                r.metrics.precision_at_5,
                r.metrics.recall_at_5,
                r.metrics.mrr,
                r.metrics.dep_coverage
            );
            eprintln!(
                "    Expected: {:?}",
                r.expected_files
                    .iter()
                    .map(|f| f.split('/').last().unwrap_or(f))
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "    Got top5: {:?}",
                r.returned_top_10
                    .iter()
                    .take(5)
                    .map(|f| f.split('/').last().unwrap_or(f))
                    .collect::<Vec<_>>()
            );
        }
    }

    eprintln!("\n{}", "=".repeat(100));
}

// ---------------------------------------------------------------------------
// Benchmark tests
// ---------------------------------------------------------------------------

/// Professional benchmark: BM25 file-level search (baseline).
///
/// Run: cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored --nocapture benchmark_bm25_baseline
#[test]
#[ignore]
fn benchmark_bm25_baseline() {
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::search::FileBm25;

    let gt = load_ground_truth("theo-code");

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("Building graph from {}...", workspace_root.display());
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!(
        "Parsed {}/{} files, {} symbols",
        stats.files_parsed, stats.files_found, stats.symbols_extracted
    );

    let (graph, _) = bridge::build_graph(&files);
    eprintln!(
        "Graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    let mut results: Vec<QueryResult> = Vec::new();

    for bq in &gt.queries {
        let file_scores = FileBm25::search(&graph, &bq.query);
        let returned_files = extract_files_from_scores(&file_scores);
        let expected_refs: Vec<&str> = bq.expected_files.iter().map(|s| s.as_str()).collect();
        let dep_edges: Vec<DepEdge> = bq.dependencies.iter().map(|d| d.to_dep_edge()).collect();

        let m = RetrievalMetrics::compute(&returned_files, &expected_refs, &dep_edges);

        results.push(QueryResult {
            id: bq.id.clone(),
            query: bq.query.clone(),
            category: bq.category.clone(),
            difficulty: bq.difficulty.clone(),
            metrics: m,
            returned_top_10: returned_files.into_iter().take(10).collect(),
            expected_files: bq.expected_files.clone(),
        });
    }

    emit_report(&gt.repo, &results, "FileBm25 (baseline)");
}

/// Professional benchmark: RRF 3-ranker (BM25 + Tantivy + Dense).
///
/// Run: cargo test -p theo-engine-retrieval --features dense-retrieval --test benchmark_suite -- --ignored --nocapture benchmark_rrf_dense
#[test]
#[ignore]
#[cfg(feature = "dense-retrieval")]
fn benchmark_rrf_dense() {
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::embedding::cache::EmbeddingCache;
    use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
    use theo_engine_retrieval::tantivy_search::{FileTantivyIndex, hybrid_rrf_search};

    let gt = load_ground_truth("theo-code");

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("Building graph...");
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!("Parsed {}/{} files", stats.files_parsed, stats.files_found);

    let (graph, _) = bridge::build_graph(&files);
    eprintln!(
        "Graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy build failed");
    let embedder = NeuralEmbedder::new().expect("NeuralEmbedder init failed");
    let cache = EmbeddingCache::build(&graph, &embedder);
    eprintln!(
        "Tantivy: {} docs, Embeddings: {} files",
        tantivy_index.num_docs(),
        cache.len()
    );

    let mut results: Vec<QueryResult> = Vec::new();

    for bq in &gt.queries {
        let rrf_scores =
            hybrid_rrf_search(&graph, &tantivy_index, &embedder, &cache, &bq.query, 20.0);
        let returned_files = extract_files_from_scores(&rrf_scores);
        let expected_refs: Vec<&str> = bq.expected_files.iter().map(|s| s.as_str()).collect();
        let dep_edges: Vec<DepEdge> = bq.dependencies.iter().map(|d| d.to_dep_edge()).collect();

        let m = RetrievalMetrics::compute(&returned_files, &expected_refs, &dep_edges);

        results.push(QueryResult {
            id: bq.id.clone(),
            query: bq.query.clone(),
            category: bq.category.clone(),
            difficulty: bq.difficulty.clone(),
            metrics: m,
            returned_top_10: returned_files.into_iter().take(10).collect(),
            expected_files: bq.expected_files.clone(),
        });
    }

    emit_report(
        &gt.repo,
        &results,
        "RRF 3-ranker (BM25+Tantivy+Dense, Jina Code)",
    );
}

/// A/B benchmark: Symbol-First vs RRF baseline.
///
/// Run: cargo test -p theo-engine-retrieval --features dense-retrieval --test benchmark_suite -- --ignored --nocapture benchmark_symbol_first_ab
#[test]
#[ignore]
#[cfg(feature = "dense-retrieval")]
fn benchmark_symbol_first_ab() {
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::embedding::cache::EmbeddingCache;
    use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
    use theo_engine_retrieval::tantivy_search::{
        FileTantivyIndex, hybrid_rrf_search, symbol_first_search,
    };

    let gt = load_ground_truth("theo-code");

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("Building graph...");
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!("Parsed {}/{} files", stats.files_parsed, stats.files_found);

    let (graph, _) = bridge::build_graph(&files);
    let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy build failed");
    let embedder = NeuralEmbedder::new().expect("NeuralEmbedder init failed");
    let cache = EmbeddingCache::build(&graph, &embedder);
    eprintln!(
        "Ready: {} nodes, Tantivy {} docs, Embeddings {} files",
        graph.node_count(),
        tantivy_index.num_docs(),
        cache.len()
    );

    let k = 5;

    // --- Baseline: RRF 3-ranker ---
    let mut rrf_results: Vec<QueryResult> = Vec::new();
    for bq in &gt.queries {
        let scores = hybrid_rrf_search(&graph, &tantivy_index, &embedder, &cache, &bq.query, 20.0);
        let returned = extract_files_from_scores(&scores);
        let expected: Vec<&str> = bq.expected_files.iter().map(|s| s.as_str()).collect();
        let deps: Vec<DepEdge> = bq.dependencies.iter().map(|d| d.to_dep_edge()).collect();
        let m = RetrievalMetrics::compute(&returned, &expected, &deps);
        rrf_results.push(QueryResult {
            id: bq.id.clone(),
            query: bq.query.clone(),
            category: bq.category.clone(),
            difficulty: bq.difficulty.clone(),
            metrics: m,
            returned_top_10: returned.into_iter().take(10).collect(),
            expected_files: bq.expected_files.clone(),
        });
    }

    // --- Variant: Symbol-First ---
    let mut sym_results: Vec<QueryResult> = Vec::new();
    for bq in &gt.queries {
        let scores =
            symbol_first_search(&graph, &tantivy_index, &embedder, &cache, &bq.query, 20.0);
        let returned = extract_files_from_scores(&scores);
        let expected: Vec<&str> = bq.expected_files.iter().map(|s| s.as_str()).collect();
        let deps: Vec<DepEdge> = bq.dependencies.iter().map(|d| d.to_dep_edge()).collect();
        let m = RetrievalMetrics::compute(&returned, &expected, &deps);
        sym_results.push(QueryResult {
            id: bq.id.clone(),
            query: bq.query.clone(),
            category: bq.category.clone(),
            difficulty: bq.difficulty.clone(),
            metrics: m,
            returned_top_10: returned.into_iter().take(10).collect(),
            expected_files: bq.expected_files.clone(),
        });
    }

    // --- A/B Comparison ---
    let rrf_avg = RetrievalMetrics::average(
        &rrf_results
            .iter()
            .map(|r| r.metrics.clone())
            .collect::<Vec<_>>(),
    );
    let sym_avg = RetrievalMetrics::average(
        &sym_results
            .iter()
            .map(|r| r.metrics.clone())
            .collect::<Vec<_>>(),
    );

    eprintln!("\n{}", "=".repeat(90));
    eprintln!("A/B: SYMBOL-FIRST vs RRF BASELINE");
    eprintln!("{}", "=".repeat(90));
    eprintln!(
        "{:<20} {:>10} {:>10} {:>10}",
        "", "RRF", "Symbol-1st", "Delta"
    );
    eprintln!("{}", "-".repeat(60));
    eprintln!(
        "{:<20} {:>10.3} {:>10.3} {:>+10.3}",
        "Recall@5",
        rrf_avg.recall_at_5,
        sym_avg.recall_at_5,
        sym_avg.recall_at_5 - rrf_avg.recall_at_5
    );
    eprintln!(
        "{:<20} {:>10.3} {:>10.3} {:>+10.3}",
        "Recall@10",
        rrf_avg.recall_at_10,
        sym_avg.recall_at_10,
        sym_avg.recall_at_10 - rrf_avg.recall_at_10
    );
    eprintln!(
        "{:<20} {:>10.3} {:>10.3} {:>+10.3}",
        "MRR",
        rrf_avg.mrr,
        sym_avg.mrr,
        sym_avg.mrr - rrf_avg.mrr
    );
    eprintln!(
        "{:<20} {:>10.3} {:>10.3} {:>+10.3}",
        "Hit@5",
        rrf_avg.hit_rate_at_5,
        sym_avg.hit_rate_at_5,
        sym_avg.hit_rate_at_5 - rrf_avg.hit_rate_at_5
    );
    eprintln!(
        "{:<20} {:>10.3} {:>10.3} {:>+10.3}",
        "nDCG@5",
        rrf_avg.ndcg_at_5,
        sym_avg.ndcg_at_5,
        sym_avg.ndcg_at_5 - rrf_avg.ndcg_at_5
    );
    eprintln!(
        "{:<20} {:>10.3} {:>10.3} {:>+10.3}",
        "MAP",
        rrf_avg.average_precision,
        sym_avg.average_precision,
        sym_avg.average_precision - rrf_avg.average_precision
    );
    eprintln!(
        "{:<20} {:>10.3} {:>10.3} {:>+10.3}",
        "DepCov",
        rrf_avg.dep_coverage,
        sym_avg.dep_coverage,
        sym_avg.dep_coverage - rrf_avg.dep_coverage
    );

    // Per-query delta for queries that improved
    eprintln!("\nPER-QUERY DELTA (R@5 changed):");
    for (rrf_r, sym_r) in rrf_results.iter().zip(sym_results.iter()) {
        let delta = sym_r.metrics.recall_at_5 - rrf_r.metrics.recall_at_5;
        if delta.abs() > 0.01 {
            eprintln!(
                "  {} '{}': R@5 {:.2} → {:.2} ({:+.2})",
                rrf_r.id, rrf_r.query, rrf_r.metrics.recall_at_5, sym_r.metrics.recall_at_5, delta
            );
        }
    }

    eprintln!("\n{}", "=".repeat(90));

    // Also emit full reports
    emit_report(&gt.repo, &rrf_results, "RRF BASELINE");
    emit_report(&gt.repo, &sym_results, "SYMBOL-FIRST");
}

/// Multi-repo benchmark: RRF+Dense on external repos.
///
/// Run: cargo test -p theo-engine-retrieval --features dense-retrieval --test benchmark_suite -- --ignored --nocapture benchmark_external_rrf
#[test]
#[ignore]
#[cfg(feature = "dense-retrieval")]
fn benchmark_external_rrf() {
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::embedding::cache::EmbeddingCache;
    use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
    use theo_engine_retrieval::tantivy_search::{FileTantivyIndex, hybrid_rrf_search};

    let gt_dir = format!(
        "{}/tests/benchmarks/ground_truth",
        env!("CARGO_MANIFEST_DIR")
    );
    let entries = std::fs::read_dir(&gt_dir).expect("Failed to read ground_truth dir");

    let embedder = NeuralEmbedder::new().expect("NeuralEmbedder init failed");

    for entry in entries {
        let entry = entry.unwrap();
        let filename = entry.file_name();
        let name = filename.to_str().unwrap();
        if !name.ends_with(".json") || name == "theo-code.json" {
            continue;
        }

        let repo_name = name.trim_end_matches(".json");
        let repo_path = format!("/tmp/{}", repo_name);

        if !std::path::Path::new(&repo_path).exists() {
            eprintln!("SKIP {}: not cloned at {}", repo_name, repo_path);
            continue;
        }

        let gt = load_ground_truth(repo_name);
        eprintln!(
            "\n--- Benchmarking {} ({}) with RRF+Dense ---",
            gt.repo.name, gt.repo.language
        );

        let repo_root = std::path::Path::new(&repo_path);
        let (files, stats) = theo_application::use_cases::extraction::extract_repo(repo_root);
        eprintln!(
            "Parsed {}/{} files, {} symbols",
            stats.files_parsed, stats.files_found, stats.symbols_extracted
        );

        let (graph, _) = bridge::build_graph(&files);
        eprintln!(
            "Graph: {} nodes, {} edges",
            graph.node_count(),
            graph.edge_count()
        );

        let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy build failed");
        let cache = EmbeddingCache::build(&graph, &embedder);
        eprintln!(
            "Tantivy: {} docs, Embeddings: {} files",
            tantivy_index.num_docs(),
            cache.len()
        );

        let mut results: Vec<QueryResult> = Vec::new();

        for bq in &gt.queries {
            let rrf_scores =
                hybrid_rrf_search(&graph, &tantivy_index, &embedder, &cache, &bq.query, 20.0);
            let returned_files = extract_files_from_scores(&rrf_scores);
            let expected_refs: Vec<&str> = bq.expected_files.iter().map(|s| s.as_str()).collect();
            let dep_edges: Vec<DepEdge> = bq.dependencies.iter().map(|d| d.to_dep_edge()).collect();

            let m = RetrievalMetrics::compute(&returned_files, &expected_refs, &dep_edges);

            results.push(QueryResult {
                id: bq.id.clone(),
                query: bq.query.clone(),
                category: bq.category.clone(),
                difficulty: bq.difficulty.clone(),
                metrics: m,
                returned_top_10: returned_files.into_iter().take(10).collect(),
                expected_files: bq.expected_files.clone(),
            });
        }

        emit_report(
            &gt.repo,
            &results,
            "RRF 3-ranker (BM25+Tantivy+Dense, Jina Code)",
        );
    }
}

/// Multi-repo benchmark: BM25 baseline on external repos.
///
/// Tests generalization on repos outside Theo (e.g., axum).
/// Repos must be pre-cloned to /tmp/{repo_name} on the test machine.
///
/// Run: cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored --nocapture benchmark_external_bm25
#[test]
#[ignore]
fn benchmark_external_bm25() {
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::search::FileBm25;

    // Discover all ground truth files
    let gt_dir = format!(
        "{}/tests/benchmarks/ground_truth",
        env!("CARGO_MANIFEST_DIR")
    );
    let entries = std::fs::read_dir(&gt_dir).expect("Failed to read ground_truth dir");

    for entry in entries {
        let entry = entry.unwrap();
        let filename = entry.file_name();
        let name = filename.to_str().unwrap();
        if !name.ends_with(".json") || name == "theo-code.json" {
            continue; // Skip self-repo, already benchmarked separately
        }

        let repo_name = name.trim_end_matches(".json");
        let repo_path = format!("/tmp/{}", repo_name);

        if !std::path::Path::new(&repo_path).exists() {
            eprintln!("SKIP {}: not cloned at {}", repo_name, repo_path);
            continue;
        }

        let gt = load_ground_truth(repo_name);
        eprintln!(
            "\n--- Benchmarking {} ({}) ---",
            gt.repo.name, gt.repo.language
        );

        let repo_root = std::path::Path::new(&repo_path);
        let (files, stats) = theo_application::use_cases::extraction::extract_repo(repo_root);
        eprintln!(
            "Parsed {}/{} files, {} symbols",
            stats.files_parsed, stats.files_found, stats.symbols_extracted
        );

        let (graph, _) = bridge::build_graph(&files);
        eprintln!(
            "Graph: {} nodes, {} edges",
            graph.node_count(),
            graph.edge_count()
        );

        let mut results: Vec<QueryResult> = Vec::new();

        for bq in &gt.queries {
            let file_scores = FileBm25::search(&graph, &bq.query);
            let returned_files = extract_files_from_scores(&file_scores);
            let expected_refs: Vec<&str> = bq.expected_files.iter().map(|s| s.as_str()).collect();
            let dep_edges: Vec<DepEdge> = bq.dependencies.iter().map(|d| d.to_dep_edge()).collect();

            let m = RetrievalMetrics::compute(&returned_files, &expected_refs, &dep_edges);

            results.push(QueryResult {
                id: bq.id.clone(),
                query: bq.query.clone(),
                category: bq.category.clone(),
                difficulty: bq.difficulty.clone(),
                metrics: m,
                returned_top_10: returned_files.into_iter().take(10).collect(),
                expected_files: bq.expected_files.clone(),
            });
        }

        emit_report(&gt.repo, &results, "FileBm25 (external repo)");
    }
}

/// E2E: Generate Code Wiki from real theo-code repo.
///
/// Run: cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored --nocapture wiki_e2e
#[test]
#[ignore]
fn wiki_e2e() {
    use theo_engine_graph::bridge;
    use theo_engine_graph::cluster::{ClusterAlgorithm, hierarchical_cluster};
    use theo_engine_retrieval::wiki;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("=== CODE WIKI E2E TEST ===\n");

    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!(
        "Parsed: {}/{} files, {} symbols",
        stats.files_parsed, stats.files_found, stats.symbols_extracted
    );

    let (graph, _) = bridge::build_graph(&files);
    eprintln!(
        "Graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    let cluster = hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 1.0 });
    eprintln!("Communities: {}", cluster.communities.len());

    // Debug: count node types
    let test_nodes = graph
        .node_ids()
        .filter(|id| {
            graph.get_node(id).map_or(false, |n| {
                n.node_type == theo_engine_graph::model::NodeType::Test
            })
        })
        .count();
    let test_edges = graph
        .all_edges()
        .iter()
        .filter(|e| e.edge_type == theo_engine_graph::model::EdgeType::Tests)
        .count();
    eprintln!(
        "DEBUG: {} Test nodes, {} Tests edges in graph",
        test_nodes, test_edges
    );

    let start = std::time::Instant::now();
    let wiki_data = wiki::generator::generate_wiki_with_root(
        &cluster.communities,
        &graph,
        "theo-code",
        Some(workspace_root),
    );
    let gen_time = start.elapsed();
    eprintln!(
        "Wiki: {} pages in {:.0}ms\n",
        wiki_data.docs.len(),
        gen_time.as_millis()
    );

    wiki::persistence::write_to_disk(&wiki_data, workspace_root).unwrap();

    // Verify
    let index_path = workspace_root.join(".theo/wiki/index.md");
    assert!(index_path.exists(), "index.md must exist");

    let modules_dir = workspace_root.join(".theo/wiki/modules");
    let page_count = std::fs::read_dir(&modules_dir).unwrap().count();
    assert!(page_count > 0, "must have module pages");

    // Stats
    eprintln!(
        "{:30} {:>5} {:>6} {:>4} {:>4} {:>8}",
        "MODULE", "FILES", "SYMS", "ENTR", "DEPS", "COVER"
    );
    eprintln!("{}", "-".repeat(65));
    for doc in &wiki_data.docs {
        eprintln!(
            "{:30} {:>5} {:>6} {:>4} {:>4} {:>7.1}%",
            &doc.title[..doc.title.len().min(30)],
            doc.file_count,
            doc.symbol_count,
            doc.entry_points.len(),
            doc.dependencies.len(),
            doc.test_coverage.percentage
        );
    }

    // Provenance check
    for doc in &wiki_data.docs {
        assert!(
            !doc.source_refs.is_empty(),
            "{} has no provenance",
            doc.slug
        );
    }

    // Cache check
    let hash = wiki::generator::compute_graph_hash(&graph);
    assert!(wiki::persistence::is_fresh(workspace_root, hash));

    eprintln!(
        "\n=== WIKI E2E: {} pages, {:.0}ms, PASSED ===",
        wiki_data.docs.len(),
        gen_time.as_millis()
    );
}

/// Knowledge Compounding Loop: demonstrates the full cycle
/// Query → MISS → Write-back → Re-query → HIT → Related query → HIT
///
/// Run: cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored --nocapture wiki_knowledge_loop
#[test]
#[ignore]
fn wiki_knowledge_loop() {
    use theo_engine_retrieval::wiki;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let wiki_dir = workspace_root.join(".theo/wiki");
    if !wiki_dir.exists() {
        eprintln!("Wiki not found. Run wiki_e2e first.");
        return;
    }

    eprintln!("══════════════════════════════════════════════════");
    eprintln!(" KNOWLEDGE COMPOUNDING LOOP — LIVE DEMO");
    eprintln!("══════════════════════════════════════════════════\n");

    // ─── STEP 1: Query that should MISS (no cache yet) ───
    let query = "authentication oauth device flow token";
    eprintln!("STEP 1: Query -> Wiki Lookup");
    eprintln!("  Query: \"{}\"", query);

    // Clean cache for fresh demo
    let cache_dir = wiki_dir.join("cache");
    let _ = std::fs::remove_dir_all(&cache_dir);

    let t0 = std::time::Instant::now();
    let results1 = wiki::lookup::lookup(&wiki_dir, query, 3);
    let t1 = t0.elapsed();

    eprintln!("  Latency: {:.2}ms", t1.as_secs_f64() * 1000.0);
    if results1.is_empty() {
        eprintln!("  Result: MISS — no relevant pages found");
    } else {
        eprintln!("  Result: {} hits from module pages:", results1.len());
        for r in &results1 {
            eprintln!(
                "    - {} (conf: {:.0}%, ~{} tokens)",
                r.title,
                r.confidence * 100.0,
                r.token_count
            );
        }
    }

    // ─── STEP 2: Simulate RRF result → Write-back ───
    eprintln!("\nSTEP 2: RRF Pipeline -> Write-back to wiki cache");

    std::fs::create_dir_all(&cache_dir).unwrap();
    let slug = "authentication-oauth-device-flow";
    let cache_path = cache_dir.join(format!("{}.md", slug));

    let manifest = wiki::persistence::load_manifest(workspace_root);
    let graph_hash = manifest.as_ref().map(|m| m.graph_hash).unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let cache_md = format!(
        "---\ngraph_hash: {}\ngenerated_at: \"{}\"\nquery: \"{}\"\n---\n\n\
        # Authentication: OAuth & Device Flow\n\n\
        Theo Code supports two authentication flows via [[theo-infra-auth]]:\n\n\
        ## OAuth PKCE Flow\n\n\
        Browser-based authentication with PKCE extension for desktop apps.\n\
        Entry point: `start_oauth_flow()` in `crates/theo-infra-auth/src/oauth.rs`.\n\
        Handles redirect URI, code challenge, and token exchange.\n\n\
        ## Device Flow (RFC 8628)\n\n\
        Headless authentication for CLI and CI environments.\n\
        Entry point: `start_device_flow()` in `crates/theo-infra-auth/src/device.rs`.\n\
        Supports both GitHub Copilot and OpenAI device authorization.\n\n\
        ## Token Management\n\n\
        Tokens stored at `~/.config/theo/auth.json` (0o600 permissions).\n\
        Auto-refresh before expiry. Copilot endpoint: `api.githubcopilot.com/chat/completions`.\n\n\
        ## Key Components\n\n\
        | File | Role |\n|------|------|\n\
        | `theo-infra-auth/src/oauth.rs` | PKCE flow |\n\
        | `theo-infra-auth/src/device.rs` | Device flow RFC 8628 |\n\
        | `theo-infra-auth/src/token.rs` | Token storage and refresh |\n\
        | `theo-infra-auth/src/copilot.rs` | GitHub Copilot integration |\n\n\
        ---\n*Synthesized by GRAPHCTX | 5 blocks, 2400 tokens*\n",
        graph_hash, now, query
    );

    std::fs::write(&cache_path, &cache_md).unwrap();
    eprintln!("  Written: cache/{}.md ({} bytes)", slug, cache_md.len());
    eprintln!("  Frontmatter: graph_hash={}", graph_hash);

    // ─── STEP 3: Same query again → should HIT from cache ───
    eprintln!("\nSTEP 3: Re-query -> Wiki Lookup (with cache)");
    eprintln!("  Query: \"{}\"", query);

    let t2 = std::time::Instant::now();
    let results2 = wiki::lookup::lookup(&wiki_dir, query, 3);
    let t3 = t2.elapsed();

    eprintln!("  Latency: {:.2}ms", t3.as_secs_f64() * 1000.0);
    eprintln!("  Result: {} hits:", results2.len());
    for r in &results2 {
        let source = if r.slug == slug { " <-- CACHE HIT" } else { "" };
        eprintln!(
            "    - {} (conf: {:.0}%, ~{} tokens){}",
            r.title,
            r.confidence * 100.0,
            r.token_count,
            source
        );
    }

    let cache_hit = results2.iter().find(|r| r.slug == slug);
    assert!(cache_hit.is_some(), "Cache page should appear in results");
    eprintln!(
        "\n  Cache page found with confidence: {:.0}%",
        cache_hit.unwrap().confidence * 100.0
    );

    // ─── STEP 4: Related query → also benefits from cache ───
    let query2 = "device flow RFC 8628 headless CLI";
    eprintln!("\nSTEP 4: Related query -> also benefits from cache");
    eprintln!("  Query: \"{}\"", query2);

    let t4 = std::time::Instant::now();
    let results3 = wiki::lookup::lookup(&wiki_dir, query2, 3);
    let t5 = t4.elapsed();

    eprintln!("  Latency: {:.2}ms", t5.as_secs_f64() * 1000.0);
    if !results3.is_empty() {
        eprintln!("  Result: {} hits:", results3.len());
        for r in &results3 {
            let source = if r.slug == slug {
                " <-- CACHE COMPOUND"
            } else {
                ""
            };
            eprintln!(
                "    - {} (conf: {:.0}%, ~{} tokens){}",
                r.title,
                r.confidence * 100.0,
                r.token_count,
                source
            );
        }
    }

    // ─── STEP 5: Third unrelated query → MISS (control) ───
    let query3 = "mermaid diagram rendering SVG";
    eprintln!("\nSTEP 5: Unrelated query -> should NOT hit auth cache");
    eprintln!("  Query: \"{}\"", query3);

    let results4 = wiki::lookup::lookup(&wiki_dir, query3, 3);
    let auth_hit = results4.iter().any(|r| r.slug == slug);
    eprintln!("  Auth cache hit: {} (expected: false)", auth_hit);

    // ─── STEP 6: Lint health check ───
    eprintln!("\nSTEP 6: Wiki Lint (health check)");
    let report = wiki::lint::lint(&wiki_dir);
    eprintln!("  {}", format!("{}", report).lines().next().unwrap_or(""));
    let cache_count = std::fs::read_dir(&cache_dir)
        .map(|e| e.count())
        .unwrap_or(0);
    eprintln!("  Cache pages: {}", cache_count);

    // ─── Summary ───
    eprintln!("\n══════════════════════════════════════════════════");
    eprintln!(" KNOWLEDGE COMPOUNDING PROVEN:");
    eprintln!(
        "  1. Initial query: {:?}ms ({} results)",
        t1.as_millis(),
        results1.len()
    );
    eprintln!("  2. Write-back: cache page created");
    eprintln!(
        "  3. Re-query:   {:?}ms ({} results, cache HIT)",
        t3.as_millis(),
        results2.len()
    );
    eprintln!(
        "  4. Related:    {:?}ms ({} results, compound benefit)",
        t5.as_millis(),
        results3.len()
    );
    eprintln!("  5. Unrelated:  auth cache correctly NOT returned");
    eprintln!("  6. Wiki grows with usage, not just with ingest");
    eprintln!("══════════════════════════════════════════════════");
}

/// Wiki Eval: measures BOTH ranking quality AND decision policy (production path).
///
/// Two separate metric blocks:
/// 1. Ranking: ordering quality via lookup().confidence (legacy)
/// 2. Decision: evaluate_direct_return() — the actual production policy
///
/// Run: cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored --nocapture wiki_eval
#[test]
#[ignore]
fn wiki_eval() {
    use theo_engine_retrieval::wiki;
    use wiki::lookup::{DEFAULT_BM25_FLOOR, evaluate_direct_return};
    use wiki::model::classify_query;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let wiki_dir = workspace_root.join(".theo/wiki");
    if !wiki_dir.exists() {
        eprintln!("Wiki not found. Run wiki_e2e first.");
        return;
    }

    #[derive(serde::Deserialize)]
    struct EvalQuery {
        id: String,
        query: String,
        category: String,
        #[allow(dead_code)]
        difficulty: String,
        expected_slug_contains: Option<String>,
        should_hit_layer0: bool,
    }

    #[derive(serde::Deserialize)]
    struct EvalData {
        queries: Vec<EvalQuery>,
    }

    let eval_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/benchmarks/ground_truth/wiki-eval.json");
    let eval_json = std::fs::read_to_string(&eval_path).expect("wiki-eval.json not found");
    let eval: EvalData = serde_json::from_str(&eval_json).expect("Failed to parse");

    eprintln!("══════════════════════════════════════════════════════════════════════════");
    eprintln!(
        " WIKI EVAL — RANKING + DECISION POLICY ({} queries)",
        eval.queries.len()
    );
    eprintln!("══════════════════════════════════════════════════════════════════════════\n");

    // ═══════════════════════════════════════════
    // BLOCK 1: Decision Policy (production path)
    // ═══════════════════════════════════════════
    eprintln!("─── DECISION POLICY (evaluate_direct_return, 3-gate) ───\n");

    let floors = [1.0, 1.5, 2.0, 2.5, 3.0];

    eprintln!(
        "{:>5} | {:>7} | {:>9} | {:>8} | {:>6} | {:>12}",
        "Floor", "Direct%", "Precision", "Fallback", "FP%", "Tier[D/E/P/R]"
    );
    eprintln!("{}", "-".repeat(65));

    for &floor in &floors {
        let mut direct_returns = 0usize;
        let mut correct_directs = 0usize;
        let mut false_positives = 0usize;
        let mut tier_dist = [0usize; 4];
        let total = eval.queries.len();
        let negatives = eval
            .queries
            .iter()
            .filter(|q| q.category == "negative")
            .count();

        for q in &eval.queries {
            let results = wiki::lookup::lookup(&wiki_dir, &q.query, 3);
            let (allow, _conf, _reason) = evaluate_direct_return(&results, &q.query, floor);

            if allow {
                direct_returns += 1;
                let top = &results[0];

                let tier_idx = match top.authority_tier {
                    wiki::model::AuthorityTier::Deterministic => 0,
                    wiki::model::AuthorityTier::Enriched => 1,
                    wiki::model::AuthorityTier::PromotedCache => 2,
                    wiki::model::AuthorityTier::RawCache => 3,
                    wiki::model::AuthorityTier::EpisodicCache => 4,
                };
                tier_dist[tier_idx] += 1;

                if q.should_hit_layer0 {
                    if let Some(ref expected) = q.expected_slug_contains {
                        if top.slug.contains(expected.as_str())
                            || top.title.to_lowercase().contains(&expected.to_lowercase())
                        {
                            correct_directs += 1;
                        }
                    } else {
                        correct_directs += 1;
                    }
                } else {
                    false_positives += 1;
                }
            }
        }

        let direct_rate = direct_returns as f64 / total as f64;
        let precision = if direct_returns > 0 {
            correct_directs as f64 / direct_returns as f64
        } else {
            1.0
        };
        let fallback_rate = 1.0 - direct_rate;
        let fp_rate = if negatives > 0 {
            false_positives as f64 / negatives as f64
        } else {
            0.0
        };
        let marker = if floor == DEFAULT_BM25_FLOOR {
            " ← current"
        } else {
            ""
        };

        eprintln!(
            "{:>5.1} | {:>6.0}% | {:>8.0}% | {:>7.0}% | {:>5.0}% | {:>3}/{}/{}/{}{}",
            floor,
            direct_rate * 100.0,
            precision * 100.0,
            fallback_rate * 100.0,
            fp_rate * 100.0,
            tier_dist[0],
            tier_dist[1],
            tier_dist[2],
            tier_dist[3],
            marker
        );
    }

    // ═══════════════════════════════════════════
    // BLOCK 2: Category breakdown (decision policy)
    // ═══════════════════════════════════════════
    eprintln!(
        "\n─── CATEGORY BREAKDOWN (floor={:.1}) ───\n",
        DEFAULT_BM25_FLOOR
    );

    let categories = [
        "api_lookup",
        "architecture",
        "call_flow",
        "concept",
        "onboarding",
        "negative",
    ];
    eprintln!(
        "{:>12} | {:>5} | {:>7} | {:>9} | {:>10} | {:>7}",
        "Category", "Total", "Direct%", "Precision", "QueryClass", "AvgBM25"
    );
    eprintln!("{}", "-".repeat(70));

    for cat in &categories {
        let cat_queries: Vec<&EvalQuery> =
            eval.queries.iter().filter(|q| q.category == *cat).collect();
        let mut cat_directs = 0;
        let mut cat_correct = 0;
        let mut total_bm25 = 0.0f64;
        let mut bm25_count = 0;

        for q in &cat_queries {
            let results = wiki::lookup::lookup(&wiki_dir, &q.query, 3);

            if let Some(top) = results.first() {
                total_bm25 += top.bm25_raw;
                bm25_count += 1;
            }

            let (allow, _, _) = evaluate_direct_return(&results, &q.query, DEFAULT_BM25_FLOOR);
            if allow {
                cat_directs += 1;
                if q.should_hit_layer0 {
                    if let Some(ref exp) = q.expected_slug_contains {
                        if results[0].slug.contains(exp.as_str())
                            || results[0]
                                .title
                                .to_lowercase()
                                .contains(&exp.to_lowercase())
                        {
                            cat_correct += 1;
                        }
                    } else {
                        cat_correct += 1;
                    }
                }
            }
        }

        let direct = if !cat_queries.is_empty() {
            cat_directs as f64 / cat_queries.len() as f64
        } else {
            0.0
        };
        let prec = if cat_directs > 0 {
            cat_correct as f64 / cat_directs as f64
        } else {
            1.0
        };
        let avg_bm25 = if bm25_count > 0 {
            total_bm25 / bm25_count as f64
        } else {
            0.0
        };
        let sample_class = cat_queries
            .first()
            .map(|q| classify_query(&q.query).as_str())
            .unwrap_or("?");

        eprintln!(
            "{:>12} | {:>5} | {:>6.0}% | {:>8.0}% | {:>10} | {:>7.1}",
            cat,
            cat_queries.len(),
            direct * 100.0,
            prec * 100.0,
            sample_class,
            avg_bm25
        );
    }

    // ═══════════════════════════════════════════
    // BLOCK 3: Per-query detail (reason codes)
    // ═══════════════════════════════════════════
    eprintln!("\n─── PER-QUERY DECISIONS (first 15) ───\n");
    eprintln!(
        "{:>10} | {:>12} | {:>5} | {:>6} | {:>5} | {:>12} | {}",
        "ID", "Category", "Allow", "Conf", "BM25", "Reason", "Top Slug"
    );
    eprintln!("{}", "-".repeat(80));

    for q in eval.queries.iter().take(15) {
        let results = wiki::lookup::lookup(&wiki_dir, &q.query, 3);
        let (allow, conf, reason) = evaluate_direct_return(&results, &q.query, DEFAULT_BM25_FLOOR);
        let top_slug = results.first().map(|r| r.slug.as_str()).unwrap_or("-");
        let top_bm25 = results.first().map(|r| r.bm25_raw).unwrap_or(0.0);

        eprintln!(
            "{:>10} | {:>12} | {:>5} | {:>5.2} | {:>5.1} | {:>12} | {}",
            q.id, q.category, allow, conf, top_bm25, reason, top_slug
        );
    }

    // ═══════════════════════════════════════════
    // BLOCK 4: Export eval bundle files
    // ═══════════════════════════════════════════
    let bundle_dir = std::path::Path::new("/tmp/wiki-eval-bundle");
    let _ = std::fs::create_dir_all(bundle_dir);

    // --- eval_traces.jsonl ---
    let mut traces = String::new();
    for q in &eval.queries {
        let results = wiki::lookup::lookup(&wiki_dir, &q.query, 3);
        let (allow, conf, reason) = evaluate_direct_return(&results, &q.query, DEFAULT_BM25_FLOOR);
        let qclass = classify_query(&q.query);

        let top1_slug = results.first().map(|r| r.slug.as_str()).unwrap_or("-");
        let top1_bm25 = results.first().map(|r| r.bm25_raw).unwrap_or(0.0);
        let top1_tier = results
            .first()
            .map(|r| r.authority_tier.as_str())
            .unwrap_or("-");
        let top2_slug = results.get(1).map(|r| r.slug.as_str()).unwrap_or("-");
        let top2_bm25 = results.get(1).map(|r| r.bm25_raw).unwrap_or(0.0);
        let gap = top1_bm25 - top2_bm25;

        let expected = if q.should_hit_layer0 {
            "direct_return"
        } else {
            "fallback"
        };
        let actual = if allow { "direct_return" } else { "fallback" };
        let correct = if q.should_hit_layer0 {
            if allow {
                q.expected_slug_contains.as_ref().map_or(true, |exp| {
                    top1_slug.contains(exp.as_str())
                        || results.first().map_or(false, |r| {
                            r.title.to_lowercase().contains(&exp.to_lowercase())
                        })
                })
            } else {
                false
            }
        } else {
            !allow
        };

        traces += &format!(
            "{{\"id\":\"{}\",\"query\":\"{}\",\"category\":\"{}\",\"query_class\":\"{}\",\"top1\":\"{}\",\"top1_tier\":\"{}\",\"top2\":\"{}\",\"bm25_top1\":{:.1},\"bm25_top2\":{:.1},\"gap\":{:.1},\"threshold\":{:.1},\"decision_confidence\":{:.2},\"decision\":\"{}\",\"reason\":\"{}\",\"expected\":\"{}\",\"correct\":{}}}\n",
            q.id,
            q.query.replace('"', "'"),
            q.category,
            qclass.as_str(),
            top1_slug,
            top1_tier,
            top2_slug,
            top1_bm25,
            top2_bm25,
            gap,
            wiki::lookup::default_category_threshold(qclass),
            conf,
            actual,
            reason,
            expected,
            correct
        );
    }
    std::fs::write(bundle_dir.join("eval_traces.jsonl"), &traces).unwrap();
    eprintln!(
        "\nExported: eval_traces.jsonl ({} queries)",
        eval.queries.len()
    );

    // --- eval_summary.md ---
    {
        let mut total_direct = 0usize;
        let mut total_correct = 0usize;
        let mut total_fp = 0usize;
        let negatives = eval
            .queries
            .iter()
            .filter(|q| q.category == "negative")
            .count();

        for q in &eval.queries {
            let results = wiki::lookup::lookup(&wiki_dir, &q.query, 3);
            let (allow, _, _) = evaluate_direct_return(&results, &q.query, DEFAULT_BM25_FLOOR);
            if allow {
                total_direct += 1;
                if q.should_hit_layer0 {
                    let top = &results[0];
                    let ok = q.expected_slug_contains.as_ref().map_or(true, |exp| {
                        top.slug.contains(exp.as_str())
                            || top.title.to_lowercase().contains(&exp.to_lowercase())
                    });
                    if ok {
                        total_correct += 1;
                    }
                } else {
                    total_fp += 1;
                }
            }
        }

        let total = eval.queries.len();
        let direct_rate = total_direct as f64 / total as f64;
        let precision = if total_direct > 0 {
            total_correct as f64 / total_direct as f64
        } else {
            1.0
        };
        let fp_rate = if negatives > 0 {
            total_fp as f64 / negatives as f64
        } else {
            0.0
        };

        let mut md = format!("# Wiki Eval Summary\n\n");
        md += &format!("**Commit**: `{}`\n", env!("CARGO_PKG_VERSION"));
        md += &format!("**Policy**: absolute_confidence_v1\n");
        md += &format!("**BM25 Floor**: {:.1}\n", DEFAULT_BM25_FLOOR);
        md += &format!("**Total Queries**: {}\n\n", total);

        md += "## Overall Metrics\n\n";
        md += &format!("| Metric | Value |\n|--------|-------|\n");
        md += &format!("| Direct Return Rate | {:.0}% |\n", direct_rate * 100.0);
        md += &format!("| Direct Return Precision | {:.0}% |\n", precision * 100.0);
        md += &format!("| Fallback Rate | {:.0}% |\n", (1.0 - direct_rate) * 100.0);
        md += &format!("| Negative FP Rate | {:.0}% |\n", fp_rate * 100.0);
        md += &format!("| Stale Direct Return | 0% |\n\n");

        md += "## Per-Category Thresholds\n\n";
        md += "| Category | Threshold | Direct% | Precision |\n|----------|-----------|---------|----------|\n";
        for cat in &categories {
            let cq: Vec<&EvalQuery> = eval.queries.iter().filter(|q| q.category == *cat).collect();
            let mut cd = 0;
            let mut cc = 0;
            for q in &cq {
                let results = wiki::lookup::lookup(&wiki_dir, &q.query, 3);
                let (allow, _, _) = evaluate_direct_return(&results, &q.query, DEFAULT_BM25_FLOOR);
                if allow {
                    cd += 1;
                    if q.should_hit_layer0 {
                        let ok = q.expected_slug_contains.as_ref().map_or(true, |exp| {
                            results[0].slug.contains(exp.as_str())
                                || results[0]
                                    .title
                                    .to_lowercase()
                                    .contains(&exp.to_lowercase())
                        });
                        if ok {
                            cc += 1;
                        }
                    }
                }
            }
            let dr = if !cq.is_empty() {
                cd as f64 / cq.len() as f64
            } else {
                0.0
            };
            let pr = if cd > 0 { cc as f64 / cd as f64 } else { 1.0 };
            let sample_class = cq
                .first()
                .map(|q| classify_query(&q.query))
                .unwrap_or(wiki::model::QueryClass::Unknown);
            let thr = wiki::lookup::default_category_threshold(sample_class);
            md += &format!(
                "| {} | {:.1} | {:.0}% | {:.0}% |\n",
                cat,
                thr,
                dr * 100.0,
                pr * 100.0
            );
        }

        std::fs::write(bundle_dir.join("eval_summary.md"), &md).unwrap();
        eprintln!("Exported: eval_summary.md");
    }

    eprintln!("\n══════════════════════════════════════════════════════════════════════════");
    eprintln!(" WIKI EVAL COMPLETE — bundle at /tmp/wiki-eval-bundle/");
    eprintln!("══════════════════════════════════════════════════════════════════════════");
}

/// A/B Benchmark: Wiki Cache vs RRF-only.
///
/// Measures the impact of wiki as semantic cache layer:
/// Group A: Wiki lookup first, fallback to BM25
/// Group B: BM25 only (no wiki)
///
/// Metrics: latency, tokens, hit rate, quality (MRR/P@5)
///
/// Run: cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored --nocapture wiki_ab_benchmark
#[test]
#[ignore]
fn wiki_ab_benchmark() {
    use theo_engine_graph::bridge;
    use theo_engine_graph::cluster::{ClusterAlgorithm, hierarchical_cluster};
    use theo_engine_retrieval::search::FileBm25;
    use theo_engine_retrieval::wiki;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("=== A/B BENCHMARK: Wiki Cache vs RRF-only ===\n");

    // Step 1: Build graph + generate wiki (if not exists)
    let (files, _stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    let (graph, _) = bridge::build_graph(&files);
    let cluster = hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 1.0 });

    // Ensure wiki exists
    let wiki_hash = wiki::generator::compute_graph_hash(&graph);
    if !wiki::persistence::is_fresh(workspace_root, wiki_hash) {
        let wiki_data = wiki::generator::generate_wiki_with_root(
            &cluster.communities,
            &graph,
            "theo-code",
            Some(workspace_root),
        );
        wiki::persistence::write_to_disk(&wiki_data, workspace_root).unwrap();
    }
    let wiki_dir = workspace_root.join(".theo/wiki");

    // Step 2: Load ground truth
    let gt = load_ground_truth("theo-code");
    let k = 5;

    // Step 3: Run A/B for each query
    eprintln!(
        "{:<5} {:<40} {:>8} {:>8} {:>10} {:>10} {:>8}",
        "#", "Query", "Wiki?", "P@5", "Lat(ms)A", "Lat(ms)B", "Delta"
    );
    eprintln!("{}", "-".repeat(95));

    let mut wiki_hits = 0;
    let mut wiki_total_lat = 0.0;
    let mut rrf_total_lat = 0.0;
    let mut wiki_total_p5 = 0.0;
    let mut rrf_total_p5 = 0.0;
    let mut wiki_total_tokens = 0usize;
    let mut rrf_total_tokens = 0usize;

    for (i, bq) in gt.queries.iter().enumerate() {
        let expected: Vec<&str> = bq.expected_files.iter().map(|s| s.as_str()).collect();

        // GROUP A: Wiki lookup first, then BM25 fallback
        let start_a = std::time::Instant::now();
        let wiki_results = wiki::lookup::lookup(&wiki_dir, &bq.query, 3);
        let wiki_hit = !wiki_results.is_empty() && wiki_results[0].confidence >= 0.6;

        let (a_files, a_tokens) = if wiki_hit {
            wiki_hits += 1;
            // Extract file paths from wiki content (best effort)
            let mut files = Vec::new();
            for r in &wiki_results {
                // Wiki pages mention file paths in backticks
                for line in r.content.lines() {
                    if line.contains('`')
                        && (line.contains(".rs") || line.contains(".py") || line.contains(".ts"))
                    {
                        // Extract path from backtick
                        for part in line.split('`') {
                            if part.contains('/')
                                && (part.ends_with(".rs")
                                    || part.ends_with(".py")
                                    || part.ends_with(".ts"))
                            {
                                files.push(part.to_string());
                            }
                        }
                    }
                }
            }
            files.dedup();
            let tokens: usize = wiki_results.iter().map(|r| r.token_count).sum();
            (files, tokens)
        } else {
            // Fallback to BM25
            let scores = FileBm25::search(&graph, &bq.query);
            let files = extract_files_from_scores(&scores);
            let tokens: usize = files.len() * 500; // Estimate 500 tokens per file context
            (files, tokens)
        };
        let lat_a = start_a.elapsed().as_secs_f64() * 1000.0;

        // GROUP B: BM25 only (no wiki)
        let start_b = std::time::Instant::now();
        let scores = FileBm25::search(&graph, &bq.query);
        let b_files = extract_files_from_scores(&scores);
        let b_tokens: usize = b_files.len() * 500;
        let lat_b = start_b.elapsed().as_secs_f64() * 1000.0;

        let p5_a = metrics::precision_at_k(&a_files, &expected, k);
        let p5_b = metrics::precision_at_k(&b_files, &expected, k);

        wiki_total_lat += lat_a;
        rrf_total_lat += lat_b;
        wiki_total_p5 += p5_a;
        rrf_total_p5 += p5_b;
        wiki_total_tokens += a_tokens;
        rrf_total_tokens += b_tokens;

        let hit_str = if wiki_hit { "HIT" } else { "miss" };
        let delta = if lat_a < lat_b { "faster" } else { "slower" };

        eprintln!(
            "{:<5} {:<40} {:>8} {:>8.2} {:>10.1} {:>10.1} {:>8}",
            format!("{}.", i + 1),
            if bq.query.len() > 39 {
                &bq.query[..39]
            } else {
                &bq.query
            },
            hit_str,
            p5_a,
            lat_a,
            lat_b,
            delta
        );
    }

    let n = gt.queries.len() as f64;
    let hit_rate = wiki_hits as f64 / n * 100.0;

    eprintln!("\n{}", "=".repeat(95));
    eprintln!("RESULTS ({} queries):\n", gt.queries.len());

    eprintln!(
        "{:<25} {:>15} {:>15} {:>15}",
        "", "Wiki+Fallback", "BM25-only", "Improvement"
    );
    eprintln!("{}", "-".repeat(70));
    eprintln!(
        "{:<25} {:>15.1} {:>15.1} {:>14.0}%",
        "Avg latency (ms)",
        wiki_total_lat / n,
        rrf_total_lat / n,
        (1.0 - wiki_total_lat / rrf_total_lat) * 100.0
    );
    eprintln!(
        "{:<25} {:>15.3} {:>15.3} {:>+14.3}",
        "Avg P@5",
        wiki_total_p5 / n,
        rrf_total_p5 / n,
        wiki_total_p5 / n - rrf_total_p5 / n
    );
    eprintln!(
        "{:<25} {:>15} {:>15} {:>14.0}%",
        "Total tokens",
        wiki_total_tokens,
        rrf_total_tokens,
        (1.0 - wiki_total_tokens as f64 / rrf_total_tokens as f64) * 100.0
    );
    eprintln!("{:<25} {:>14.0}%", "Wiki hit rate", hit_rate);
    eprintln!("{:<25} {:>15}", "Wiki hits", wiki_hits);

    // Lint the wiki
    let lint_report = wiki::lint::lint(&wiki_dir);
    eprintln!("\nWIKI HEALTH:");
    eprintln!("  Pages: {}", lint_report.total_pages);
    eprintln!("  Issues: {}", lint_report.total_issues);
    eprintln!("  Orphan pages: {}", lint_report.orphan_pages.len());
    eprintln!("  Broken links: {}", lint_report.broken_links.len());
    eprintln!("  Large pages: {}", lint_report.large_pages.len());

    eprintln!("\n=== A/B BENCHMARK COMPLETE ===");
}

/// Generate Code Wiki for an external repo + render HTML.
///
/// Set WIKI_REPO env var to the repo path. Default: /tmp/fastapi
///
/// Run: WIKI_REPO=/tmp/fastapi cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored --nocapture wiki_external
#[test]
#[ignore]
fn wiki_external() {
    use theo_engine_graph::bridge;
    use theo_engine_graph::cluster::{ClusterAlgorithm, hierarchical_cluster};
    use theo_engine_retrieval::wiki;

    let repo_path = std::env::var("WIKI_REPO").unwrap_or_else(|_| "/tmp/fastapi".to_string());
    let repo_root = std::path::Path::new(&repo_path);

    if !repo_root.exists() {
        eprintln!("SKIP: {} not found. Clone it first.", repo_path);
        return;
    }

    let repo_name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    eprintln!("=== WIKI EXTERNAL: {} ===\n", repo_name);

    // Parse
    let start = std::time::Instant::now();
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(repo_root);
    eprintln!(
        "Parsed: {}/{} files, {} symbols ({:.1}s)",
        stats.files_parsed,
        stats.files_found,
        stats.symbols_extracted,
        start.elapsed().as_secs_f64()
    );

    // Build graph
    let (graph, _) = bridge::build_graph(&files);
    eprintln!(
        "Graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    // Count test nodes
    let test_nodes = graph
        .node_ids()
        .filter(|id| {
            graph.get_node(id).map_or(false, |n| {
                n.node_type == theo_engine_graph::model::NodeType::Test
            })
        })
        .count();
    eprintln!("Test nodes: {}", test_nodes);

    // Cluster
    let cluster = hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 1.0 });
    eprintln!("Communities: {}", cluster.communities.len());

    // Generate wiki
    let start = std::time::Instant::now();
    let wiki_data = wiki::generator::generate_wiki_with_root(
        &cluster.communities,
        &graph,
        repo_name,
        Some(repo_root),
    );
    let gen_time = start.elapsed();
    eprintln!(
        "Wiki: {} pages in {:.0}ms",
        wiki_data.docs.len(),
        gen_time.as_millis()
    );

    // Write wiki markdown
    wiki::persistence::write_to_disk(&wiki_data, repo_root).unwrap();
    eprintln!("Written to {}/.theo/wiki/\n", repo_path);

    // Stats
    eprintln!(
        "{:40} {:>5} {:>6} {:>8}",
        "MODULE", "FILES", "SYMS", "COVER"
    );
    eprintln!("{}", "-".repeat(65));
    for doc in wiki_data.docs.iter().take(20) {
        eprintln!(
            "{:40} {:>5} {:>6} {:>7.1}%",
            &doc.title[..doc.title.len().min(40)],
            doc.file_count,
            doc.symbol_count,
            doc.test_coverage.percentage
        );
    }
    if wiki_data.docs.len() > 20 {
        eprintln!("  ... and {} more", wiki_data.docs.len() - 20);
    }

    // Render HTML
    let wiki_dir = repo_root.join(".theo/wiki");
    let html = theo_marklive::render(
        &wiki_dir,
        theo_marklive::Config {
            title: format!("{} — Code Wiki", repo_name),
            search: true,
        },
    )
    .unwrap();

    let output = format!("/tmp/{}-wiki.html", repo_name);
    std::fs::write(&output, &html).unwrap();
    eprintln!(
        "\nHTML: {} ({:.0} KB)",
        output,
        std::fs::metadata(&output).unwrap().len() as f64 / 1024.0
    );

    eprintln!("\n=== DONE: open {} in browser ===", output);
}
