//! GRAPHCTX Eval Suite — measures retrieval quality with ground truth.
//!
//! 20 queries in 4 categories:
//! A. Symbol exact (5) — "where is verify_token"
//! B. Module (5) — "how does clustering work"
//! C. Semantic (5) — "error handling and recovery"
//! D. Cross-cutting (5) — "what tests exist for auth"
//!
//! Metrics: precision@5, recall@5, MRR.
//! Run with: cargo test -p theo-engine-retrieval --test eval_suite -- --nocapture

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Ground truth
// ---------------------------------------------------------------------------

struct EvalQuery {
    query: &'static str,
    category: &'static str,
    /// Expected files (relative paths). At least 1 of these should appear in top-5.
    expected_files: Vec<&'static str>,
}

fn ground_truth() -> Vec<EvalQuery> {
    vec![
        // --- A. Symbol exact ---
        EvalQuery {
            query: "assemble_greedy",
            category: "symbol",
            expected_files: vec![
                "crates/theo-engine-retrieval/src/assembly.rs",
                "crates/theo-application/src/use_cases/pipeline.rs",
                "crates/theo-application/src/use_cases/graph_context_service.rs",
            ],
        },
        EvalQuery {
            query: "propagate_attention",
            category: "symbol",
            expected_files: vec![
                "crates/theo-engine-retrieval/src/graph_attention.rs",
                "crates/theo-engine-retrieval/src/search.rs",
            ],
        },
        EvalQuery {
            query: "louvain_phase1",
            category: "symbol",
            expected_files: vec!["crates/theo-engine-graph/src/cluster.rs"],
        },
        EvalQuery {
            query: "AgentRunEngine execute",
            category: "symbol",
            expected_files: vec![
                "crates/theo-agent-runtime/src/run_engine.rs",
                "crates/theo-agent-runtime/src/agent_loop.rs",
            ],
        },
        EvalQuery {
            query: "TurboQuantizer quantize",
            category: "symbol",
            expected_files: vec![
                "crates/theo-engine-retrieval/src/embedding/turboquant.rs",
                "crates/theo-engine-retrieval/src/search.rs",
            ],
        },
        // --- B. Module ---
        EvalQuery {
            query: "sandbox bwrap bubblewrap executor",
            category: "module",
            expected_files: vec![
                "crates/theo-tooling/src/sandbox/bwrap.rs",
                "crates/theo-tooling/src/sandbox/executor.rs",
                "crates/theo-tooling/src/sandbox/probe.rs",
            ],
        },
        EvalQuery {
            query: "community detection clustering algorithm",
            category: "module",
            expected_files: vec![
                "crates/theo-engine-graph/src/cluster.rs",
                "crates/theo-engine-graph/src/model.rs",
            ],
        },
        EvalQuery {
            query: "LLM provider registry strategy",
            category: "module",
            expected_files: vec![
                "crates/theo-infra-llm/src/provider/registry.rs",
                "crates/theo-infra-llm/src/provider/mod.rs",
                "crates/theo-infra-llm/src/provider/spec.rs",
                "crates/theo-infra-llm/src/providers/mod.rs",
                "crates/theo-infra-llm/src/lib.rs",
            ],
        },
        EvalQuery {
            query: "tool registry schema category",
            category: "module",
            expected_files: vec![
                "crates/theo-tooling/src/registry/mod.rs",
                "crates/theo-domain/src/tool.rs",
            ],
        },
        EvalQuery {
            query: "agent loop state machine transitions",
            category: "module",
            expected_files: vec![
                "crates/theo-agent-runtime/src/run_engine.rs",
                "crates/theo-agent-runtime/src/agent_loop.rs",
                "crates/theo-agent-runtime/src/state.rs",
                "crates/theo-agent-runtime/src/convergence.rs",
            ],
        },
        // --- C. Semantic ---
        EvalQuery {
            query: "error handling recovery retry",
            category: "semantic",
            expected_files: vec![
                "crates/theo-agent-runtime/src/retry.rs",
                "crates/theo-agent-runtime/src/failure_tracker.rs",
                "crates/theo-agent-runtime/src/dlq.rs",
                "crates/theo-domain/src/error.rs",
            ],
        },
        EvalQuery {
            query: "token budget enforcement truncation",
            category: "semantic",
            expected_files: vec![
                "crates/theo-agent-runtime/src/budget_enforcer.rs",
                "crates/theo-engine-retrieval/src/budget.rs",
                "crates/theo-domain/src/budget.rs",
                "crates/theo-engine-retrieval/src/assembly.rs",
            ],
        },
        EvalQuery {
            query: "OAuth authentication device flow",
            category: "semantic",
            expected_files: vec![
                "crates/theo-infra-auth/src/lib.rs",
                "crates/theo-infra-auth/src/openai.rs",
                "crates/theo-infra-auth/src/pkce.rs",
                "crates/theo-infra-auth/src/copilot.rs",
            ],
        },
        EvalQuery {
            query: "semantic search embeddings TF-IDF",
            category: "semantic",
            expected_files: vec![
                "crates/theo-engine-retrieval/src/embedding/tfidf.rs",
                "crates/theo-engine-retrieval/src/embedding/neural.rs",
                "crates/theo-engine-retrieval/src/search.rs",
            ],
        },
        EvalQuery {
            query: "governance policy impact analysis",
            category: "semantic",
            expected_files: vec![
                "crates/theo-governance/src/sandbox_policy.rs",
                "crates/theo-governance/src/impact.rs",
                "crates/theo-governance/src/sandbox_audit.rs",
            ],
        },
        // --- D. Cross-cutting ---
        EvalQuery {
            query: "sandbox security tests",
            category: "cross-cutting",
            expected_files: vec![
                "crates/theo-tooling/src/sandbox/bwrap.rs",
                "crates/theo-tooling/src/sandbox/executor.rs",
                "crates/theo-governance/src/sandbox_audit.rs",
                "crates/theo-governance/src/sandbox_policy.rs",
            ],
        },
        EvalQuery {
            query: "BM25 scoring tokenization",
            category: "cross-cutting",
            expected_files: vec![
                "crates/theo-engine-retrieval/src/search.rs",
                "crates/theo-engine-retrieval/src/embedding/tfidf.rs",
            ],
        },
        EvalQuery {
            query: "error types defined across crates",
            category: "cross-cutting",
            expected_files: vec![
                "crates/theo-domain/src/error.rs",
                "crates/theo-engine-parser/src/error.rs",
                "crates/theo-infra-llm/src/error.rs",
                "crates/theo-infra-auth/src/error.rs",
            ],
        },
        EvalQuery {
            query: "streaming LLM response parsing",
            category: "cross-cutting",
            expected_files: vec![
                "crates/theo-infra-llm/src/stream.rs",
                "crates/theo-infra-llm/src/client.rs",
                "crates/theo-infra-llm/src/provider/client.rs",
            ],
        },
        EvalQuery {
            query: "compaction context window management",
            category: "cross-cutting",
            expected_files: vec![
                "crates/theo-agent-runtime/src/compaction.rs",
                "crates/theo-agent-runtime/src/run_engine.rs",
                "crates/theo-domain/src/tokens.rs",
            ],
        },
    ]
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

fn precision_at_k(returned_files: &[String], expected: &[&str], k: usize) -> f64 {
    let top_k: HashSet<&str> = returned_files.iter().take(k).map(|s| s.as_str()).collect();
    let relevant: HashSet<&str> = expected.iter().copied().collect();
    let hits = top_k.iter().filter(|f| relevant.contains(**f)).count();
    if k == 0 { 0.0 } else { hits as f64 / k as f64 }
}

fn recall_at_k(returned_files: &[String], expected: &[&str], k: usize) -> f64 {
    let top_k: HashSet<&str> = returned_files.iter().take(k).map(|s| s.as_str()).collect();
    let relevant: HashSet<&str> = expected.iter().copied().collect();
    let hits = top_k.iter().filter(|f| relevant.contains(**f)).count();
    if relevant.is_empty() {
        0.0
    } else {
        hits as f64 / relevant.len() as f64
    }
}

fn mrr(returned_files: &[String], expected: &[&str]) -> f64 {
    let relevant: HashSet<&str> = expected.iter().copied().collect();
    for (i, f) in returned_files.iter().enumerate() {
        if relevant.contains(f.as_str()) {
            return 1.0 / (i + 1) as f64;
        }
    }
    0.0
}

/// Extract unique file paths from assembly context items, ordered by item score.
fn extract_files_from_content(
    items: &[theo_engine_retrieval::assembly::ContextItem],
) -> Vec<String> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    // Items are already sorted by score (descending). Extract files preserving that order.
    for item in items {
        for line in item.content.lines() {
            if line.starts_with("## ") {
                let path = line.trim_start_matches("## ").trim();
                if !path.is_empty() && seen.insert(path.to_string()) {
                    files.push(path.to_string());
                }
            }
        }
    }
    files
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

/// NOTE: This test requires the Theo Code repo to be parsed.
/// It builds a real graph from the workspace and measures retrieval quality.
/// Run with: cargo test -p theo-engine-retrieval --test eval_suite -- --nocapture --ignored
#[test]
#[ignore] // Heavy test — run explicitly with --ignored
fn eval_graphctx_retrieval_quality() {
    use theo_engine_graph::bridge;
    use theo_engine_graph::cluster::{ClusterAlgorithm, hierarchical_cluster};
    use theo_engine_retrieval::assembly::{
        assemble_by_symbol, assemble_files_direct, assemble_greedy,
    };
    use theo_engine_retrieval::search::FileBm25;
    use theo_engine_retrieval::search::MultiSignalScorer;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap() // crates/
        .parent()
        .unwrap(); // workspace root

    // Build graph from workspace
    eprintln!("Building graph from {}...", workspace_root.display());
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!(
        "Parsed {}/{} files, {} symbols",
        stats.files_parsed, stats.files_found, stats.symbols_extracted
    );

    let (mut graph, _bridge_stats) = bridge::build_graph(&files);
    eprintln!(
        "Graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    // SCIP enrichment: if index.scip exists, merge exact cross-file edges.
    // This adds compiler-verified Calls/Imports edges that Tree-Sitter misses.
    #[cfg(feature = "scip")]
    {
        let scip_path = workspace_root.join(".theo/index.scip");
        if scip_path.exists() {
            if let Some(scip_index) = theo_engine_graph::scip::reader::ScipIndex::load(&scip_path) {
                let edges_before = graph.edge_count();
                theo_engine_graph::scip::merge::merge_scip_edges(&mut graph, &scip_index);
                let edges_after = graph.edge_count();
                eprintln!(
                    "SCIP: loaded {} docs, {} occurrences, +{} edges merged",
                    scip_index.document_count,
                    scip_index.occurrence_count,
                    edges_after - edges_before
                );
            } else {
                eprintln!("SCIP: index.scip exists but failed to parse");
            }
        } else {
            eprintln!(
                "SCIP: no index.scip found (run `rust-analyzer scip . --output .theo/index.scip` to enable)"
            );
        }
    }

    // Use FileLeiden for eval — same as production.
    // Note: Leiden is non-deterministic. Results vary ±10% between runs.
    let cluster_result =
        hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 0.5 });
    let communities = cluster_result.communities;
    eprintln!(
        "Communities: {} (FileLeiden, non-deterministic)",
        communities.len()
    );

    // DEBUG: Check BM25 index quality for multiple queries
    let bm25_debug = theo_engine_retrieval::search::Bm25Index::build(&communities, &graph);
    for debug_query in &[
        "assemble_greedy",
        "LLM provider registry",
        "OAuth authentication",
        "error types",
    ] {
        let debug_results = bm25_debug.search(debug_query, &communities);
        let non_zero = debug_results.iter().filter(|r| r.score > 0.0).count();
        let top = debug_results
            .first()
            .map(|r| format!("{} ({:.2})", r.community.name, r.score))
            .unwrap_or("none".into());
        eprintln!(
            "BM25 '{}': {}/{} non-zero, top: {}",
            debug_query,
            non_zero,
            communities.len(),
            top
        );
    }

    let _scorer = MultiSignalScorer::build(&communities, &graph);

    // Run eval
    let queries = ground_truth();
    let k = 5;

    let mut total_precision = 0.0;
    let mut total_recall = 0.0;
    let mut total_mrr = 0.0;
    let mut category_scores: std::collections::HashMap<&str, Vec<(f64, f64, f64)>> =
        std::collections::HashMap::new();

    eprintln!(
        "\n{:<5} {:<45} {:>8} {:>8} {:>6}",
        "#", "Query", "P@5", "R@5", "MRR"
    );
    eprintln!("{}", "-".repeat(80));

    for (i, eq) in queries.iter().enumerate() {
        // File-direct ranking: rank FILES by FileBm25, not communities.
        // This is the FAANG pattern (Zoekt/Sourcegraph/CodeCompass).
        let file_scores = FileBm25::search(&graph, eq.query);
        let payload = assemble_files_direct(&file_scores, &graph, &communities, 16_384);
        let mut returned_files = extract_files_from_content(&payload.items);

        // Veto protocol: if file-direct returned nothing, try symbol lookup.
        if returned_files.is_empty() {
            let symbol_payload = assemble_by_symbol(eq.query, &graph, 16_384);
            let symbol_files = extract_files_from_content(&symbol_payload.items);
            if !symbol_files.is_empty() {
                returned_files = symbol_files;
            }
        }

        // Debug: show top-5 files from FileBm25 for failing queries
        if eq.query.contains("LLM provider") || eq.query.contains("OAuth") || i == 0 {
            let mut top_files: Vec<_> = file_scores.iter().collect();
            top_files.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
            eprintln!("\nDEBUG TOP-5 FILES for '{}' (FileBm25 direct):", eq.query);
            for (j, (path, score)) in top_files.iter().take(5).enumerate() {
                eprintln!("  {}: {} score={:.4}", j, path, score);
            }
            eprintln!(
                "  Assembly items: {}, returned files: {:?}",
                payload.items.len(),
                &returned_files[..returned_files.len().min(5)]
            );
        }

        let p = precision_at_k(&returned_files, &eq.expected_files, k);
        let r = recall_at_k(&returned_files, &eq.expected_files, k);
        let m = mrr(&returned_files, &eq.expected_files);

        total_precision += p;
        total_recall += r;
        total_mrr += m;

        category_scores
            .entry(eq.category)
            .or_default()
            .push((p, r, m));

        eprintln!(
            "{:<5} {:<45} {:>8.2} {:>8.2} {:>6.2}",
            format!("{}.", i + 1),
            if eq.query.len() > 44 {
                &eq.query[..44]
            } else {
                eq.query
            },
            p,
            r,
            m
        );

        // Show what was returned vs expected
        if p < 0.4 {
            eprintln!("  MISS! Expected: {:?}", eq.expected_files);
            eprintln!(
                "  Got: {:?}",
                &returned_files[..returned_files.len().min(5)]
            );
        }
    }

    let n = queries.len() as f64;
    let avg_p = total_precision / n;
    let avg_r = total_recall / n;
    let avg_m = total_mrr / n;

    eprintln!("\n{}", "=".repeat(80));
    eprintln!(
        "OVERALL: P@5={:.3}  R@5={:.3}  MRR={:.3}",
        avg_p, avg_r, avg_m
    );
    eprintln!();

    for (cat, scores) in &category_scores {
        let cp: f64 = scores.iter().map(|(p, _, _)| p).sum::<f64>() / scores.len() as f64;
        let cr: f64 = scores.iter().map(|(_, r, _)| r).sum::<f64>() / scores.len() as f64;
        let cm: f64 = scores.iter().map(|(_, _, m)| m).sum::<f64>() / scores.len() as f64;
        eprintln!("  {:<15} P@5={:.3}  R@5={:.3}  MRR={:.3}", cat, cp, cr, cm);
    }

    eprintln!("\nThresholds: P@5 >= 0.475 (acceptable), >= 0.675 (good)");
    eprintln!("           R@5 >= 0.475 (acceptable), >= 0.675 (good)");

    // Soft assertion — report but don't fail on first run (calibration)
    if avg_p < 0.30 {
        eprintln!(
            "\nWARNING: Average precision@5 ({:.3}) is below minimum threshold (0.30)",
            avg_p
        );
        eprintln!("This suggests fundamental retrieval problems.");
    }
}

