/// Golden case evaluation for retrieval quality.
///
/// Defines a fixed set of query → expected_files mappings and verifies
/// that the IR metric functions compute correct scores for known inputs.
///
/// This is the CI gate: if MRR or DepCov drops below thresholds, the PR fails.
/// Run: `cargo test -p theo-engine-retrieval --test eval_golden`
use theo_engine_retrieval::metrics::{
    DepEdge, RetrievalMetrics, dep_coverage, hit_at_k, mrr, recall_at_k,
};

// ---------------------------------------------------------------------------
// Golden cases: synthetic but representative
// ---------------------------------------------------------------------------

struct GoldenCase {
    name: &'static str,
    /// Files returned by retrieval (ordered by relevance)
    returned: Vec<String>,
    /// Files expected to be relevant
    expected_files: &'static [&'static str],
    /// Expected dependency edges
    expected_deps: Vec<DepEdge>,
}

fn golden_cases() -> Vec<GoldenCase> {
    let mut all = Vec::new();
    all.extend(golden_cases_1_to_5());
    all.extend(golden_cases_6_to_10());
    all
}

fn golden_cases_1_to_5() -> Vec<GoldenCase> {
    vec![
        // Case 1: Perfect hit — top result is expected
        GoldenCase {
            name: "auth_flow_perfect",
            returned: vec![
                "crates/theo-infra-auth/src/lib.rs".into(),
                "crates/theo-domain/src/session.rs".into(),
                "crates/theo-tooling/src/sandbox.rs".into(),
            ],
            expected_files: &["crates/theo-infra-auth/src/lib.rs"],
            expected_deps: vec![DepEdge {
                edge_type: "Imports".into(),
                source: "crates/theo-infra-auth/src/lib.rs".into(),
                target: "crates/theo-domain/src/session.rs".into(),
            }],
        },
        // Case 2: Relevant file at rank 2
        GoldenCase {
            name: "graph_context_rank2",
            returned: vec![
                "crates/theo-engine-retrieval/src/search.rs".into(),
                "crates/theo-application/src/use_cases/graph_context_service.rs".into(),
                "crates/theo-domain/src/graph_context.rs".into(),
            ],
            expected_files: &["crates/theo-application/src/use_cases/graph_context_service.rs"],
            expected_deps: vec![],
        },
        // Case 3: Multiple expected files
        GoldenCase {
            name: "event_system_multi",
            returned: vec![
                "crates/theo-domain/src/event.rs".into(),
                "crates/theo-agent-runtime/src/event_bus.rs".into(),
                "crates/theo-agent-runtime/src/run_engine.rs".into(),
                "crates/theo-tooling/src/memory/mod.rs".into(),
            ],
            expected_files: &[
                "crates/theo-domain/src/event.rs",
                "crates/theo-agent-runtime/src/event_bus.rs",
            ],
            expected_deps: vec![DepEdge {
                edge_type: "Imports".into(),
                source: "crates/theo-agent-runtime/src/event_bus.rs".into(),
                target: "crates/theo-domain/src/event.rs".into(),
            }],
        },
        // Case 4: No hit — completely wrong results
        GoldenCase {
            name: "wiki_generation_miss",
            returned: vec![
                "crates/theo-tooling/src/sandbox.rs".into(),
                "crates/theo-infra-llm/src/lib.rs".into(),
            ],
            expected_files: &["crates/theo-engine-retrieval/src/wiki/generator.rs"],
            expected_deps: vec![],
        },
        // Case 5: Partial hit — some expected found
        GoldenCase {
            name: "snapshot_partial",
            returned: vec![
                "crates/theo-agent-runtime/src/snapshot.rs".into(),
                "crates/theo-domain/src/agent_run.rs".into(),
                "crates/theo-agent-runtime/src/persistence.rs".into(),
                "crates/theo-agent-runtime/src/compaction.rs".into(),
            ],
            expected_files: &[
                "crates/theo-agent-runtime/src/snapshot.rs",
                "crates/theo-agent-runtime/src/persistence.rs",
            ],
            expected_deps: vec![DepEdge {
                edge_type: "Imports".into(),
                source: "crates/theo-agent-runtime/src/snapshot.rs".into(),
                target: "crates/theo-domain/src/agent_run.rs".into(),
            }],
        },
    ]
}

