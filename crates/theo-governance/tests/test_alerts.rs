/// Integration tests for the risk alert system.

use theo_governance::alerts::{check_untested_modifications, generate_alerts, RiskLevel};
use theo_governance::impact::ImpactReport;

fn s(v: &str) -> String {
    v.to_string()
}

fn make_report(
    affected_communities: Vec<&str>,
    tests_covering_edit: Vec<&str>,
    co_change_candidates: Vec<&str>,
    risk_alerts: Vec<&str>,
) -> ImpactReport {
    ImpactReport {
        edited_file: "some_file.py".to_string(),
        affected_communities: affected_communities.into_iter().map(s).collect(),
        tests_covering_edit: tests_covering_edit.into_iter().map(s).collect(),
        co_change_candidates: co_change_candidates.into_iter().map(s).collect(),
        risk_alerts: risk_alerts.into_iter().map(s).collect(),
        bfs_depth: 3,
    }
}

// ---------------------------------------------------------------------------
// generate_alerts tests
// ---------------------------------------------------------------------------

#[test]
fn test_three_plus_communities_generates_critical_alert() {
    let report = make_report(
        vec!["comm_a", "comm_b", "comm_c"],
        vec!["test_foo"],
        vec![],
        vec![],
    );
    let alerts = generate_alerts(&report);
    let has_critical = alerts.iter().any(|a| matches!(a.level, RiskLevel::Critical));
    assert!(
        has_critical,
        "Expected Critical alert for 3+ affected communities, got: {:?}",
        alerts.iter().map(|a| &a.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_untested_symbols_generates_warning_alert() {
    let report = make_report(
        vec!["comm_a"],
        vec![],              // no tests covering the edit
        vec![],
        vec!["Untested modification: login has no test coverage"],
    );
    let alerts = generate_alerts(&report);
    let has_warning = alerts.iter().any(|a| matches!(a.level, RiskLevel::Warning));
    assert!(
        has_warning,
        "Expected Warning alert for untested modification, got: {:?}",
        alerts.iter().map(|a| &a.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_co_changes_generates_info_alert() {
    let report = make_report(
        vec!["comm_a"],
        vec!["test_foo"],
        vec!["api.py"],     // co-change candidate
        vec![],
    );
    let alerts = generate_alerts(&report);
    let has_info = alerts.iter().any(|a| matches!(a.level, RiskLevel::Info));
    assert!(
        has_info,
        "Expected Info alert for co-change candidate, got: {:?}",
        alerts.iter().map(|a| &a.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_empty_impact_no_alerts() {
    let report = make_report(vec![], vec![], vec![], vec![]);
    let alerts = generate_alerts(&report);
    assert!(
        alerts.is_empty(),
        "Expected no alerts for empty impact report, got: {:?}",
        alerts.iter().map(|a| &a.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_two_communities_does_not_generate_critical() {
    // Only 2 communities affected — should be Warning at most, not Critical
    let report = make_report(vec!["comm_a", "comm_b"], vec!["test_a"], vec![], vec![]);
    let alerts = generate_alerts(&report);
    let has_critical = alerts.iter().any(|a| matches!(a.level, RiskLevel::Critical));
    assert!(
        !has_critical,
        "Should NOT have Critical alert for only 2 communities"
    );
}

// ---------------------------------------------------------------------------
// check_untested_modifications tests
// ---------------------------------------------------------------------------

#[test]
fn test_check_untested_with_no_coverage_returns_warning() {
    let edited_symbols = vec![s("login"), s("validate_token")];
    let test_coverage: Vec<String> = vec![];
    let alerts = check_untested_modifications(&edited_symbols, &test_coverage);
    assert!(
        !alerts.is_empty(),
        "Expected warning alerts for untested symbols"
    );
    assert!(
        alerts.iter().all(|a| matches!(a.level, RiskLevel::Warning)),
        "All untested alerts should be Warning level"
    );
}

#[test]
fn test_check_untested_with_full_coverage_returns_no_alerts() {
    let edited_symbols = vec![s("login")];
    let test_coverage = vec![s("test_login")];
    let alerts = check_untested_modifications(&edited_symbols, &test_coverage);
    // coverage is not per-symbol, it's an overall list — if non-empty, considered covered
    // Per spec: if affected symbols have no tests -> alert
    // Since test_coverage is non-empty, no untested alert
    assert!(
        alerts.is_empty(),
        "Expected no alerts when test coverage is present, got: {:?}",
        alerts.iter().map(|a| &a.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_check_untested_empty_symbols_no_alerts() {
    let edited_symbols: Vec<String> = vec![];
    let test_coverage: Vec<String> = vec![];
    let alerts = check_untested_modifications(&edited_symbols, &test_coverage);
    assert!(alerts.is_empty(), "No symbols = no alerts");
}

#[test]
fn test_alert_has_file_path_or_community_id() {
    // generate_alerts with co-change should carry file_path in the alert
    let report = make_report(
        vec!["comm_a"],
        vec!["test_foo"],
        vec!["db.py"],
        vec![],
    );
    let alerts = generate_alerts(&report);
    let info_alerts: Vec<_> = alerts
        .iter()
        .filter(|a| matches!(a.level, RiskLevel::Info))
        .collect();
    assert!(!info_alerts.is_empty(), "Expected at least one Info alert");
    // The co-change Info alert should mention the file path
    let has_file = info_alerts.iter().any(|a| a.file_path.is_some());
    assert!(has_file, "Info alert for co-change should have file_path set");
}