/// RRF 3-ranker fusion: BM25 + Tantivy + Dense embeddings.
///
/// Run: cargo test -p theo-engine-retrieval --features dense-retrieval --test eval_suite -- --ignored --nocapture eval_rrf_dense
#[test]
#[ignore]
#[cfg(feature = "dense-retrieval")]
fn eval_rrf_dense() {
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::dense_search::FileDenseSearch;
    use theo_engine_retrieval::embedding::cache::EmbeddingCache;
    use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
    use theo_engine_retrieval::search::FileBm25;
    use theo_engine_retrieval::tantivy_search::{FileTantivyIndex, hybrid_rrf_search};

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("Building graph...");
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

    // Build Tantivy index
    let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy index build failed");
    eprintln!("Tantivy: {} docs indexed", tantivy_index.num_docs());

    // Build dense embeddings
    let embedder = NeuralEmbedder::new().expect("NeuralEmbedder init failed");
    eprintln!("Building embedding cache...");
    let start = std::time::Instant::now();
    let cache = EmbeddingCache::build(&graph, &embedder);
    let build_time = start.elapsed();
    eprintln!(
        "Embedding cache: {} files, built in {:.1}s",
        cache.len(),
        build_time.as_secs_f64()
    );

    let queries = ground_truth();
    let k = 5;

    // --- Baseline: Custom BM25 only (for A/B comparison) ---
    {
        let mut total_p = 0.0;
        let mut total_m = 0.0;
        for eq in queries.iter() {
            let scores = FileBm25::search(&graph, eq.query);
            let files = extract_files_from_scores(&scores);
            total_p += precision_at_k(&files, &eq.expected_files, k);
            total_m += mrr(&files, &eq.expected_files);
        }
        let n = queries.len() as f64;
        eprintln!(
            "\nBASELINE (BM25 only): P@5={:.3}  MRR={:.3}",
            total_p / n,
            total_m / n
        );
    }

    // --- Dense only (with diagnostic for weak queries) ---
    {
        let mut total_p = 0.0;
        let mut total_m = 0.0;
        eprintln!("\n--- DENSE RANKER DIAGNOSTIC (expected file positions) ---");
        for (i, eq) in queries.iter().enumerate() {
            let scores = FileDenseSearch::search(&embedder, &cache, eq.query, 200);
            let files = extract_files_from_scores(&scores);
            let p = precision_at_k(&files, &eq.expected_files, k);
            total_p += p;
            total_m += mrr(&files, &eq.expected_files);

            // For weak queries: show where expected files rank in dense
            if p < 0.40 {
                let expected_ranks: Vec<String> = eq
                    .expected_files
                    .iter()
                    .map(|ef| match files.iter().position(|f| f == *ef) {
                        Some(pos) => format!("{}@{}", ef.split('/').last().unwrap_or(ef), pos + 1),
                        None => format!("{}@MISS", ef.split('/').last().unwrap_or(ef)),
                    })
                    .collect();
                eprintln!(
                    "  Q{} '{}': P@5={:.2} dense_ranks=[{}]",
                    i + 1,
                    if eq.query.len() > 30 {
                        &eq.query[..30]
                    } else {
                        eq.query
                    },
                    p,
                    expected_ranks.join(", ")
                );
            }
        }
        let n = queries.len() as f64;
        eprintln!(
            "DENSE ONLY: P@5={:.3}  MRR={:.3}\n",
            total_p / n,
            total_m / n
        );
    }

    // --- RRF 3-ranker with different k values ---
    // Use extract_files_from_scores instead of assembly to avoid community dilution
    for rrf_k in [10.0, 20.0, 40.0] {
        let mut total_p = 0.0;
        let mut total_r = 0.0;
        let mut total_m = 0.0;

        eprintln!("\n=== RRF 3-RANKER k={} ===", rrf_k);
        eprintln!(
            "{:<5} {:<45} {:>8} {:>8} {:>6}",
            "#", "Query", "P@5", "R@5", "MRR"
        );
        eprintln!("{}", "-".repeat(80));

        for (i, eq) in queries.iter().enumerate() {
            let rrf_scores =
                hybrid_rrf_search(&graph, &tantivy_index, &embedder, &cache, eq.query, rrf_k);
            // Extract files directly from scores — no assembly expansion
            let returned_files = extract_files_from_scores(&rrf_scores);

            let p = precision_at_k(&returned_files, &eq.expected_files, k);
            let r = recall_at_k(&returned_files, &eq.expected_files, k);
            let m = mrr(&returned_files, &eq.expected_files);

            total_p += p;
            total_r += r;
            total_m += m;

            eprintln!(
                "{:<5} {:<45} {:>8.2} {:>8.2} {:>6.2}",
                format!("{}.", i + 1),
                if eq.query.len() > 44 {
                    &eq.query[..44]
                } else {
                    eq.query
                },
                p,
                r,
                m
            );

            if p < 0.2 {
                eprintln!("  MISS! Expected: {:?}", eq.expected_files);
                eprintln!(
                    "  Got: {:?}",
                    &returned_files[..returned_files.len().min(5)]
                );
            }
        }

        let n = queries.len() as f64;
        let avg_p = total_p / n;
        let avg_r = total_r / n;
        let avg_m = total_m / n;

        eprintln!(
            "\nRRF k={}: P@5={:.3}  R@5={:.3}  MRR={:.3}",
            rrf_k, avg_p, avg_r, avg_m
        );
    }

    // --- RRF + Assembly (reverse dep boost + test penalty) ---
    {
        use theo_engine_graph::cluster::{ClusterAlgorithm, hierarchical_cluster};
        use theo_engine_retrieval::assembly::assemble_files_direct;

        let cluster_result =
            hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 0.5 });
        let communities = cluster_result.communities;

        eprintln!("\n=== RRF k=20 + ASSEMBLY (reverse dep boost) ===");
        let mut total_p = 0.0;
        let mut total_r = 0.0;
        let mut total_m = 0.0;

        for (i, eq) in queries.iter().enumerate() {
            let rrf_scores =
                hybrid_rrf_search(&graph, &tantivy_index, &embedder, &cache, eq.query, 20.0);
            let payload = assemble_files_direct(&rrf_scores, &graph, &communities, 16_384);
            let returned_files = extract_files_from_content(&payload.items);

            let p = precision_at_k(&returned_files, &eq.expected_files, k);
            let r = recall_at_k(&returned_files, &eq.expected_files, k);
            let m = mrr(&returned_files, &eq.expected_files);

            total_p += p;
            total_r += r;
            total_m += m;

            if p < 0.2 {
                eprintln!(
                    "  Q{} MISS: {:?}",
                    i + 1,
                    &returned_files[..returned_files.len().min(5)]
                );
            }
        }

        let n = queries.len() as f64;
        eprintln!(
            "RRF+ASSEMBLY: P@5={:.3}  R@5={:.3}  MRR={:.3}",
            total_p / n,
            total_r / n,
            total_m / n
        );
    }

    // --- 2-RANKER RRF (BM25 + Dense, no Tantivy) ---
    {
        eprintln!("\n=== 2-RANKER RRF (BM25 + Dense) k=60 ===");
        let mut total_p = 0.0;
        let mut total_m = 0.0;

        for eq in queries.iter() {
            let bm25_scores = FileBm25::search(&graph, eq.query);
            let dense_scores = FileDenseSearch::search(&embedder, &cache, eq.query, 100);

            let is_noise = |p: &str| {
                let lp = p.to_lowercase();
                lp.contains("test") || lp.contains("benchmark") || lp.contains("example")
            };
            let to_ranked = |scores: &std::collections::HashMap<String, f64>| -> Vec<String> {
                let mut s: Vec<_> = scores.iter().filter(|(k, _)| !is_noise(k)).collect();
                s.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
                s.into_iter().map(|(k, _)| k.clone()).collect()
            };

            let bm25_ranked = to_ranked(&bm25_scores);
            let dense_ranked = to_ranked(&dense_scores);

            let bm25_rank: std::collections::HashMap<String, usize> = bm25_ranked
                .iter()
                .enumerate()
                .map(|(i, p)| (p.clone(), i))
                .collect();
            let dense_rank: std::collections::HashMap<String, usize> = dense_ranked
                .iter()
                .enumerate()
                .map(|(i, p)| (p.clone(), i))
                .collect();

            let mut all: std::collections::HashSet<String> = std::collections::HashSet::new();
            for k in bm25_scores.keys() {
                if !is_noise(k) {
                    all.insert(k.clone());
                }
            }
            for k in dense_scores.keys() {
                if !is_noise(k) {
                    all.insert(k.clone());
                }
            }

            let mut merged: std::collections::HashMap<String, f64> =
                std::collections::HashMap::new();
            for path in all {
                let mut s = 0.0;
                if let Some(&r) = bm25_rank.get(&path) {
                    s += 1.0 / (60.0 + r as f64);
                }
                if let Some(&r) = dense_rank.get(&path) {
                    s += 1.0 / (60.0 + r as f64);
                }
                if s > 0.0 {
                    merged.insert(path, s);
                }
            }

            let returned_files = extract_files_from_scores(&merged);
            total_p += precision_at_k(&returned_files, &eq.expected_files, k);
            total_m += mrr(&returned_files, &eq.expected_files);
        }

        let n = queries.len() as f64;
        eprintln!("2-RANKER: P@5={:.3}  MRR={:.3}", total_p / n, total_m / n);
    }
}