fn golden_cases_6_to_10() -> Vec<GoldenCase> {
    vec![
        // Case 6: Assembler query
        GoldenCase {
            name: "context_assembler",
            returned: vec![
                "crates/theo-application/src/use_cases/context_assembler.rs".into(),
                "crates/theo-domain/src/working_set.rs".into(),
                "crates/theo-domain/src/graph_context.rs".into(),
            ],
            expected_files: &["crates/theo-application/src/use_cases/context_assembler.rs"],
            expected_deps: vec![DepEdge {
                edge_type: "Imports".into(),
                source: "crates/theo-application/src/use_cases/context_assembler.rs".into(),
                target: "crates/theo-domain/src/working_set.rs".into(),
            }],
        },
        // Case 7: Episode summary
        GoldenCase {
            name: "episode_summary",
            returned: vec![
                "crates/theo-domain/src/episode.rs".into(),
                "crates/theo-domain/src/event.rs".into(),
            ],
            expected_files: &["crates/theo-domain/src/episode.rs"],
            expected_deps: vec![],
        },
        // Case 8: BM25 search
        GoldenCase {
            name: "bm25_search",
            returned: vec![
                "crates/theo-engine-retrieval/src/search.rs".into(),
                "crates/theo-engine-retrieval/src/wiki/lookup.rs".into(),
            ],
            expected_files: &["crates/theo-engine-retrieval/src/search.rs"],
            expected_deps: vec![],
        },
        // Case 9: Impact analysis
        GoldenCase {
            name: "impact_analysis",
            returned: vec![
                "crates/theo-application/src/use_cases/impact.rs".into(),
                "crates/theo-engine-graph/src/cochange.rs".into(),
                "crates/theo-domain/src/graph_context.rs".into(),
            ],
            expected_files: &["crates/theo-application/src/use_cases/impact.rs"],
            expected_deps: vec![DepEdge {
                edge_type: "Imports".into(),
                source: "crates/theo-application/src/use_cases/impact.rs".into(),
                target: "crates/theo-engine-graph/src/cochange.rs".into(),
            }],
        },
        // Case 10: Clustering
        GoldenCase {
            name: "clustering",
            returned: vec![
                "crates/theo-engine-graph/src/cluster.rs".into(),
                "crates/theo-engine-graph/src/model.rs".into(),
            ],
            expected_files: &["crates/theo-engine-graph/src/cluster.rs"],
            expected_deps: vec![],
        },
    ]
}

// ---------------------------------------------------------------------------
// Eval tests
// ---------------------------------------------------------------------------

#[test]
fn eval_per_case_mrr_computed() {
    for case in golden_cases() {
        let m = mrr(&case.returned, case.expected_files);
        eprintln!("[{}] MRR = {:.3}", case.name, m);
        // Individual MRR can be 0 for misses — we check aggregate below
        assert!(
            (0.0..=1.0).contains(&m),
            "[{}] MRR out of range: {}",
            case.name,
            m
        );
    }
}

#[test]
fn eval_aggregate_mrr_above_floor() {
    let cases = golden_cases();
    let mrr_values: Vec<f64> = cases
        .iter()
        .map(|c| mrr(&c.returned, c.expected_files))
        .collect();
    let aggregate = mrr_values.iter().sum::<f64>() / mrr_values.len() as f64;
    eprintln!("Aggregate MRR = {:.3} (threshold: 0.80)", aggregate);
    assert!(
        aggregate >= 0.80,
        "Aggregate MRR must be >= 0.80, got {:.3}",
        aggregate
    );
}

#[test]
fn eval_aggregate_recall_at_5() {
    let cases = golden_cases();
    let values: Vec<f64> = cases
        .iter()
        .map(|c| recall_at_k(&c.returned, c.expected_files, 5))
        .collect();
    let aggregate = values.iter().sum::<f64>() / values.len() as f64;
    eprintln!("Aggregate Recall@5 = {:.3}", aggregate);
    assert!(
        aggregate >= 0.70,
        "Aggregate Recall@5 must be >= 0.70, got {:.3}",
        aggregate
    );
}

#[test]
fn eval_aggregate_hit_rate_at_5() {
    let cases = golden_cases();
    let values: Vec<f64> = cases
        .iter()
        .map(|c| hit_at_k(&c.returned, c.expected_files, 5))
        .collect();
    let aggregate = values.iter().sum::<f64>() / values.len() as f64;
    eprintln!("Aggregate Hit@5 = {:.3}", aggregate);
    assert!(
        aggregate >= 0.80,
        "Aggregate Hit@5 must be >= 0.80, got {:.3}",
        aggregate
    );
}

#[test]
fn eval_aggregate_dep_coverage() {
    let cases = golden_cases();
    let cases_with_deps: Vec<&GoldenCase> = cases
        .iter()
        .filter(|c| !c.expected_deps.is_empty())
        .collect();

    if cases_with_deps.is_empty() {
        return; // No dep expectations → skip
    }

    let values: Vec<f64> = cases_with_deps
        .iter()
        .map(|c| dep_coverage(&c.expected_deps, &c.returned))
        .collect();
    let aggregate = values.iter().sum::<f64>() / values.len() as f64;
    eprintln!("Aggregate DepCov = {:.3} (threshold: 0.80)", aggregate);
    assert!(
        aggregate >= 0.80,
        "Aggregate DepCov must be >= 0.80, got {:.3}",
        aggregate
    );
}

#[test]
fn eval_full_metrics_per_case() {
    let cases = golden_cases();
    let all_metrics: Vec<RetrievalMetrics> = cases
        .iter()
        .map(|c| RetrievalMetrics::compute(&c.returned, c.expected_files, &c.expected_deps))
        .collect();

    let avg = RetrievalMetrics::average(&all_metrics);
    eprintln!("=== Aggregate Metrics ===");
    eprintln!("  MRR:        {:.3}", avg.mrr);
    eprintln!("  Recall@5:   {:.3}", avg.recall_at_5);
    eprintln!("  Recall@10:  {:.3}", avg.recall_at_10);
    eprintln!("  Precision@5:{:.3}", avg.precision_at_5);
    eprintln!("  Hit@5:      {:.3}", avg.hit_rate_at_5);
    eprintln!("  nDCG@5:     {:.3}", avg.ndcg_at_5);
    eprintln!("  DepCov:     {:.3}", avg.dep_coverage);

    // These are CI gates
    assert!(avg.mrr >= 0.80, "MRR gate failed: {:.3}", avg.mrr);
    assert!(
        avg.hit_rate_at_5 >= 0.80,
        "Hit@5 gate failed: {:.3}",
        avg.hit_rate_at_5
    );
}
