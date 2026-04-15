/// Integration tests for session quality metrics.
use theo_governance::metrics::compute_session_metrics;

fn s(v: &str) -> String {
    v.to_string()
}

#[test]
fn test_all_files_hit_rate_is_one() {
    let context = vec![s("a.py"), s("b.py"), s("c.py")];
    let accessed = vec![s("a.py"), s("b.py"), s("c.py")];
    let m = compute_session_metrics(&context, &accessed, 3, &[s("c1"), s("c2"), s("c3")]);
    assert!(
        (m.context_hit_rate - 1.0).abs() < 1e-9,
        "expected hit_rate=1.0, got {}",
        m.context_hit_rate
    );
    assert!(
        (m.context_miss_rate - 0.0).abs() < 1e-9,
        "expected miss_rate=0.0, got {}",
        m.context_miss_rate
    );
}

#[test]
fn test_no_files_hit_rate_is_zero() {
    let context = vec![s("a.py"), s("b.py")];
    let accessed = vec![s("x.py"), s("y.py")];
    let m = compute_session_metrics(&context, &accessed, 2, &[s("c1")]);
    assert!(
        (m.context_hit_rate - 0.0).abs() < 1e-9,
        "expected hit_rate=0.0, got {}",
        m.context_hit_rate
    );
    assert!(
        (m.context_miss_rate - 1.0).abs() < 1e-9,
        "expected miss_rate=1.0, got {}",
        m.context_miss_rate
    );
}

#[test]
fn test_half_files_hit_rate_is_half() {
    let context = vec![s("a.py"), s("b.py")];
    let accessed = vec![s("a.py"), s("z.py")];
    let m = compute_session_metrics(&context, &accessed, 2, &[]);
    assert!(
        (m.context_hit_rate - 0.5).abs() < 1e-9,
        "expected hit_rate=0.5, got {}",
        m.context_hit_rate
    );
}

#[test]
fn test_over_read_ratio_when_llm_reads_more() {
    // 2 files in context, LLM accessed 4 files
    let context = vec![s("a.py"), s("b.py")];
    let accessed = vec![s("a.py"), s("b.py"), s("c.py"), s("d.py")];
    let m = compute_session_metrics(&context, &accessed, 4, &[]);
    // over_read_ratio = actual_files / files_in_context = 4 / 2 = 2.0
    assert!(
        (m.over_read_ratio - 2.0).abs() < 1e-9,
        "expected over_read_ratio=2.0, got {}",
        m.over_read_ratio
    );
}

#[test]
fn test_over_read_ratio_equal_context() {
    let context = vec![s("a.py"), s("b.py")];
    let accessed = vec![s("a.py"), s("b.py")];
    let m = compute_session_metrics(&context, &accessed, 2, &[]);
    assert!(
        (m.over_read_ratio - 1.0).abs() < 1e-9,
        "expected over_read_ratio=1.0, got {}",
        m.over_read_ratio
    );
}

#[test]
fn test_cluster_coverage() {
    // 2 out of 4 communities touched
    let context: Vec<String> = vec![];
    let accessed: Vec<String> = vec![];
    let m = compute_session_metrics(&context, &accessed, 4, &[s("c1"), s("c2")]);
    assert!(
        (m.cluster_coverage - 0.5).abs() < 1e-9,
        "expected cluster_coverage=0.5, got {}",
        m.cluster_coverage
    );
}

#[test]
fn test_cluster_coverage_zero_total() {
    // Avoid division by zero
    let context: Vec<String> = vec![];
    let accessed: Vec<String> = vec![];
    let m = compute_session_metrics(&context, &accessed, 0, &[]);
    assert!(
        (m.cluster_coverage - 0.0).abs() < 1e-9,
        "expected cluster_coverage=0.0 when total=0, got {}",
        m.cluster_coverage
    );
}

#[test]
fn test_hit_miss_sum_to_one() {
    let context = vec![s("a.py"), s("b.py"), s("c.py")];
    let accessed = vec![s("a.py"), s("x.py")];
    let m = compute_session_metrics(&context, &accessed, 3, &[]);
    assert!(
        (m.context_hit_rate + m.context_miss_rate - 1.0).abs() < 1e-9,
        "hit_rate + miss_rate must equal 1.0, got {} + {}",
        m.context_hit_rate,
        m.context_miss_rate
    );
}

#[test]
fn test_empty_context_no_panic() {
    let context: Vec<String> = vec![];
    let accessed: Vec<String> = vec![s("x.py")];
    let m = compute_session_metrics(&context, &accessed, 1, &[]);
    // With empty context, hit rate is 0 (no context files to hit)
    assert!(m.context_hit_rate >= 0.0);
    assert!(m.context_hit_rate <= 1.0);
}