/// Full pipeline eval: RRF + Cross-Encoder Reranker (Jina multilingual).
///
/// Run: cargo test -p theo-engine-retrieval --features reranker --test eval_suite -- --ignored --nocapture eval_full_pipeline
#[test]
#[ignore]
#[cfg(feature = "reranker")]
fn eval_full_pipeline() {
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::embedding::cache::EmbeddingCache;
    use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
    use theo_engine_retrieval::pipeline::retrieve_and_rerank;
    use theo_engine_retrieval::reranker::CrossEncoderReranker;
    use theo_engine_retrieval::tantivy_search::FileTantivyIndex;
    use theo_engine_retrieval::tantivy_search::hybrid_rrf_search;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("Building graph...");
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

    let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy index build failed");
    eprintln!("Tantivy: {} docs indexed", tantivy_index.num_docs());

    let embedder = NeuralEmbedder::new().expect("NeuralEmbedder init failed");
    let cache = EmbeddingCache::build(&graph, &embedder);
    eprintln!("Embedding cache: {} files", cache.len());

    let reranker = CrossEncoderReranker::new().expect("CrossEncoderReranker init failed");
    eprintln!("Reranker: Jina v2 multilingual loaded");

    let queries = ground_truth();
    let k = 5;

    // --- A/B: RRF-only (baseline for comparison) ---
    let mut rrf_total_p = 0.0;
    let mut rrf_total_m = 0.0;
    for eq in queries.iter() {
        let scores = hybrid_rrf_search(&graph, &tantivy_index, &embedder, &cache, eq.query, 20.0);
        let files = extract_files_from_scores(&scores);
        rrf_total_p += precision_at_k(&files, &eq.expected_files, k);
        rrf_total_m += mrr(&files, &eq.expected_files);
    }
    let n = queries.len() as f64;
    eprintln!(
        "\nRRF-ONLY: P@5={:.3}  MRR={:.3}",
        rrf_total_p / n,
        rrf_total_m / n
    );

    // --- Full pipeline: RRF + Reranker ---
    let mut total_p = 0.0;
    let mut total_r = 0.0;
    let mut total_m = 0.0;
    let mut total_rerank_ms = 0.0;

    eprintln!("\n=== FULL PIPELINE (RRF k=20 + Reranker top-20) ===");
    eprintln!(
        "{:<5} {:<45} {:>8} {:>8} {:>6} {:>8}",
        "#", "Query", "P@5", "R@5", "MRR", "ms"
    );
    eprintln!("{}", "-".repeat(90));

    for (i, eq) in queries.iter().enumerate() {
        let start = std::time::Instant::now();
        let reranked = retrieve_and_rerank(
            &graph,
            &tantivy_index,
            &embedder,
            &cache,
            &reranker,
            eq.query,
            20.0,
            20,
        );
        let elapsed = start.elapsed();
        total_rerank_ms += elapsed.as_millis() as f64;

        let returned_files = extract_files_from_scores(&reranked);

        let p = precision_at_k(&returned_files, &eq.expected_files, k);
        let r = recall_at_k(&returned_files, &eq.expected_files, k);
        let m = mrr(&returned_files, &eq.expected_files);

        total_p += p;
        total_r += r;
        total_m += m;

        eprintln!(
            "{:<5} {:<45} {:>8.2} {:>8.2} {:>6.2} {:>8.0}",
            format!("{}.", i + 1),
            if eq.query.len() > 44 {
                &eq.query[..44]
            } else {
                eq.query
            },
            p,
            r,
            m,
            elapsed.as_millis()
        );

        if p < 0.2 {
            eprintln!("  MISS! Expected: {:?}", eq.expected_files);
            eprintln!(
                "  Got: {:?}",
                &returned_files[..returned_files.len().min(5)]
            );
        }
    }

    let avg_p = total_p / n;
    let avg_r = total_r / n;
    let avg_m = total_m / n;
    let avg_ms = total_rerank_ms / n;

    eprintln!("\n{}", "=".repeat(90));
    eprintln!(
        "FULL PIPELINE: P@5={:.3}  R@5={:.3}  MRR={:.3}  avg_ms={:.0}",
        avg_p, avg_r, avg_m, avg_ms
    );
    eprintln!(
        "vs RRF-ONLY:   P@5={:.3}  MRR={:.3}",
        rrf_total_p / n,
        rrf_total_m / n
    );
    eprintln!(
        "Delta:         P@5={:+.3}  MRR={:+.3}",
        avg_p - rrf_total_p / n,
        avg_m - rrf_total_m / n
    );
}

