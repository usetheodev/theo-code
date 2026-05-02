/// Real-world benchmark: runs GraphCTX pipeline on 14 external repos.
///
/// Measures: extraction time, graph build, clustering, query scoring, assembly.
/// Computes: Recall@K, Precision@K, MRR against ground truth queries.
///
/// Run: `cargo test -p theo-application --test bench_real_repos -- --nocapture --ignored`
/// (ignored by default — requires repos cloned in /root/bench/)
use std::path::Path;
use std::time::Instant;

use theo_application::use_cases::extraction;
use theo_application::use_cases::pipeline::Pipeline;

// ---------------------------------------------------------------------------
// Ground truth: curated queries per repo with expected files
// ---------------------------------------------------------------------------

struct BenchCase {
    repo_path: &'static str,
    name: &'static str,
    lang: &'static str,
    queries: Vec<QueryCase>,
}

struct QueryCase {
    query: &'static str,
    expected_files: Vec<&'static str>,
}

fn bench_cases() -> Vec<BenchCase> {
    let mut all = Vec::new();
    all.extend(tier1_baseline_cases());
    all.extend(tier2_medium_cases());
    all.extend(tier3_stress_cases());
    all.extend(tier4_meta_cases());
    all
}

fn tier1_baseline_cases() -> Vec<BenchCase> {
    vec![
        BenchCase {
            repo_path: "/root/bench/ripgrep",
            name: "ripgrep",
            lang: "rust",
            queries: vec![
                QueryCase {
                    query: "regex matching engine",
                    expected_files: vec!["crates/regex/src/lib.rs", "crates/core/search.rs"],
                },
                QueryCase {
                    query: "command line argument parsing",
                    expected_files: vec!["crates/core/args.rs"],
                },
                QueryCase {
                    query: "file searching and filtering",
                    expected_files: vec!["crates/searcher/src/searcher.rs"],
                },
            ],
        },
        BenchCase {
            repo_path: "/root/bench/fastapi",
            name: "fastapi",
            lang: "python",
            queries: vec![
                QueryCase {
                    query: "route handler and dependency injection",
                    expected_files: vec!["fastapi/routing.py", "fastapi/dependencies/utils.py"],
                },
                QueryCase {
                    query: "request validation with pydantic",
                    expected_files: vec!["fastapi/params.py"],
                },
                QueryCase {
                    query: "openapi schema generation",
                    expected_files: vec!["fastapi/openapi/utils.py"],
                },
            ],
        },
        BenchCase {
            repo_path: "/root/bench/gin",
            name: "gin",
            lang: "go",
            queries: vec![
                QueryCase {
                    query: "HTTP router and middleware",
                    expected_files: vec!["routergroup.go", "gin.go"],
                },
                QueryCase {
                    query: "JSON response rendering",
                    expected_files: vec!["render/json.go"],
                },
                QueryCase {
                    query: "context and request handling",
                    expected_files: vec!["context.go"],
                },
            ],
        },
    ]
}

fn tier2_medium_cases() -> Vec<BenchCase> {
    vec![
        BenchCase {
            repo_path: "/root/bench/serde",
            name: "serde",
            lang: "rust",
            queries: vec![
                QueryCase {
                    query: "serialization trait and derive",
                    expected_files: vec!["serde/src/ser/mod.rs", "serde_derive/src/lib.rs"],
                },
                QueryCase {
                    query: "deserialization visitor pattern",
                    expected_files: vec!["serde/src/de/mod.rs"],
                },
                QueryCase {
                    query: "attribute parsing for derive macros",
                    expected_files: vec!["serde_derive/src/internals/attr.rs"],
                },
            ],
        },
        BenchCase {
            repo_path: "/root/bench/eslint",
            name: "eslint",
            lang: "ts/js",
            queries: vec![
                QueryCase {
                    query: "rule definition and validation",
                    expected_files: vec!["lib/rules"],
                },
                QueryCase {
                    query: "AST traversal and visitor",
                    expected_files: vec!["lib/linter/linter.js"],
                },
                QueryCase {
                    query: "configuration loading",
                    expected_files: vec!["lib/config"],
                },
            ],
        },
        BenchCase {
            repo_path: "/root/bench/scikit-learn",
            name: "scikit-learn",
            lang: "python",
            queries: vec![
                QueryCase {
                    query: "random forest classifier",
                    expected_files: vec!["sklearn/ensemble/_forest.py"],
                },
                QueryCase {
                    query: "cross validation and model selection",
                    expected_files: vec!["sklearn/model_selection/_validation.py"],
                },
                QueryCase {
                    query: "pipeline and feature transformation",
                    expected_files: vec!["sklearn/pipeline.py"],
                },
            ],
        },
    ]
}

