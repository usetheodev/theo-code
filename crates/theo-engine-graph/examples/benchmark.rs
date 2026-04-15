/// Benchmark for theo-code-graph crate.
///
/// Measures graph construction, Louvain/hierarchical clustering, co-change
/// update, and temporal decay across synthetic graphs of varying sizes.
///
/// Run with:
///   cargo run --example benchmark -p theo-code-graph --release
use std::time::Instant;

use theo_engine_graph::{
    cluster::{ClusterAlgorithm, detect_communities, hierarchical_cluster},
    cochange::{DEFAULT_LAMBDA, temporal_decay, update_cochanges},
    model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const RUNS: usize = 5;

/// Graph sizes for graph-construction benchmark (fast — O(n) insert).
const CONSTRUCTION_SIZES: &[usize] = &[100, 500, 1_000, 5_000, 15_000];

/// Graph sizes for clustering benchmarks.
///
/// NOTE: The current Louvain implementation has O(n^3) worst-case complexity
/// because `weight_to_community` and `degree` both iterate the full weight map
/// on every inner loop iteration.  Measured at N=100 ~77 ms, N=200 ~1.2 s.
/// Larger sizes are omitted to keep the benchmark suite practical; this is a
/// known algorithmic bottleneck that should be addressed with adjacency-list
/// based degree/weight helpers.
const CLUSTER_SIZES: &[usize] = &[20, 50, 100, 200];

// ---------------------------------------------------------------------------
// Synthetic graph generator
// ---------------------------------------------------------------------------

/// Build a synthetic `CodeGraph` with:
/// - `num_symbols` Symbol nodes spread across `num_symbols / 10` File nodes.
/// - CONTAINS edges from each File node to its 10 symbols.
/// - CALLS edges: each symbol gets ~`calls_per_node` random outgoing edges.
/// - CO_CHANGES edges: each pair of consecutive file nodes shares an edge.
fn generate_synthetic_graph(num_symbols: usize, calls_per_node: usize) -> CodeGraph {
    let mut graph = CodeGraph::new();

    let num_files = (num_symbols / 10).max(1);

    // --- File nodes ---
    for fi in 0..num_files {
        graph.add_node(Node {
            id: format!("file-{fi}"),
            node_type: NodeType::File,
            name: format!("src/module_{fi}.rs"),
            file_path: Some(format!("src/module_{fi}.rs")),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: (fi as f64) * 86400.0,
            doc: None,
        });
    }

    // --- Symbol nodes (10 per file) ---
    for si in 0..num_symbols {
        let file_idx = si / 10;
        graph.add_node(Node {
            id: format!("sym-{si}"),
            node_type: NodeType::Symbol,
            name: format!("fn_or_struct_{si}"),
            file_path: Some(format!("src/module_{file_idx}.rs")),
            signature: Some(format!("pub fn function_{si}(arg: u32) -> u32")),
            kind: Some(SymbolKind::Function),
            line_start: Some(si * 5 + 1),
            line_end: Some(si * 5 + 4),
            last_modified: (file_idx as f64) * 86400.0,
            doc: None,
        });
        graph.add_edge(Edge {
            source: format!("file-{file_idx}"),
            target: format!("sym-{si}"),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
    }

    // --- CALLS edges (sparse, deterministic LCG) ---
    let calls_per_node = calls_per_node.min(num_symbols.saturating_sub(1));
    let mut lcg_state: u64 = 42;
    let lcg_next = |s: &mut u64| -> usize {
        *s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*s >> 33) as usize
    };

    for si in 0..num_symbols {
        for _ in 0..calls_per_node {
            let target = lcg_next(&mut lcg_state) % num_symbols;
            if target == si {
                continue;
            }
            graph.add_edge(Edge {
                source: format!("sym-{si}"),
                target: format!("sym-{target}"),
                edge_type: EdgeType::Calls,
                weight: 1.0,
            });
        }
    }

    // --- CO_CHANGES between consecutive files ---
    for fi in 0..num_files.saturating_sub(1) {
        graph.add_edge(Edge {
            source: format!("file-{fi}"),
            target: format!("file-{}", fi + 1),
            edge_type: EdgeType::CoChanges,
            weight: temporal_decay(1.0, DEFAULT_LAMBDA),
        });
    }

    graph
}

// ---------------------------------------------------------------------------
// Timing helpers
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

fn estimate_memory_kb(graph: &CodeGraph) -> usize {
    // Each node: ~300 bytes (strings, enums, options).
    // Each edge: ~80 bytes.
    (graph.node_count() * 300 + graph.edge_count() * 80) / 1024
}

// ---------------------------------------------------------------------------
// Benchmark 1: Graph construction
// ---------------------------------------------------------------------------

fn bench_graph_construction() {
    println!("\nBENCHMARK: graph_construction");
    for &n in CONSTRUCTION_SIZES {
        let (mean, min, max) = time_fn(|| {
            let _ = generate_synthetic_graph(n, 2);
        });
        let graph = generate_synthetic_graph(n, 2);
        let mem_kb = estimate_memory_kb(&graph);
        println!(
            "  N={n:<6}: time_ms(mean={:.3}, min={:.3}, max={:.3})  nodes={}  edges={}  memory_kb={}",
            mean,
            min,
            max,
            graph.node_count(),
            graph.edge_count(),
            mem_kb,
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark 2: Louvain clustering
// ---------------------------------------------------------------------------

fn bench_louvain() {
    println!("\nBENCHMARK: louvain_clustering");
    println!("  NOTE: current implementation is O(n^3) — capped at N=200");
    for &n in CLUSTER_SIZES {
        let graph = generate_synthetic_graph(n, 2);
        let (mean, min, max) = time_fn(|| {
            let _ = detect_communities(&graph);
        });
        let result = detect_communities(&graph);
        println!(
            "  N={n:<4}: time_ms(mean={:.3}, min={:.3}, max={:.3})  communities={}  modularity={:.4}",
            mean,
            min,
            max,
            result.communities.len(),
            result.modularity,
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark 3: Hierarchical clustering
// ---------------------------------------------------------------------------

fn bench_hierarchical() {
    println!("\nBENCHMARK: hierarchical_clustering (louvain + LPA)");
    println!("  NOTE: current implementation is O(n^3) — capped at N=100");
    let hier_sizes: &[usize] = &[20, 50, 100];
    for &n in hier_sizes {
        let graph = generate_synthetic_graph(n, 2);
        let (mean, min, max) = time_fn(|| {
            let _ = hierarchical_cluster(&graph, ClusterAlgorithm::Louvain);
        });
        let result = hierarchical_cluster(&graph, ClusterAlgorithm::Louvain);
        let l0 = result.communities.iter().filter(|c| c.level == 0).count();
        let l1 = result.communities.iter().filter(|c| c.level == 1).count();
        println!(
            "  N={n:<4}: time_ms(mean={:.3}, min={:.3}, max={:.3})  level0_comms={}  level1_modules={}",
            mean, min, max, l0, l1,
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark 4: Co-change update
// ---------------------------------------------------------------------------

fn bench_cochange_update() {
    println!("\nBENCHMARK: cochange_update (files changed per commit, base graph N=500)");
    let commit_sizes: &[usize] = &[2, 5, 10, 50];
    let num_symbols = 500;
    let base_graph = generate_synthetic_graph(num_symbols, 2);

    for &changed_count in commit_sizes {
        let changed_files: Vec<String> = (0..changed_count)
            .map(|i| format!("file-{}", i % (num_symbols / 10)))
            .collect();

        let (mean, min, max) = time_fn(|| {
            let mut g = base_graph.clone();
            update_cochanges(&mut g, &changed_files, 0.0);
        });
        let mut g = base_graph.clone();
        update_cochanges(&mut g, &changed_files, 0.0);
        let cochange_edges = g.edges_of_type(&EdgeType::CoChanges).len();
        println!(
            "  changed_files={changed_count:<3}: time_ms(mean={:.3}, min={:.3}, max={:.3})  total_cochange_edges={}",
            mean, min, max, cochange_edges,
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark 5: Temporal decay
// ---------------------------------------------------------------------------

fn bench_temporal_decay() {
    println!("\nBENCHMARK: temporal_decay (batch scalar computation)");
    let batch_sizes: &[usize] = &[1_000, 10_000, 100_000, 1_000_000];
    for &n in batch_sizes {
        let (mean, min, max) = time_fn(|| {
            let sum: f64 = (0..n)
                .map(|d| temporal_decay(d as f64, DEFAULT_LAMBDA))
                .sum();
            std::hint::black_box(sum);
        });
        println!(
            "  N={n:<10}: time_ms(mean={:.4}, min={:.4}, max={:.4})",
            mean, min, max,
        );
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    println!("=== theo-code-graph benchmarks ===");
    println!("Runs per measurement: {RUNS}");
    println!("Build: release");

    bench_graph_construction();
    bench_louvain();
    bench_hierarchical();
    bench_cochange_update();
    bench_temporal_decay();

    println!("\nDone.");
}
