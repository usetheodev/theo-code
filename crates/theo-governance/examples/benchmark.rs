/// Benchmark for theo-code-governance crate.
///
/// Measures BFS impact analysis at varying graph sizes. Communities are
/// constructed synthetically (one community per file, 10 symbols each) so
/// that the benchmark does not depend on the Louvain clustering time.
///
/// Run with:
///   cargo run --example benchmark -p theo-code-governance --release
use std::time::Instant;

use theo_governance::impact::analyze_impact;
use theo_engine_graph::{
    cluster::Community,
    cochange::{temporal_decay, update_cochanges, DEFAULT_LAMBDA},
    model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const RUNS: usize = 5;
const IMPACT_SIZES: &[usize] = &[100, 500, 1_000, 5_000];

// ---------------------------------------------------------------------------
// Synthetic graph + community generator
// ---------------------------------------------------------------------------

/// Build a graph and corresponding community list without invoking Louvain.
/// One community is created per file (10 symbols per file/community).
fn generate_graph_and_communities(num_symbols: usize, calls_per_node: usize) -> (CodeGraph, Vec<Community>) {
    let mut graph = CodeGraph::new();
    let num_files = (num_symbols / 10).max(1);

    // File nodes.
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
            last_modified: fi as f64 * 86400.0,
            doc: None,
        });
    }

    // Symbol nodes + CONTAINS edges.
    for si in 0..num_symbols {
        let fi = si / 10;
        graph.add_node(Node {
            id: format!("sym-{si}"),
            node_type: NodeType::Symbol,
            name: format!("fn_{si}"),
            file_path: Some(format!("src/module_{fi}.rs")),
            signature: Some(format!("pub fn fn_{si}() -> u32")),
            kind: Some(SymbolKind::Function),
            line_start: Some(si * 5 + 1),
            line_end: Some(si * 5 + 4),
            last_modified: fi as f64 * 86400.0,
            doc: None,
        });
        graph.add_edge(Edge {
            source: format!("file-{fi}"),
            target: format!("sym-{si}"),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
    }

    // CALLS edges (deterministic LCG).
    let calls_per_node = calls_per_node.min(num_symbols.saturating_sub(1));
    let mut lcg: u64 = 1234; // separate seed from construction
    let lcg_next = |s: &mut u64| -> usize {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*s >> 33) as usize
    };
    for si in 0..num_symbols {
        for _ in 0..calls_per_node {
            let target = lcg_next(&mut lcg) % num_symbols;
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

    // CO_CHANGES edges between consecutive files.
    for fi in 0..num_files.saturating_sub(1) {
        graph.add_edge(Edge {
            source: format!("file-{fi}"),
            target: format!("file-{}", fi + 1),
            edge_type: EdgeType::CoChanges,
            weight: temporal_decay(1.0, DEFAULT_LAMBDA),
        });
    }

    // Build communities synthetically: one per file.
    let communities: Vec<Community> = (0..num_files)
        .map(|fi| {
            let node_ids: Vec<String> = (0..10)
                .map(|j| format!("sym-{}", fi * 10 + j))
                .filter(|id| {
                    let idx: usize = id.trim_start_matches("sym-").parse().unwrap_or(usize::MAX);
                    idx < num_symbols
                })
                .collect();
            Community {
                id: format!("comm-{fi}"),
                name: format!("Community {fi}"),
                level: 0,
                node_ids,
                parent_id: None,
                version: 1,
            }
        })
        .collect();

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
// Benchmark 1: BFS impact analysis at scale
// ---------------------------------------------------------------------------

fn bench_impact_analysis() {
    println!("\nBENCHMARK: impact_analysis (BFS from edited file, max_depth=3)");
    for &n in IMPACT_SIZES {
        let (graph, communities) = generate_graph_and_communities(n, 2);
        let edited_file = "src/module_0.rs";

        let (mean, min, max) = time_fn(|| {
            let report = analyze_impact(edited_file, &graph, &communities, 3);
            std::hint::black_box(&report);
        });

        let report = analyze_impact(edited_file, &graph, &communities, 3);
        println!(
            "  N={n:<6}: time_ms(mean={:.3}, min={:.3}, max={:.3})  affected_communities={}  alerts={}  co_change_candidates={}",
            mean, min, max,
            report.affected_communities.len(),
            report.risk_alerts.len(),
            report.co_change_candidates.len(),
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark 2: BFS depth sensitivity (N=1000)
// ---------------------------------------------------------------------------

fn bench_impact_depth() {
    println!("\nBENCHMARK: impact_analysis_depth_sensitivity (N=1000, varying max_depth)");
    let n = 1_000;
    let (graph, communities) = generate_graph_and_communities(n, 2);
    let edited_file = "src/module_0.rs";

    for max_depth in [0usize, 1, 2, 3, 5] {
        let (mean, min, max_t) = time_fn(|| {
            let report = analyze_impact(edited_file, &graph, &communities, max_depth);
            std::hint::black_box(&report);
        });
        let report = analyze_impact(edited_file, &graph, &communities, max_depth);
        println!(
            "  depth={max_depth}: time_ms(mean={:.3}, min={:.3}, max={:.3})  affected_communities={}",
            mean, min, max_t,
            report.affected_communities.len(),
        );
    }
}

// ---------------------------------------------------------------------------
// Benchmark 3: Co-change update at scale
// ---------------------------------------------------------------------------

fn bench_cochange_at_scale() {
    println!("\nBENCHMARK: cochange_update (N=1000, varying commit size)");
    let n = 1_000;
    let (base_graph, _) = generate_graph_and_communities(n, 2);

    for &changed_count in &[2usize, 5, 10, 50] {
        let changed_files: Vec<String> = (0..changed_count)
            .map(|i| format!("file-{}", i % (n / 10)))
            .collect();

        let (mean, min, max) = time_fn(|| {
            let mut g = base_graph.clone();
            update_cochanges(&mut g, &changed_files, 0.0);
        });

        let mut g = base_graph.clone();
        update_cochanges(&mut g, &changed_files, 0.0);
        let total_cochange = g.edges_of_type(&EdgeType::CoChanges).len();

        println!(
            "  changed_files={changed_count:<3}: time_ms(mean={:.3}, min={:.3}, max={:.3})  total_cochange_edges={}",
            mean, min, max, total_cochange,
        );
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    println!("=== theo-code-governance benchmarks ===");
    println!("Runs per measurement: {RUNS}");
    println!("Build: release");

    bench_impact_analysis();
    bench_impact_depth();
    bench_cochange_at_scale();

    println!("\nDone.");
}
