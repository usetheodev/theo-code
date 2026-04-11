/// Risk alert system for GRAPHCTX governance.
use crate::impact::ImpactReport;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Severity level for a risk alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskLevel {
    Info,
    Warning,
    Critical,
}

/// A structured risk alert produced by the governance layer.
#[derive(Debug, Clone)]
pub struct RiskAlert {
    pub level: RiskLevel,
    pub message: String,
    /// Community ID, if the alert is scoped to a specific community.
    pub community_id: Option<String>,
    /// File path, if the alert is scoped to a specific file.
    pub file_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Generate structured `RiskAlert`s from an `ImpactReport`.
///
/// Rules:
/// - 3+ affected communities       → **Critical** cross-cluster alert
/// - Untested modification alerts  → **Warning** (forwarded from `report.risk_alerts`)
/// - Co-change candidates          → **Info** alert per candidate
pub fn generate_alerts(report: &ImpactReport) -> Vec<RiskAlert> {
    let mut alerts: Vec<RiskAlert> = Vec::new();

    // Critical: 3 or more communities affected simultaneously.
    if report.affected_communities.len() >= 3 {
        alerts.push(RiskAlert {
            level: RiskLevel::Critical,
            message: format!(
                "Cross-cluster impact: edit to '{}' affects {} communities: {}",
                report.edited_file,
                report.affected_communities.len(),
                report.affected_communities.join(", ")
            ),
            community_id: None,
            file_path: Some(report.edited_file.clone()),
        });
    }

    // Warning: propagate "Untested modification" strings from risk_alerts.
    for alert_str in &report.risk_alerts {
        if alert_str.contains("Untested") || alert_str.contains("untested") {
            alerts.push(RiskAlert {
                level: RiskLevel::Warning,
                message: alert_str.clone(),
                community_id: None,
                file_path: Some(report.edited_file.clone()),
            });
        }
    }

    // Info: one alert per co-change candidate.
    for candidate in &report.co_change_candidates {
        alerts.push(RiskAlert {
            level: RiskLevel::Info,
            message: format!(
                "Co-change alert: '{}' historically co-changes with '{}'",
                candidate, report.edited_file
            ),
            community_id: None,
            file_path: Some(candidate.clone()),
        });
    }

    alerts
}

/// Check which edited symbols have no test coverage and return `Warning` alerts.
///
/// A symbol is considered covered if `test_coverage` is non-empty (the list of
/// test node IDs that exercise it).  When `test_coverage` is empty but
/// `edited_symbols` is non-empty, every symbol gets a Warning.
pub fn check_untested_modifications(
    edited_symbols: &[String],
    test_coverage: &[String],
) -> Vec<RiskAlert> {
    if edited_symbols.is_empty() {
        return vec![];
    }

    // If there is any test coverage, consider all symbols covered.
    if !test_coverage.is_empty() {
        return vec![];
    }

    // No coverage at all — one Warning per symbol.
    edited_symbols
        .iter()
        .map(|sym| RiskAlert {
            level: RiskLevel::Warning,
            message: format!("Untested modification: {sym} has no test coverage"),
            community_id: None,
            file_path: None,
        })
        .collect()
}