/// A/B benchmark: Custom FileBm25 vs Tantivy backend.
///
/// Both use BM25F with same field boosts (filename 5x, symbol 3x, sig 1x, doc 1x).
/// Measures P@5, R@5, MRR for both backends and compares.
///
/// Run: cargo test -p theo-engine-retrieval --features tantivy-backend --test eval_suite -- --ignored --nocapture eval_tantivy_vs_custom
#[test]
#[ignore]
#[cfg(feature = "tantivy-backend")]
fn eval_tantivy_vs_custom() {
    use theo_engine_graph::bridge;
    use theo_engine_graph::cluster::{ClusterAlgorithm, hierarchical_cluster};
    use theo_engine_retrieval::assembly::assemble_files_direct;
    use theo_engine_retrieval::search::FileBm25;
    use theo_engine_retrieval::tantivy_search::FileTantivyIndex;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("Building graph...");
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

    let cluster_result =
        hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 0.5 });
    let communities = cluster_result.communities;
    eprintln!("Communities: {}", communities.len());

    // Build Tantivy index
    let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy index build failed");
    eprintln!("Tantivy: {} docs indexed", tantivy_index.num_docs());

    let queries = ground_truth();
    let k = 5;

    let mut custom_total_p = 0.0;
    let mut custom_total_m = 0.0;
    let mut tantivy_total_p = 0.0;
    let mut tantivy_total_m = 0.0;

    eprintln!(
        "\n{:<5} {:<40} {:>10} {:>10} {:>10} {:>10}",
        "#", "Query", "Custom P@5", "Tantivy P@5", "Custom MRR", "Tantivy MRR"
    );
    eprintln!("{}", "-".repeat(95));

    for (i, eq) in queries.iter().enumerate() {
        // Custom FileBm25
        let custom_scores = FileBm25::search(&graph, eq.query);
        let custom_payload = assemble_files_direct(&custom_scores, &graph, &communities, 16_384);
        let custom_files = extract_files_from_content(&custom_payload.items);

        // Tantivy
        let tantivy_scores = tantivy_index
            .search_with_prf(&graph, eq.query, 50)
            .unwrap_or_default();
        let tantivy_payload = assemble_files_direct(&tantivy_scores, &graph, &communities, 16_384);
        let tantivy_files = extract_files_from_content(&tantivy_payload.items);

        let cp = precision_at_k(&custom_files, &eq.expected_files, k);
        let cm = mrr(&custom_files, &eq.expected_files);
        let tp = precision_at_k(&tantivy_files, &eq.expected_files, k);
        let tm = mrr(&tantivy_files, &eq.expected_files);

        custom_total_p += cp;
        custom_total_m += cm;
        tantivy_total_p += tp;
        tantivy_total_m += tm;

        let winner = if tp > cp {
            "TANTIVY"
        } else if cp > tp {
            "CUSTOM"
        } else {
            "TIE"
        };

        eprintln!(
            "{:<5} {:<40} {:>10.2} {:>10.2} {:>10.2} {:>10.2}  {}",
            format!("{}.", i + 1),
            if eq.query.len() > 39 {
                &eq.query[..39]
            } else {
                eq.query
            },
            cp,
            tp,
            cm,
            tm,
            winner
        );
    }

    let n = queries.len() as f64;
    eprintln!("\n{}", "=".repeat(95));
    eprintln!(
        "CUSTOM:  P@5={:.3}  MRR={:.3}",
        custom_total_p / n,
        custom_total_m / n
    );
    eprintln!(
        "TANTIVY: P@5={:.3}  MRR={:.3}",
        tantivy_total_p / n,
        tantivy_total_m / n
    );

    let p5_gate = 0.40;
    let mrr_gate = 0.85;
    let tantivy_p5 = tantivy_total_p / n;
    let tantivy_mrr = tantivy_total_m / n;

    eprintln!("\nGate: P@5 >= {:.2}, MRR >= {:.2}", p5_gate, mrr_gate);
    eprintln!(
        "Tantivy: P@5={:.3} {}, MRR={:.3} {}",
        tantivy_p5,
        if tantivy_p5 >= p5_gate {
            "PASS"
        } else {
            "FAIL"
        },
        tantivy_mrr,
        if tantivy_mrr >= mrr_gate {
            "PASS"
        } else {
            "FAIL"
        },
    );
}

