/// Benchmark for theo-code-context crate.
///
/// Measures BM25 index build, BM25 query, MultiSignalScorer build + query,
/// and greedy knapsack assembly at varying community counts and token budgets.
///
/// Run with:
///   cargo run --example benchmark -p theo-code-context --release
use std::time::Instant;

use theo_engine_retrieval::{
    assembly::assemble_greedy,
    search::{Bm25Index, MultiSignalScorer},
};
use theo_engine_graph::{
    cluster::Community,
    model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const RUNS: usize = 5;

const COMMUNITY_COUNTS: &[usize] = &[5, 20, 50, 100];
const ASSEMBLY_CANDIDATE_COUNTS: &[usize] = &[10, 50, 100, 500];
const BUDGETS: &[usize] = &[1_000, 8_000, 16_000];

// ---------------------------------------------------------------------------
// Synthetic graph + community generator
// ---------------------------------------------------------------------------

/// Build a graph containing `num_communities` communities, each with
/// `nodes_per_community` symbol nodes.  Adds some CALLS edges across
/// communities so PageRank has something to work with.
fn generate_graph_and_communities(
    num_communities: usize,
    nodes_per_community: usize,
) -> (CodeGraph, Vec<Community>) {
    let mut graph = CodeGraph::new();
    let mut communities: Vec<Community> = Vec::new();

    let total_nodes = num_communities * nodes_per_community;

    // LCG state for pseudo-random edges.
    let mut lcg: u64 = 42;
    let lcg_next = |s: &mut u64| -> usize {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*s >> 33) as usize
    };

    for ci in 0..num_communities {
        let mut node_ids: Vec<String> = Vec::new();
        for ni in 0..nodes_per_community {
            let global_idx = ci * nodes_per_community + ni;
            let id = format!("sym-{global_idx}");
            graph.add_node(Node {
                id: id.clone(),
                node_type: NodeType::Symbol,
                name: format!("function_{global_idx}"),
                file_path: Some(format!("src/module_{ci}.rs")),
                signature: Some(format!(
                    "pub fn process_{global_idx}(input: &Data) -> Result<Output>"
                )),
                kind: Some(SymbolKind::Function),
                line_start: Some(ni * 10 + 1),
                line_end: Some(ni * 10 + 9),
                last_modified: (ci as f64) * 86400.0 * 7.0, // one week apart
                doc: None,
            });
            node_ids.push(id);
        }

        // A few cross-community CALLS edges (~2 per community).
        for _ in 0..2 {
            let src_idx = ci * nodes_per_community + lcg_next(&mut lcg) % nodes_per_community;
            let tgt_idx = lcg_next(&mut lcg) % total_nodes;
            if src_idx != tgt_idx {
                graph.add_edge(Edge {
                    source: format!("sym-{src_idx}"),
                    target: format!("sym-{tgt_idx}"),
                    edge_type: EdgeType::Calls,
                    weight: 1.0,
                });
            }
        }

        communities.push(Community {
            id: format!("comm-{ci}"),
            name: format!("Module {ci}"),
            level: 0,
            node_ids,
            parent_id: None,
            version: 1,
        });
    }

    (graph, communities)
}

// ---------------------------------------------------------------------------
// Timing helper
// ---------------------------------------------------------------------------

fn time_fn<F: FnMut()>(mut f: F) -> (f64, f64, f64) {
    let mut times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t0 = Instant::now();
        f();
        times.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    let min = times.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = times.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    (mean, min, max)
}

// ---------------------------------------------------------------------------
// Benchmark 1: BM25 index build
// ---------------------------------------------------------------------------

fn bench_bm25_build() {
    println!("\nBENCHMARK: bm25_index_build");
    for &nc in COMMUNITY_COUNTS {
        let (graph, communities) = generate_graph_and_communities(nc, 10);
        let (mean, min, max) = time_fn(|| {
            let _ = Bm25Index::build(&communities, &graph);
        });
        println!(
            "  communities={nc:<4}: time_ms(mean={:.3}, min={:.3}, max={:.3})",
            mean, min, max,
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark 2: BM25 query
// ---------------------------------------------------------------------------

fn bench_bm25_query() {
    println!("\nBENCHMARK: bm25_query (single query)");
    let query = "process input data result";
    for &nc in COMMUNITY_COUNTS {
        let (graph, communities) = generate_graph_and_communities(nc, 10);
        let index = Bm25Index::build(&communities, &graph);
        let (mean, min, max) = time_fn(|| {
            let results = index.search(query, &communities);
            std::hint::black_box(&results);
        });
        let top_score = index.search(query, &communities)
            .first()
            .map(|r| r.score)
            .unwrap_or(0.0);
        println!(
            "  communities={nc:<4}: time_ms(mean={:.3}, min={:.3}, max={:.3})  top_score={:.4}",
            mean, min, max, top_score,
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark 3: MultiSignalScorer build + query
// ---------------------------------------------------------------------------

fn bench_multi_signal() {
    println!("\nBENCHMARK: multi_signal_scorer (build + query)");
    let query = "process input data result";
    for &nc in COMMUNITY_COUNTS {
        let (graph, communities) = generate_graph_and_communities(nc, 10);

        // Build time.
        let (build_mean, build_min, build_max) = time_fn(|| {
            let _ = MultiSignalScorer::build(&communities, &graph);
        });

        // Query time.
        let scorer = MultiSignalScorer::build(&communities, &graph);
        let (query_mean, query_min, query_max) = time_fn(|| {
            let results = scorer.score(query, &communities, &graph);
            std::hint::black_box(&results);
        });

        let top_score = scorer.score(query, &communities, &graph)
            .first()
            .map(|r| r.score)
            .unwrap_or(0.0);

        println!(
            "  communities={nc:<4}: build_ms(mean={:.3}, min={:.3}, max={:.3})  query_ms(mean={:.4}, min={:.4}, max={:.4})  top_score={:.4}",
            build_mean, build_min, build_max,
            query_mean, query_min, query_max,
            top_score,
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark 4: Greedy knapsack assembly
// ---------------------------------------------------------------------------

fn bench_assembly() {
    println!("\nBENCHMARK: assembly_greedy (candidates x budget)");
    let query = "process data result";
    for &nc in ASSEMBLY_CANDIDATE_COUNTS {
        for &budget in BUDGETS {
            let (graph, communities) = generate_graph_and_communities(nc, 5);
            let scorer = MultiSignalScorer::build(&communities, &graph);
            let scored = scorer.score(query, &communities, &graph);

            let (mean, min, max) = time_fn(|| {
                let payload = assemble_greedy(&scored, &graph, budget);
                std::hint::black_box(&payload);
            });

            let payload = assemble_greedy(&scored, &graph, budget);
            let utilization = if budget > 0 {
                payload.total_tokens as f64 / budget as f64
            } else {
                0.0
            };
            println!(
                "  candidates={nc:<4} budget={budget:<6}: time_ms(mean={:.4}, min={:.4}, max={:.4})  items_selected={}  utilization={:.2}",
                mean, min, max,
                payload.items.len(),
                utilization,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    println!("=== theo-code-context benchmarks ===");
    println!("Runs per measurement: {RUNS}");
    println!("Build: release");

    bench_bm25_build();
    bench_bm25_query();
    bench_multi_signal();
    bench_assembly();

    println!("\nDone.");
}
