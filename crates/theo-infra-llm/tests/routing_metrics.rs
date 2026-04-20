//! R0 acceptance tests for the routing-benchmark harness.
//! Plan: `outputs/smart-model-routing-plan.md` §2 R0 table.

use std::path::PathBuf;

use theo_infra_llm::routing::metrics::{RoutingReport, Tier, load_cases, run_cases};

fn fixture_dir() -> PathBuf {
    // tests/ sits beside src/ inside the crate; go up two levels to the
    // workspace root and into .theo/fixtures/routing.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(".theo")
        .join("fixtures")
        .join("routing")
}

// ── R0-AC-1 ──────────────────────────────────────────────────────────

#[test]
fn test_r0_ac_1_fixture_dir_contains_thirty_labelled_cases() {
    let cases = load_cases(&fixture_dir()).expect("fixture dir must load");
    assert_eq!(cases.len(), 30, "expected exactly 30 labelled cases");
    let labels: std::collections::BTreeMap<&str, usize> =
        cases.iter().fold(Default::default(), |mut acc, c| {
            *acc.entry(c.label.as_str()).or_insert(0) += 1;
            acc
        });
    assert_eq!(labels.get("simple"), Some(&10), "10 simple cases");
    assert_eq!(labels.get("medium"), Some(&10), "10 medium cases");
    assert_eq!(labels.get("complex"), Some(&10), "10 complex cases");
}

// ── R0-AC-2 ──────────────────────────────────────────────────────────

#[test]
fn test_r0_ac_2_report_contains_required_metrics() {
    let cases = load_cases(&fixture_dir()).unwrap();
    // NullRouter stand-in: always pick the Default tier.
    let report = run_cases(&cases, |_| Tier::Default);
    assert_eq!(report.cases_total, 30);
    assert!(
        report.avg_cost_per_task > 0.0,
        "avg_cost_per_task must be > 0"
    );
    assert!(
        (0.0..=1.0).contains(&report.task_success_rate),
        "task_success_rate must be a ratio"
    );
    // p50 can legitimately be 0 µs for a fast stand-in — just assert the
    // field exists and is not obviously absurd.
    assert!(report.p50_turn_latency_us < 1_000_000, "p50 < 1s");
}

// ── R0-AC-3 ──────────────────────────────────────────────────────────
// CLI flag `--router rules` deferred to R2. Here we verify the harness
// accepts a router stand-in via a closure, which is the Rust-native
// equivalent of the deferred CLI plumbing.

#[test]
fn test_r0_ac_3_harness_accepts_pluggable_router_stand_in() {
    let cases = load_cases(&fixture_dir()).unwrap();
    let always_cheap = run_cases(&cases, |_| Tier::Cheap);
    let always_strong = run_cases(&cases, |_| Tier::Strong);
    // Different routers produce different reports — proves the closure
    // really drives the outcome.
    assert_ne!(always_cheap.avg_cost_per_task, always_strong.avg_cost_per_task);
    // Strong always passes (tier >= any required tier); cheap fails on
    // medium + complex cases by the success heuristic (see metrics.rs).
    assert_eq!(always_strong.task_success_rate, 1.0);
    assert!(always_cheap.task_success_rate < 1.0);
}

// ── R0-AC-4 ──────────────────────────────────────────────────────────

#[test]
fn test_r0_ac_4_report_is_machine_readable_json() {
    let cases = load_cases(&fixture_dir()).unwrap();
    let report = run_cases(&cases, |_| Tier::Default);
    let json = serde_json::to_string(&report).expect("must serialise to JSON");
    // Round-trip proves it's real JSON, not a Display impl.
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("round-trip");
    assert!(parsed["cases_total"].as_u64().unwrap_or(0) == 30);
    assert!(parsed["avg_cost_per_task"].is_number());
    assert!(parsed["task_success_rate"].is_number());
    assert!(parsed["p50_turn_latency_us"].is_number());
    assert!(parsed["per_label"].is_array());
    // Pretty-printed variant must also parse — downstream CI diff tooling
    // typically runs `jq . report.json`.
    let pretty = serde_json::to_string_pretty(&report).unwrap();
    let _: RoutingReport = serde_json::from_str(&pretty).unwrap();
}