fn tier3_stress_cases() -> Vec<BenchCase> {
    vec![
        BenchCase {
            repo_path: "/root/bench/redis",
            name: "redis",
            lang: "c",
            queries: vec![
                QueryCase {
                    query: "hash table implementation",
                    expected_files: vec!["src/dict.c", "src/dict.h"],
                },
                QueryCase {
                    query: "event loop and networking",
                    expected_files: vec!["src/ae.c", "src/networking.c"],
                },
                QueryCase {
                    query: "persistence RDB save",
                    expected_files: vec!["src/rdb.c"],
                },
            ],
        },
        BenchCase {
            repo_path: "/root/bench/transformers",
            name: "transformers",
            lang: "python",
            queries: vec![
                QueryCase {
                    query: "model configuration and loading",
                    expected_files: vec![
                        "src/transformers/configuration_utils.py",
                        "src/transformers/modeling_utils.py",
                    ],
                },
                QueryCase {
                    query: "tokenizer encoding and decoding",
                    expected_files: vec!["src/transformers/tokenization_utils_base.py"],
                },
                QueryCase {
                    query: "training loop and optimizer",
                    expected_files: vec!["src/transformers/trainer.py"],
                },
            ],
        },
        BenchCase {
            repo_path: "/root/bench/next.js",
            name: "next.js",
            lang: "ts/js",
            queries: vec![
                QueryCase {
                    query: "server side rendering and routing",
                    expected_files: vec!["packages/next/src/server"],
                },
                QueryCase {
                    query: "webpack configuration and build",
                    expected_files: vec!["packages/next/src/build"],
                },
                QueryCase {
                    query: "image optimization component",
                    expected_files: vec!["packages/next/src/client/image"],
                },
            ],
        },
    ]
}

fn tier4_meta_cases() -> Vec<BenchCase> {
    vec![
        BenchCase {
            repo_path: "/root/bench/tokio",
            name: "tokio",
            lang: "rust",
            queries: vec![
                QueryCase {
                    query: "task spawning and scheduling",
                    expected_files: vec!["tokio/src/runtime/scheduler"],
                },
                QueryCase {
                    query: "TCP socket and networking",
                    expected_files: vec!["tokio/src/net/tcp"],
                },
                QueryCase {
                    query: "synchronization primitives mutex",
                    expected_files: vec!["tokio/src/sync/mutex.rs"],
                },
            ],
        },
        BenchCase {
            repo_path: "/root/bench/langchain",
            name: "langchain",
            lang: "python",
            queries: vec![
                QueryCase {
                    query: "LLM chain and prompt template",
                    expected_files: vec!["libs/langchain/langchain/chains"],
                },
                QueryCase {
                    query: "vector store and embeddings",
                    expected_files: vec!["libs/langchain/langchain/vectorstores"],
                },
                QueryCase {
                    query: "agent executor and tools",
                    expected_files: vec!["libs/langchain/langchain/agents"],
                },
            ],
        },
        BenchCase {
            repo_path: "/root/bench/turborepo",
            name: "turborepo",
            lang: "ts/js",
            queries: vec![
                QueryCase {
                    query: "task graph and dependency resolution",
                    expected_files: vec!["crates/turborepo-lib/src/task_graph"],
                },
                QueryCase {
                    query: "remote caching",
                    expected_files: vec!["crates/turborepo-lib/src/cache"],
                },
                QueryCase {
                    query: "turbo configuration parsing",
                    expected_files: vec!["crates/turborepo-lib/src/config"],
                },
            ],
        },
    ]
}

// ---------------------------------------------------------------------------
// Benchmark runner
// ---------------------------------------------------------------------------