/// Hybrid search: combine Custom + Tantivy for best of both.
///
/// Run: cargo test -p theo-engine-retrieval --features tantivy-backend --test eval_suite -- --ignored --nocapture eval_hybrid_search
#[test]
#[ignore]
#[cfg(feature = "tantivy-backend")]
fn eval_hybrid_search() {
    use theo_engine_graph::bridge;
    use theo_engine_graph::cluster::{ClusterAlgorithm, hierarchical_cluster};
    use theo_engine_retrieval::assembly::assemble_files_direct;
    use theo_engine_retrieval::tantivy_search::{FileTantivyIndex, hybrid_search};

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("Building graph...");
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

    let cluster_result =
        hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 0.5 });
    let communities = cluster_result.communities;

    let tantivy_index = FileTantivyIndex::build(&graph).expect("Tantivy index build failed");
    eprintln!("Tantivy: {} docs indexed", tantivy_index.num_docs());

    let queries = ground_truth();
    let k = 5;

    let mut total_p = 0.0;
    let mut total_r = 0.0;
    let mut total_m = 0.0;

    eprintln!(
        "\n{:<5} {:<45} {:>8} {:>8} {:>6}",
        "#", "Query", "P@5", "R@5", "MRR"
    );
    eprintln!("{}", "-".repeat(80));

    for (i, eq) in queries.iter().enumerate() {
        let hybrid_scores = hybrid_search(&graph, &tantivy_index, eq.query);
        let payload = assemble_files_direct(&hybrid_scores, &graph, &communities, 16_384);
        let returned_files = extract_files_from_content(&payload.items);

        let p = precision_at_k(&returned_files, &eq.expected_files, k);
        let r = recall_at_k(&returned_files, &eq.expected_files, k);
        let m = mrr(&returned_files, &eq.expected_files);

        total_p += p;
        total_r += r;
        total_m += m;

        eprintln!(
            "{:<5} {:<45} {:>8.2} {:>8.2} {:>6.2}",
            format!("{}.", i + 1),
            if eq.query.len() > 44 {
                &eq.query[..44]
            } else {
                eq.query
            },
            p,
            r,
            m
        );

        if p < 0.2 {
            eprintln!("  MISS! Expected: {:?}", eq.expected_files);
            eprintln!(
                "  Got: {:?}",
                &returned_files[..returned_files.len().min(5)]
            );
        }
    }

    let n = queries.len() as f64;
    let avg_p = total_p / n;
    let avg_r = total_r / n;
    let avg_m = total_m / n;

    eprintln!("\n{}", "=".repeat(80));
    eprintln!(
        "HYBRID: P@5={:.3}  R@5={:.3}  MRR={:.3}",
        avg_p, avg_r, avg_m
    );

    let p5_gate = 0.40;
    let mrr_gate = 0.85;
    eprintln!(
        "\nGate: P@5 >= {:.2} {}, MRR >= {:.2} {}",
        p5_gate,
        if avg_p >= p5_gate { "PASS" } else { "FAIL" },
        mrr_gate,
        if avg_m >= mrr_gate { "PASS" } else { "FAIL" },
    );
}

