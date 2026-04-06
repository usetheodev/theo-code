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
            expected_files: vec![
                "crates/theo-engine-graph/src/cluster.rs",
            ],
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
    if relevant.is_empty() { 0.0 } else { hits as f64 / relevant.len() as f64 }
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
fn extract_files_from_content(items: &[theo_engine_retrieval::assembly::ContextItem]) -> Vec<String> {
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
    use theo_engine_graph::cluster::{hierarchical_cluster, ClusterAlgorithm};
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::search::MultiSignalScorer;
    use theo_engine_retrieval::assembly::{assemble_greedy, assemble_by_symbol, assemble_files_direct};
    use theo_engine_retrieval::search::FileBm25;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()  // crates/
        .parent().unwrap(); // workspace root

    // Build graph from workspace
    eprintln!("Building graph from {}...", workspace_root.display());
    let (files, stats) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    eprintln!("Parsed {}/{} files, {} symbols", stats.files_parsed, stats.files_found, stats.symbols_extracted);

    let (mut graph, _bridge_stats) = bridge::build_graph(&files);
    eprintln!("Graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());

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
            eprintln!("SCIP: no index.scip found (run `rust-analyzer scip . --output .theo/index.scip` to enable)");
        }
    }

    // Use FileLeiden for eval — same as production.
    // Note: Leiden is non-deterministic. Results vary ±10% between runs.
    let cluster_result = hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 0.5 });
    let communities = cluster_result.communities;
    eprintln!("Communities: {} (FileLeiden, non-deterministic)", communities.len());

    // DEBUG: Check BM25 index quality for multiple queries
    let bm25_debug = theo_engine_retrieval::search::Bm25Index::build(&communities, &graph);
    for debug_query in &["assemble_greedy", "LLM provider registry", "OAuth authentication", "error types"] {
        let debug_results = bm25_debug.search(debug_query, &communities);
        let non_zero = debug_results.iter().filter(|r| r.score > 0.0).count();
        let top = debug_results.first().map(|r| format!("{} ({:.2})", r.community.name, r.score)).unwrap_or("none".into());
        eprintln!("BM25 '{}': {}/{} non-zero, top: {}", debug_query, non_zero, communities.len(), top);
    }

    let scorer = MultiSignalScorer::build(&communities, &graph);

    // Run eval
    let queries = ground_truth();
    let k = 5;

    let mut total_precision = 0.0;
    let mut total_recall = 0.0;
    let mut total_mrr = 0.0;
    let mut category_scores: std::collections::HashMap<&str, Vec<(f64, f64, f64)>> = std::collections::HashMap::new();

    eprintln!("\n{:<5} {:<45} {:>8} {:>8} {:>6}", "#", "Query", "P@5", "R@5", "MRR");
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
            eprintln!("  Assembly items: {}, returned files: {:?}", payload.items.len(), &returned_files[..returned_files.len().min(5)]);
        }

        let p = precision_at_k(&returned_files, &eq.expected_files, k);
        let r = recall_at_k(&returned_files, &eq.expected_files, k);
        let m = mrr(&returned_files, &eq.expected_files);

        total_precision += p;
        total_recall += r;
        total_mrr += m;

        category_scores.entry(eq.category).or_default().push((p, r, m));

        eprintln!(
            "{:<5} {:<45} {:>8.2} {:>8.2} {:>6.2}",
            format!("{}.", i + 1),
            if eq.query.len() > 44 { &eq.query[..44] } else { eq.query },
            p, r, m
        );

        // Show what was returned vs expected
        if p < 0.4 {
            eprintln!("  MISS! Expected: {:?}", eq.expected_files);
            eprintln!("  Got: {:?}", &returned_files[..returned_files.len().min(5)]);
        }
    }

    let n = queries.len() as f64;
    let avg_p = total_precision / n;
    let avg_r = total_recall / n;
    let avg_m = total_mrr / n;

    eprintln!("\n{}", "=".repeat(80));
    eprintln!("OVERALL: P@5={:.3}  R@5={:.3}  MRR={:.3}", avg_p, avg_r, avg_m);
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
        eprintln!("\nWARNING: Average precision@5 ({:.3}) is below minimum threshold (0.30)", avg_p);
        eprintln!("This suggests fundamental retrieval problems.");
    }
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
    use theo_engine_graph::cluster::{hierarchical_cluster, ClusterAlgorithm};
    use theo_engine_graph::bridge;
    use theo_engine_retrieval::search::MultiSignalScorer;
    use theo_engine_retrieval::assembly::assemble_greedy;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();

    eprintln!("Building graph...");
    let (files, _) = theo_application::use_cases::extraction::extract_repo(workspace_root);
    let (graph, _) = bridge::build_graph(&files);
    let cluster_result = hierarchical_cluster(&graph, ClusterAlgorithm::FileLeiden { resolution: 0.5 });
    let communities = cluster_result.communities;

    // Score WITH graph attention (default)
    // SAFETY: test-only, single-threaded (#[ignore] test), no other threads reading env.
    unsafe { std::env::remove_var("THEO_NO_GRAPH_ATTENTION"); }
    let scorer_with = MultiSignalScorer::build(&communities, &graph);

    // Score WITHOUT graph attention
    unsafe { std::env::set_var("THEO_NO_GRAPH_ATTENTION", "1"); }
    let scorer_without = MultiSignalScorer::build(&communities, &graph);
    unsafe { std::env::remove_var("THEO_NO_GRAPH_ATTENTION"); }

    let queries = ground_truth();
    let mut changed_count = 0;
    let total = queries.len();

    eprintln!("\n{:<5} {:<45} {:<20} {:<20} {:>6}", "#", "Query", "Top3 WITH", "Top3 WITHOUT", "Changed?");
    eprintln!("{}", "-".repeat(100));

    for (i, eq) in queries.iter().enumerate() {
        let scored_with = scorer_with.score(eq.query, &communities, &graph);
        let scored_without = scorer_without.score(eq.query, &communities, &graph);

        let top3_with: Vec<&str> = scored_with.iter().take(3).map(|s| s.community.id.as_str()).collect();
        let top3_without: Vec<&str> = scored_without.iter().take(3).map(|s| s.community.id.as_str()).collect();

        let changed = top3_with != top3_without;
        if changed { changed_count += 1; }

        let w_names: Vec<&str> = scored_with.iter().take(3).map(|s| s.community.name.as_str()).collect();
        let wo_names: Vec<&str> = scored_without.iter().take(3).map(|s| s.community.name.as_str()).collect();

        eprintln!(
            "{:<5} {:<45} {:<20} {:<20} {:>6}",
            format!("{}.", i + 1),
            if eq.query.len() > 44 { &eq.query[..44] } else { eq.query },
            w_names.first().unwrap_or(&"?"),
            wo_names.first().unwrap_or(&"?"),
            if changed { "YES" } else { "no" }
        );
    }

    let change_pct = (changed_count as f64 / total as f64) * 100.0;
    eprintln!("\n{}", "=".repeat(100));
    eprintln!("RESULT: {}/{} queries ({:.0}%) had top-3 changed by graph attention", changed_count, total, change_pct);

    if change_pct < 20.0 {
        eprintln!("VERDICT: Graph attention changes <20% of rankings → RECOMMEND REMOVAL");
        eprintln!("  The signal adds complexity (graph_attention.rs + 25% weight) without measurable impact.");
    } else {
        eprintln!("VERDICT: Graph attention changes >=20% of rankings → KEEP");
        eprintln!("  The signal contributes meaningfully to ranking diversity.");
    }
}
