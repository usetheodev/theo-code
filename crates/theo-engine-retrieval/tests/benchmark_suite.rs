//! GRAPHCTX Professional Benchmark Suite
//!
//! Industry-standard evaluation aligned with RepoBench, CodeRAG-Bench, CodeSearchNet.
//! Measures: Recall@5, Recall@10, MRR, Hit@5, Hit@10, nDCG@5, nDCG@10, MAP,
//!           Dependency Coverage, Missing Dep Rate.
//!
//! Ground truth loaded from JSON (tests/benchmarks/ground_truth/*.json).
//!
//! Run: cargo test -p theo-engine-retrieval --test benchmark_suite -- --ignored --nocapture

use std::collections::HashMap;
use serde::Deserialize;
use theo_engine_retrieval::metrics::{self, RetrievalMetrics, DepEdge};

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

fn emit_report(
    repo: &RepoInfo,
    results: &[QueryResult],
    pipeline_name: &str,
) {
    let all_metrics: Vec<RetrievalMetrics> = results.iter().map(|r| r.metrics.clone()).collect();
    let overall = RetrievalMetrics::average(&all_metrics);

    // Per-category aggregation
    let mut by_category: HashMap<&str, Vec<RetrievalMetrics>> = HashMap::new();
    for r in results {
        by_category.entry(r.category.as_str()).or_default().push(r.metrics.clone());
    }

    // Per-difficulty aggregation
    let mut by_difficulty: HashMap<&str, Vec<RetrievalMetrics>> = HashMap::new();
    for r in results {
        by_difficulty.entry(r.difficulty.as_str()).or_default().push(r.metrics.clone());
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
    eprintln!("  Recall@5  = {:.3}    Recall@10 = {:.3}", overall.recall_at_5, overall.recall_at_10);
    eprintln!("  P@5       = {:.3}    MRR       = {:.3}", overall.precision_at_5, overall.mrr);
    eprintln!("  Hit@5     = {:.3}    Hit@10    = {:.3}", overall.hit_rate_at_5, overall.hit_rate_at_10);
    eprintln!("  nDCG@5    = {:.3}    nDCG@10   = {:.3}", overall.ndcg_at_5, overall.ndcg_at_10);
    eprintln!("  MAP       = {:.3}", overall.average_precision);
    eprintln!("  DepCov    = {:.3}    MissDep   = {:.3}", overall.dep_coverage, overall.missing_dep_rate);
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
        let pass = if *higher_better { *actual >= *target } else { *actual <= *target };
        eprintln!("  {:<12} {:.3} / {:.3}  {}", name, actual, target, if pass { "PASS" } else { "FAIL" });
    }
    eprintln!();

    // By category
    eprintln!("BY CATEGORY:");
    for (cat, cat_metrics) in &by_category {
        let avg = RetrievalMetrics::average(cat_metrics);
        eprintln!("  {:<15} R@5={:.3}  R@10={:.3}  MRR={:.3}  nDCG@5={:.3}  DepCov={:.3}",
            cat, avg.recall_at_5, avg.recall_at_10, avg.mrr, avg.ndcg_at_5, avg.dep_coverage);
    }
    eprintln!();

    // By difficulty
    eprintln!("BY DIFFICULTY:");
    for (diff, diff_metrics) in &by_difficulty {
        let avg = RetrievalMetrics::average(diff_metrics);
        eprintln!("  {:<10} R@5={:.3}  MRR={:.3}  nDCG@5={:.3}  ({} queries)",
            diff, avg.recall_at_5, avg.mrr, avg.ndcg_at_5, diff_metrics.len());
    }
    eprintln!();

    // Per-query detail (failures)
    eprintln!("FAILURES (P@5 < 0.40):");
    for r in results {
        if r.metrics.precision_at_5 < 0.40 {
            eprintln!("  {} '{}' P@5={:.2} R@5={:.2} MRR={:.2} DepCov={:.2}",
                r.id, r.query, r.metrics.precision_at_5, r.metrics.recall_at_5,
                r.metrics.mrr, r.metrics.dep_coverage);
            eprintln!("    Expected: {:?}", r.expected_files.iter().map(|f| f.split('/').last().unwrap_or(f)).collect::<Vec<_>>());
            eprintln!("    Got top5: {:?}", r.returned_top_10.iter().take(5).map(|f| f.split('/').last().unwrap_or(f)).collect::<Vec<_>>());
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
        .parent().unwrap()
        .parent().unwrap();

    eprintln!("Building graph from {}...", workspace_root.display());
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!("Parsed {}/{} files, {} symbols", stats.files_parsed, stats.files_found, stats.symbols_extracted);

    let (graph, _) = bridge::build_graph(&files);
    eprintln!("Graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());

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
    use theo_engine_retrieval::tantivy_search::{FileTantivyIndex, hybrid_rrf_search};
    use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
    use theo_engine_retrieval::embedding::cache::EmbeddingCache;

    let gt = load_ground_truth("theo-code");

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();

    eprintln!("Building graph...");
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!("Parsed {}/{} files", stats.files_parsed, stats.files_found);

    let (graph, _) = bridge::build_graph(&files);
    eprintln!("Graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());

    let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy build failed");
    let embedder = NeuralEmbedder::new().expect("NeuralEmbedder init failed");
    let cache = EmbeddingCache::build(&graph, &embedder);
    eprintln!("Tantivy: {} docs, Embeddings: {} files", tantivy_index.num_docs(), cache.len());

    let mut results: Vec<QueryResult> = Vec::new();

    for bq in &gt.queries {
        let rrf_scores = hybrid_rrf_search(&graph, &tantivy_index, &embedder, &cache, &bq.query, 20.0);
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

    emit_report(&gt.repo, &results, "RRF 3-ranker (BM25+Tantivy+Dense, Jina Code)");
}

/// A/B benchmark: Symbol-First vs RRF baseline.
///
/// Run: cargo test -p theo-engine-retrieval --features dense-retrieval --test benchmark_suite -- --ignored --nocapture benchmark_symbol_first_ab
#[test]
#[ignore]
#[cfg(feature = "dense-retrieval")]
fn benchmark_symbol_first_ab() {
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::tantivy_search::{FileTantivyIndex, hybrid_rrf_search, symbol_first_search};
    use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
    use theo_engine_retrieval::embedding::cache::EmbeddingCache;

    let gt = load_ground_truth("theo-code");

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();

    eprintln!("Building graph...");
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!("Parsed {}/{} files", stats.files_parsed, stats.files_found);

    let (graph, _) = bridge::build_graph(&files);
    let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy build failed");
    let embedder = NeuralEmbedder::new().expect("NeuralEmbedder init failed");
    let cache = EmbeddingCache::build(&graph, &embedder);
    eprintln!("Ready: {} nodes, Tantivy {} docs, Embeddings {} files",
        graph.node_count(), tantivy_index.num_docs(), cache.len());

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
            id: bq.id.clone(), query: bq.query.clone(),
            category: bq.category.clone(), difficulty: bq.difficulty.clone(),
            metrics: m, returned_top_10: returned.into_iter().take(10).collect(),
            expected_files: bq.expected_files.clone(),
        });
    }

    // --- Variant: Symbol-First ---
    let mut sym_results: Vec<QueryResult> = Vec::new();
    for bq in &gt.queries {
        let scores = symbol_first_search(&graph, &tantivy_index, &embedder, &cache, &bq.query, 20.0);
        let returned = extract_files_from_scores(&scores);
        let expected: Vec<&str> = bq.expected_files.iter().map(|s| s.as_str()).collect();
        let deps: Vec<DepEdge> = bq.dependencies.iter().map(|d| d.to_dep_edge()).collect();
        let m = RetrievalMetrics::compute(&returned, &expected, &deps);
        sym_results.push(QueryResult {
            id: bq.id.clone(), query: bq.query.clone(),
            category: bq.category.clone(), difficulty: bq.difficulty.clone(),
            metrics: m, returned_top_10: returned.into_iter().take(10).collect(),
            expected_files: bq.expected_files.clone(),
        });
    }

    // --- A/B Comparison ---
    let rrf_avg = RetrievalMetrics::average(&rrf_results.iter().map(|r| r.metrics.clone()).collect::<Vec<_>>());
    let sym_avg = RetrievalMetrics::average(&sym_results.iter().map(|r| r.metrics.clone()).collect::<Vec<_>>());

    eprintln!("\n{}", "=".repeat(90));
    eprintln!("A/B: SYMBOL-FIRST vs RRF BASELINE");
    eprintln!("{}", "=".repeat(90));
    eprintln!("{:<20} {:>10} {:>10} {:>10}", "", "RRF", "Symbol-1st", "Delta");
    eprintln!("{}", "-".repeat(60));
    eprintln!("{:<20} {:>10.3} {:>10.3} {:>+10.3}", "Recall@5", rrf_avg.recall_at_5, sym_avg.recall_at_5, sym_avg.recall_at_5 - rrf_avg.recall_at_5);
    eprintln!("{:<20} {:>10.3} {:>10.3} {:>+10.3}", "Recall@10", rrf_avg.recall_at_10, sym_avg.recall_at_10, sym_avg.recall_at_10 - rrf_avg.recall_at_10);
    eprintln!("{:<20} {:>10.3} {:>10.3} {:>+10.3}", "MRR", rrf_avg.mrr, sym_avg.mrr, sym_avg.mrr - rrf_avg.mrr);
    eprintln!("{:<20} {:>10.3} {:>10.3} {:>+10.3}", "Hit@5", rrf_avg.hit_rate_at_5, sym_avg.hit_rate_at_5, sym_avg.hit_rate_at_5 - rrf_avg.hit_rate_at_5);
    eprintln!("{:<20} {:>10.3} {:>10.3} {:>+10.3}", "nDCG@5", rrf_avg.ndcg_at_5, sym_avg.ndcg_at_5, sym_avg.ndcg_at_5 - rrf_avg.ndcg_at_5);
    eprintln!("{:<20} {:>10.3} {:>10.3} {:>+10.3}", "MAP", rrf_avg.average_precision, sym_avg.average_precision, sym_avg.average_precision - rrf_avg.average_precision);
    eprintln!("{:<20} {:>10.3} {:>10.3} {:>+10.3}", "DepCov", rrf_avg.dep_coverage, sym_avg.dep_coverage, sym_avg.dep_coverage - rrf_avg.dep_coverage);

    // Per-query delta for queries that improved
    eprintln!("\nPER-QUERY DELTA (R@5 changed):");
    for (rrf_r, sym_r) in rrf_results.iter().zip(sym_results.iter()) {
        let delta = sym_r.metrics.recall_at_5 - rrf_r.metrics.recall_at_5;
        if delta.abs() > 0.01 {
            eprintln!("  {} '{}': R@5 {:.2} → {:.2} ({:+.2})",
                rrf_r.id, rrf_r.query, rrf_r.metrics.recall_at_5, sym_r.metrics.recall_at_5, delta);
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
    use theo_engine_retrieval::tantivy_search::{FileTantivyIndex, hybrid_rrf_search};
    use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
    use theo_engine_retrieval::embedding::cache::EmbeddingCache;

    let gt_dir = format!("{}/tests/benchmarks/ground_truth", env!("CARGO_MANIFEST_DIR"));
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
        eprintln!("\n--- Benchmarking {} ({}) with RRF+Dense ---", gt.repo.name, gt.repo.language);

        let repo_root = std::path::Path::new(&repo_path);
        let (files, stats) = theo_application::use_cases::extraction::extract_repo(repo_root);
        eprintln!("Parsed {}/{} files, {} symbols", stats.files_parsed, stats.files_found, stats.symbols_extracted);

        let (graph, _) = bridge::build_graph(&files);
        eprintln!("Graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());

        let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy build failed");
        let cache = EmbeddingCache::build(&graph, &embedder);
        eprintln!("Tantivy: {} docs, Embeddings: {} files", tantivy_index.num_docs(), cache.len());

        let mut results: Vec<QueryResult> = Vec::new();

        for bq in &gt.queries {
            let rrf_scores = hybrid_rrf_search(&graph, &tantivy_index, &embedder, &cache, &bq.query, 20.0);
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

        emit_report(&gt.repo, &results, "RRF 3-ranker (BM25+Tantivy+Dense, Jina Code)");
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
    let gt_dir = format!("{}/tests/benchmarks/ground_truth", env!("CARGO_MANIFEST_DIR"));
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
        eprintln!("\n--- Benchmarking {} ({}) ---", gt.repo.name, gt.repo.language);

        let repo_root = std::path::Path::new(&repo_path);
        let (files, stats) = theo_application::use_cases::extraction::extract_repo(repo_root);
        eprintln!("Parsed {}/{} files, {} symbols", stats.files_parsed, stats.files_found, stats.symbols_extracted);

        let (graph, _) = bridge::build_graph(&files);
        eprintln!("Graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());

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
    use theo_engine_graph::cluster::{hierarchical_cluster, ClusterAlgorithm};
    use theo_engine_retrieval::wiki;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();

    eprintln!("=== CODE WIKI E2E TEST ===\n");

    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!("Parsed: {}/{} files, {} symbols", stats.files_parsed, stats.files_found, stats.symbols_extracted);

    let (graph, _) = bridge::build_graph(&files);
    eprintln!("Graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());

    let cluster = hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 1.0 });
    eprintln!("Communities: {}", cluster.communities.len());

    // Debug: count node types
    let test_nodes = graph.node_ids().filter(|id| {
        graph.get_node(id).map_or(false, |n| n.node_type == theo_engine_graph::model::NodeType::Test)
    }).count();
    let test_edges = graph.all_edges().iter().filter(|e| e.edge_type == theo_engine_graph::model::EdgeType::Tests).count();
    eprintln!("DEBUG: {} Test nodes, {} Tests edges in graph", test_nodes, test_edges);

    let start = std::time::Instant::now();
    let wiki_data = wiki::generator::generate_wiki_with_root(&cluster.communities, &graph, "theo-code", Some(workspace_root));
    let gen_time = start.elapsed();
    eprintln!("Wiki: {} pages in {:.0}ms\n", wiki_data.docs.len(), gen_time.as_millis());

    wiki::persistence::write_to_disk(&wiki_data, workspace_root).unwrap();

    // Verify
    let index_path = workspace_root.join(".theo/wiki/index.md");
    assert!(index_path.exists(), "index.md must exist");

    let modules_dir = workspace_root.join(".theo/wiki/modules");
    let page_count = std::fs::read_dir(&modules_dir).unwrap().count();
    assert!(page_count > 0, "must have module pages");

    // Stats
    eprintln!("{:30} {:>5} {:>6} {:>4} {:>4} {:>8}", "MODULE", "FILES", "SYMS", "ENTR", "DEPS", "COVER");
    eprintln!("{}", "-".repeat(65));
    for doc in &wiki_data.docs {
        eprintln!("{:30} {:>5} {:>6} {:>4} {:>4} {:>7.1}%",
            &doc.title[..doc.title.len().min(30)],
            doc.file_count, doc.symbol_count,
            doc.entry_points.len(), doc.dependencies.len(),
            doc.test_coverage.percentage);
    }

    // Provenance check
    for doc in &wiki_data.docs {
        assert!(!doc.source_refs.is_empty(), "{} has no provenance", doc.slug);
    }

    // Cache check
    let hash = wiki::generator::compute_graph_hash(&graph);
    assert!(wiki::persistence::is_fresh(workspace_root, hash));

    eprintln!("\n=== WIKI E2E: {} pages, {:.0}ms, PASSED ===", wiki_data.docs.len(), gen_time.as_millis());
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
    use theo_engine_graph::cluster::{hierarchical_cluster, ClusterAlgorithm};
    use theo_engine_retrieval::wiki;

    let repo_path = std::env::var("WIKI_REPO").unwrap_or_else(|_| "/tmp/fastapi".to_string());
    let repo_root = std::path::Path::new(&repo_path);

    if !repo_root.exists() {
        eprintln!("SKIP: {} not found. Clone it first.", repo_path);
        return;
    }

    let repo_name = repo_root.file_name().and_then(|n| n.to_str()).unwrap_or("project");
    eprintln!("=== WIKI EXTERNAL: {} ===\n", repo_name);

    // Parse
    let start = std::time::Instant::now();
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(repo_root);
    eprintln!("Parsed: {}/{} files, {} symbols ({:.1}s)",
        stats.files_parsed, stats.files_found, stats.symbols_extracted,
        start.elapsed().as_secs_f64());

    // Build graph
    let (graph, _) = bridge::build_graph(&files);
    eprintln!("Graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());

    // Count test nodes
    let test_nodes = graph.node_ids().filter(|id| {
        graph.get_node(id).map_or(false, |n| n.node_type == theo_engine_graph::model::NodeType::Test)
    }).count();
    eprintln!("Test nodes: {}", test_nodes);

    // Cluster
    let cluster = hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 1.0 });
    eprintln!("Communities: {}", cluster.communities.len());

    // Generate wiki
    let start = std::time::Instant::now();
    let wiki_data = wiki::generator::generate_wiki_with_root(
        &cluster.communities, &graph, repo_name, Some(repo_root)
    );
    let gen_time = start.elapsed();
    eprintln!("Wiki: {} pages in {:.0}ms", wiki_data.docs.len(), gen_time.as_millis());

    // Write wiki markdown
    wiki::persistence::write_to_disk(&wiki_data, repo_root).unwrap();
    eprintln!("Written to {}/.theo/wiki/\n", repo_path);

    // Stats
    eprintln!("{:40} {:>5} {:>6} {:>8}", "MODULE", "FILES", "SYMS", "COVER");
    eprintln!("{}", "-".repeat(65));
    for doc in wiki_data.docs.iter().take(20) {
        eprintln!("{:40} {:>5} {:>6} {:>7.1}%",
            &doc.title[..doc.title.len().min(40)],
            doc.file_count, doc.symbol_count, doc.test_coverage.percentage);
    }
    if wiki_data.docs.len() > 20 {
        eprintln!("  ... and {} more", wiki_data.docs.len() - 20);
    }

    // Render HTML
    let wiki_dir = repo_root.join(".theo/wiki");
    let html = theo_marklive::render(&wiki_dir, theo_marklive::Config {
        title: format!("{} — Code Wiki", repo_name),
        search: true,
    }).unwrap();

    let output = format!("/tmp/{}-wiki.html", repo_name);
    std::fs::write(&output, &html).unwrap();
    eprintln!("\nHTML: {} ({:.0} KB)", output, std::fs::metadata(&output).unwrap().len() as f64 / 1024.0);

    eprintln!("\n=== DONE: open {} in browser ===", output);
}