/// A/B benchmark: graph attention ON vs OFF.
///
/// Builds the same graph twice, scores all 20 queries with and without graph attention,
/// compares top-3 community rankings. If <20% of queries change top-3, graph attention
/// is noise and should be removed.
///
/// Run: THEO_NO_GRAPH_ATTENTION=1 cargo test -p theo-engine-retrieval --test eval_suite -- --ignored --nocapture eval_graph_attention_ab
#[test]
#[ignore]
fn eval_graph_attention_ab() {
    use theo_engine_graph::bridge;
    use theo_engine_graph::cluster::{ClusterAlgorithm, hierarchical_cluster};
    use theo_engine_retrieval::assembly::assemble_greedy;
    use theo_engine_retrieval::search::MultiSignalScorer;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    eprintln!("Building graph...");
    let (files, _) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    let (graph, _) = bridge::build_graph(&files);
    let cluster_result =
        hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 0.5 });
    let communities = cluster_result.communities;

    // Score WITH graph attention (default)
    // SAFETY: test-only, single-threaded (#[ignore] test), no other threads reading env.
    unsafe {
        std::env::remove_var("THEO_NO_GRAPH_ATTENTION");
    }
    let scorer_with = MultiSignalScorer::build(&communities, &graph);

    // Score WITHOUT graph attention
    unsafe {
        std::env::set_var("THEO_NO_GRAPH_ATTENTION", "1");
    }
    let scorer_without = MultiSignalScorer::build(&communities, &graph);
    unsafe {
        std::env::remove_var("THEO_NO_GRAPH_ATTENTION");
    }

    let queries = ground_truth();
    let mut changed_count = 0;
    let total = queries.len();

    eprintln!(
        "\n{:<5} {:<45} {:<20} {:<20} {:>6}",
        "#", "Query", "Top3 WITH", "Top3 WITHOUT", "Changed?"
    );
    eprintln!("{}", "-".repeat(100));

    for (i, eq) in queries.iter().enumerate() {
        let scored_with = scorer_with.score(eq.query, &communities, &graph);
        let scored_without = scorer_without.score(eq.query, &communities, &graph);

        let top3_with: Vec<&str> = scored_with
            .iter()
            .take(3)
            .map(|s| s.community.id.as_str())
            .collect();
        let top3_without: Vec<&str> = scored_without
            .iter()
            .take(3)
            .map(|s| s.community.id.as_str())
            .collect();

        let changed = top3_with != top3_without;
        if changed {
            changed_count += 1;
        }

        let w_names: Vec<&str> = scored_with
            .iter()
            .take(3)
            .map(|s| s.community.name.as_str())
            .collect();
        let wo_names: Vec<&str> = scored_without
            .iter()
            .take(3)
            .map(|s| s.community.name.as_str())
            .collect();

        eprintln!(
            "{:<5} {:<45} {:<20} {:<20} {:>6}",
            format!("{}.", i + 1),
            if eq.query.len() > 44 {
                &eq.query[..44]
            } else {
                eq.query
            },
            w_names.first().unwrap_or(&"?"),
            wo_names.first().unwrap_or(&"?"),
            if changed { "YES" } else { "no" }
        );
    }

    let change_pct = (changed_count as f64 / total as f64) * 100.0;
    eprintln!("\n{}", "=".repeat(100));
    eprintln!(
        "RESULT: {}/{} queries ({:.0}%) had top-3 changed by graph attention",
        changed_count, total, change_pct
    );

    if change_pct < 20.0 {
        eprintln!("VERDICT: Graph attention changes <20% of rankings → RECOMMEND REMOVAL");
        eprintln!(
            "  The signal adds complexity (graph_attention.rs + 25% weight) without measurable impact."
        );
    } else {
        eprintln!("VERDICT: Graph attention changes >=20% of rankings → KEEP");
        eprintln!("  The signal contributes meaningfully to ranking diversity.");
    }
}