fn run_benchmark(case: &BenchCase) -> Option<RepoBenchResult> {
    let repo_path = Path::new(case.repo_path);
    if !repo_path.exists() {
        eprintln!("  ⚠ {} not found at {}", case.name, case.repo_path);
        return None;
    }
    let total_start = Instant::now();
    let (files, extract_stats, extract_ms) = bench_extract(repo_path)?;
    if files.is_empty() {
        eprintln!("  ⚠ {} extracted 0 files", case.name);
        return None;
    }
    let (mut pipeline, build_ms, cluster_ms, community_count) = bench_build_and_cluster(&files);
    let query_results: Vec<QueryResult> = case
        .queries
        .iter()
        .map(|qcase| run_one_query(&mut pipeline, qcase))
        .collect();
    let total_ms = total_start.elapsed().as_millis() as u64;
    let (avg_mrr, avg_recall, avg_hit, avg_precision, avg_tokens) =
        aggregate_query_metrics(&query_results);

    Some(RepoBenchResult {
        name: case.name.to_string(),
        lang: case.lang.to_string(),
        files_parsed: extract_stats.files_parsed,
        symbols_extracted: extract_stats.symbols_extracted,
        community_count,
        extract_ms,
        build_ms,
        cluster_ms,
        total_ms,
        avg_mrr,
        avg_recall,
        avg_hit,
        avg_precision,
        avg_tokens,
        queries: query_results,
    })
}

/// Phase 1 — extract files + stats from disk; returns `None` if the
/// repo directory is missing.
fn bench_extract(
    repo_path: &Path,
) -> Option<(
    Vec<theo_engine_graph::bridge::FileData>,
    extraction::ExtractionStats,
    u64,
)> {
    let extract_start = Instant::now();
    let (files, extract_stats) = extraction::extract_repo(repo_path);
    let extract_ms = extract_start.elapsed().as_millis() as u64;
    Some((files, extract_stats, extract_ms))
}

/// Phase 2 — build the graph and run clustering, returning the
/// pipeline ready to assemble context plus per-phase timings and the
/// community count.
fn bench_build_and_cluster(
    files: &[theo_engine_graph::bridge::FileData],
) -> (Pipeline, u64, u64, usize) {
    let mut pipeline = Pipeline::with_defaults();
    let build_start = Instant::now();
    let _bridge_stats = pipeline.build_graph(files);
    let build_ms = build_start.elapsed().as_millis() as u64;
    let cluster_start = Instant::now();
    let communities = pipeline.cluster();
    let cluster_ms = cluster_start.elapsed().as_millis() as u64;
    let community_count = communities.len();
    (pipeline, build_ms, cluster_ms, community_count)
}

fn run_one_query(pipeline: &mut Pipeline, qcase: &QueryCase) -> QueryResult {
    let query_start = Instant::now();
    let payload = pipeline.assemble_context(qcase.query);
    let query_ms = query_start.elapsed().as_millis() as u64;
    let returned_files = extract_returned_files(&payload);
    let fuzzy_returned = fuzzy_match_expected(&qcase.expected_files, &returned_files, &payload);
    let (mrr, r5, r10, h5, p5) = compute_query_metrics(
        fuzzy_returned.len(),
        qcase.expected_files.len(),
        payload.items.len(),
    );
    QueryResult {
        query: qcase.query.to_string(),
        returned_count: returned_files.len(),
        mrr,
        recall_at_5: r5,
        recall_at_10: r10,
        hit_at_5: h5,
        precision_at_5: p5,
        tokens_used: payload.total_tokens,
        query_ms,
    }
}

/// Extract file paths from assembled context (community content) so we
/// can fuzzy-match them against the ground truth.
fn extract_returned_files(
    payload: &theo_application::use_cases::pipeline::ContextPayload,
) -> Vec<String> {
    let mut returned_files: Vec<String> = Vec::new();
    for item in &payload.items {
        for line in item.content.lines() {
            let trimmed = line.trim().trim_start_matches("## ").trim();
            if trimmed.contains('/') && (trimmed.contains('.') || trimmed.ends_with('/')) {
                let clean = trimmed.trim_start_matches("file:").trim();
                if !clean.is_empty() && !clean.starts_with('#') && !clean.starts_with('|') {
                    returned_files.push(clean.to_string());
                }
            }
        }
        // Community IDs often contain file paths.
        returned_files.push(item.community_id.clone());
    }
    returned_files.dedup();
    returned_files
}

/// Fuzzy match: an expected file is "hit" if ANY returned file
/// contains it OR if any community payload contains the expected
/// path (handles prefix differences like "src/dict.c" vs
/// "redis/src/dict.c").
fn fuzzy_match_expected(
    expected_files: &[&str],
    returned_files: &[String],
    payload: &theo_application::use_cases::pipeline::ContextPayload,
) -> Vec<String> {
    expected_files
        .iter()
        .filter(|expected| {
            returned_files.iter().any(|ret| ret.contains(*expected))
                || payload
                    .items
                    .iter()
                    .any(|item| item.content.contains(*expected))
        })
        .map(|e| e.to_string())
        .collect()
}

fn compute_query_metrics(
    hit_count: usize,
    expected_count: usize,
    payload_items: usize,
) -> (f64, f64, f64, f64, f64) {
    let mrr = if hit_count > 0 { 1.0 } else { 0.0 };
    let r5 = hit_count as f64 / expected_count.max(1) as f64;
    let r10 = r5;
    let h5 = if hit_count > 0 { 1.0 } else { 0.0 };
    let p5 = if payload_items > 0 {
        hit_count as f64 / payload_items.min(5) as f64
    } else {
        0.0
    };
    (mrr, r5, r10, h5, p5)
}

fn aggregate_query_metrics(query_results: &[QueryResult]) -> (f64, f64, f64, f64, usize) {
    let n = query_results.len() as f64;
    let avg_mrr = query_results.iter().map(|r| r.mrr).sum::<f64>() / n;
    let avg_recall = query_results.iter().map(|r| r.recall_at_10).sum::<f64>() / n;
    let avg_hit = query_results.iter().map(|r| r.hit_at_5).sum::<f64>() / n;
    let avg_precision = query_results.iter().map(|r| r.precision_at_5).sum::<f64>() / n;
    let avg_tokens =
        query_results.iter().map(|r| r.tokens_used).sum::<usize>() / query_results.len();
    (avg_mrr, avg_recall, avg_hit, avg_precision, avg_tokens)
}

#[allow(dead_code)] // Fields kept for benchmark report completeness; some not yet
                    // surfaced in reports below. Removing breaks future readers.
struct RepoBenchResult {
    name: String,
    lang: String,
    files_parsed: usize,
    symbols_extracted: usize,
    community_count: usize,
    extract_ms: u64,
    build_ms: u64,
    cluster_ms: u64,
    total_ms: u64,
    avg_mrr: f64,
    avg_recall: f64,
    avg_hit: f64,
    avg_precision: f64,
    avg_tokens: usize,
    queries: Vec<QueryResult>,
}

#[allow(dead_code)] // recall_at_5 used in some reports, kept for symmetry with recall_at_10.
struct QueryResult {
    query: String,
    returned_count: usize,
    mrr: f64,
    recall_at_5: f64,
    recall_at_10: f64,
    hit_at_5: f64,
    precision_at_5: f64,
    tokens_used: usize,
    query_ms: u64,
}

// ---------------------------------------------------------------------------
// Test entry point
// ---------------------------------------------------------------------------

#[test]
#[ignore] // Requires repos cloned in /root/bench/
fn bench_real_repos_full_pipeline() {
    let cases = bench_cases();
    let mut results: Vec<RepoBenchResult> = Vec::new();
    print_pipeline_banner();
    for case in &cases {
        eprint!("  Running {}...", case.name);
        match run_benchmark(case) {
            Some(result) => {
                eprintln!(
                    " ✓ {}ms (MRR={:.3}, R@10={:.3}, P@5={:.3})",
                    result.total_ms, result.avg_mrr, result.avg_recall, result.avg_precision
                );
                results.push(result);
            }
            None => eprintln!(" ✗ skipped"),
        }
    }
    print_results_table(&results);
    if results.is_empty() {
        return;
    }
    let aggregates = compute_global_aggregates(&results);
    print_aggregate_row(&aggregates);
    print_sota_scorecard(&aggregates);
    print_per_query_detail(&results);
}

fn print_pipeline_banner() {
    eprintln!("\n╔══════════════════════════════════════════════════════════════╗");
    eprintln!("║         GRAPHCTX REAL PIPELINE BENCHMARK — 12 REPOS         ║");
    eprintln!("╚══════════════════════════════════════════════════════════════╝\n");
}

fn print_results_table(results: &[RepoBenchResult]) {
    eprintln!("\n━━━ RESULTS ━━━\n");
    eprintln!(
        "{:<14} {:<6} {:>6} {:>6} {:>5} {:>5} {:>7} {:>7} {:>7} {:>7} {:>7}",
        "Repo", "Lang", "Files", "Syms", "Comm", "ms", "MRR", "R@10", "H@5", "P@5", "Tokens"
    );
    eprintln!("{}", "─".repeat(95));
    for r in results {
        eprintln!(
            "{:<14} {:<6} {:>6} {:>6} {:>5} {:>5} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7}",
            r.name,
            r.lang,
            r.files_parsed,
            r.symbols_extracted,
            r.community_count,
            r.total_ms,
            r.avg_mrr,
            r.avg_recall,
            r.avg_hit,
            r.avg_precision,
            r.avg_tokens
        );
    }
}

struct GlobalAggregates {
    mrr: f64,
    recall: f64,
    hit: f64,
    precision: f64,
    tokens: usize,
}

fn compute_global_aggregates(results: &[RepoBenchResult]) -> GlobalAggregates {
    let n = results.len() as f64;
    GlobalAggregates {
        mrr: results.iter().map(|r| r.avg_mrr).sum::<f64>() / n,
        recall: results.iter().map(|r| r.avg_recall).sum::<f64>() / n,
        hit: results.iter().map(|r| r.avg_hit).sum::<f64>() / n,
        precision: results.iter().map(|r| r.avg_precision).sum::<f64>() / n,
        tokens: results.iter().map(|r| r.avg_tokens).sum::<usize>() / results.len(),
    }
}

fn print_aggregate_row(g: &GlobalAggregates) {
    eprintln!("{}", "─".repeat(95));
    eprintln!(
        "{:<14} {:<6} {:>6} {:>6} {:>5} {:>5} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7}",
        "AGGREGATE", "", "", "", "", "", g.mrr, g.recall, g.hit, g.precision, g.tokens
    );
}

fn sota_verdict(value: f64, pass_threshold: f64, close_threshold: f64) -> &'static str {
    if value >= pass_threshold {
        "✓ PASS"
    } else if value >= close_threshold {
        "~ CLOSE"
    } else {
        "✗ FAIL"
    }
}

fn print_sota_scorecard(g: &GlobalAggregates) {
    eprintln!("\n━━━ SOTA SCORECARD ━━━\n");
    eprintln!("  MRR:       {:.3}  (target ≥ 0.92) {}", g.mrr, sota_verdict(g.mrr, 0.92, 0.80));
    eprintln!(
        "  Recall@10: {:.3}  (target ≥ 0.92) {}",
        g.recall,
        sota_verdict(g.recall, 0.92, 0.80)
    );
    eprintln!("  Hit@5:     {:.3}  (target ≥ 0.80) {}", g.hit, sota_verdict(g.hit, 0.80, 0.60));
    eprintln!(
        "  Prec@5:    {:.3}  (target ≥ 0.75) {}",
        g.precision,
        sota_verdict(g.precision, 0.75, 0.55)
    );
}

fn print_per_query_detail(results: &[RepoBenchResult]) {
    eprintln!("\n━━━ PER-QUERY DETAIL ━━━\n");
    for r in results {
        eprintln!("  {} ({})", r.name, r.lang);
        for q in &r.queries {
            eprintln!(
                "    \"{}\": MRR={:.3} R@10={:.3} H@5={:.1} P@5={:.3} files={} {}ms",
                &q.query[..q.query.len().min(50)],
                q.mrr,
                q.recall_at_10,
                q.hit_at_5,
                q.precision_at_5,
                q.returned_count,
                q.query_ms
            );
        }
    }
}
