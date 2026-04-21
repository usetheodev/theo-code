//! `theo memory lint` core logic.
//!
//! Six continuous-metric checks over an existing memory mount. Pure
//! logic — every input is an observable fact (timestamps, hit counts,
//! page links, latency samples) surfaced by the caller from the live
//! system. Zero filesystem / LLM / network use here.
//!
//! Plan: `outputs/agent-memory-plan.md` §"Health monitoring — theo memory lint".

use serde::{Deserialize, Serialize};

/// Lint severity — ordered from softest (Info) to hardest (Critical).
/// The CLI can filter by minimum severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Concern,
    Warning,
    Critical,
}

/// One finding. Stable `metric` id so dashboards can deduplicate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LintIssue {
    pub metric: String,
    pub severity: Severity,
    pub message: String,
}

/// Inputs aggregated from the live memory subsystem. Populated by the
/// CLI handler; everything here is a pure value so the core is
/// trivially testable.
#[derive(Debug, Clone)]
pub struct LintInputs {
    /// Unix seconds since the last wiki compile. Used for staleness.
    pub seconds_since_last_compile: u64,
    /// For every confirmed lesson: (id, age_seconds, hit_count).
    pub lessons: Vec<LessonMetric>,
    /// Episodes that have no lesson reference and no wiki link.
    pub orphan_episode_ids: Vec<String>,
    /// One entry per wiki page with any broken `[[]]` links.
    pub broken_link_pages: Vec<String>,
    /// p50 recall latency in milliseconds.
    pub recall_p50_ms: f32,
    /// p95 recall latency in milliseconds — surfaced when > 2×p50 target.
    pub recall_p95_ms: f32,
}

#[derive(Debug, Clone)]
pub struct LessonMetric {
    pub id: String,
    pub age_seconds: u64,
    pub hit_count: u32,
}

/// Production thresholds. Tunable per deployment; tests pin them.
#[derive(Debug, Clone)]
pub struct LintThresholds {
    pub max_staleness_seconds: u64,          // > 2h → warning
    pub zero_hit_min_age_seconds: u64,       // 30d with 0 hits → concern
    pub recall_p50_ms_ceiling: f32,          // > 500 ms → warning
    pub recall_p95_ms_ceiling: f32,          // > 2000 ms → warning
}

impl Default for LintThresholds {
    fn default() -> Self {
        Self {
            max_staleness_seconds: 2 * 60 * 60,
            zero_hit_min_age_seconds: 30 * 86_400,
            recall_p50_ms_ceiling: 500.0,
            recall_p95_ms_ceiling: 2000.0,
        }
    }
}

/// Run every check. Callers can sort / filter the returned vec by
/// severity before rendering.
pub fn run_lint(inputs: &LintInputs, thresholds: &LintThresholds) -> Vec<LintIssue> {
    let mut out = Vec::new();

    // LINT-AC-1
    if inputs.seconds_since_last_compile > thresholds.max_staleness_seconds {
        out.push(LintIssue {
            metric: "wiki.staleness".to_string(),
            severity: Severity::Warning,
            message: format!(
                "wiki not compiled for {}s (ceiling {}s)",
                inputs.seconds_since_last_compile, thresholds.max_staleness_seconds
            ),
        });
    }

    // LINT-AC-2
    for l in &inputs.lessons {
        if l.age_seconds >= thresholds.zero_hit_min_age_seconds && l.hit_count == 0 {
            out.push(LintIssue {
                metric: "lesson.zero_hit".to_string(),
                severity: Severity::Concern,
                message: format!("lesson `{}` aged {}s with zero hits", l.id, l.age_seconds),
            });
        }
    }

    // LINT-AC-3
    for id in &inputs.orphan_episode_ids {
        out.push(LintIssue {
            metric: "episode.orphan".to_string(),
            severity: Severity::Info,
            message: format!("episode `{id}` has no linked lesson or wiki page"),
        });
    }

    // LINT-AC-4
    for slug in &inputs.broken_link_pages {
        out.push(LintIssue {
            metric: "wiki.broken_link".to_string(),
            severity: Severity::Warning,
            message: format!("page `{slug}` has unresolved [[]] links"),
        });
    }

    // LINT-AC-5
    if inputs.recall_p50_ms > thresholds.recall_p50_ms_ceiling {
        out.push(LintIssue {
            metric: "retrieval.p50_latency".to_string(),
            severity: Severity::Warning,
            message: format!(
                "recall p50 {:.1}ms exceeds ceiling {:.1}ms",
                inputs.recall_p50_ms, thresholds.recall_p50_ms_ceiling
            ),
        });
    }
    if inputs.recall_p95_ms > thresholds.recall_p95_ms_ceiling {
        out.push(LintIssue {
            metric: "retrieval.p95_latency".to_string(),
            severity: Severity::Critical,
            message: format!(
                "recall p95 {:.1}ms exceeds ceiling {:.1}ms",
                inputs.recall_p95_ms, thresholds.recall_p95_ms_ceiling
            ),
        });
    }

    out
}

/// Render a JSON array of issues. Stable serialization keeps the output
/// jq-parseable and byte-identical across runs for the same input.
pub fn render_json(issues: &[LintIssue]) -> String {
    serde_json::to_string_pretty(issues).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_inputs() -> LintInputs {
        LintInputs {
            seconds_since_last_compile: 60, // 1 min ago
            lessons: Vec::new(),
            orphan_episode_ids: Vec::new(),
            broken_link_pages: Vec::new(),
            recall_p50_ms: 100.0,
            recall_p95_ms: 400.0,
        }
    }

    // ── LINT-AC-1 ─────────────────────────────────────────────────
    #[test]
    fn test_lint_ac_1_wiki_staleness_detected() {
        let mut i = healthy_inputs();
        i.seconds_since_last_compile = 3 * 60 * 60; // 3h
        let out = run_lint(&i, &LintThresholds::default());
        assert!(out.iter().any(|x| x.metric == "wiki.staleness"));
    }

    // ── LINT-AC-2 ─────────────────────────────────────────────────
    #[test]
    fn test_lint_ac_2_reflection_zero_hit_flagged() {
        let mut i = healthy_inputs();
        i.lessons.push(LessonMetric {
            id: "l-1".into(),
            age_seconds: 31 * 86_400,
            hit_count: 0,
        });
        let out = run_lint(&i, &LintThresholds::default());
        assert!(out.iter().any(|x| x.metric == "lesson.zero_hit"));
    }

    // ── LINT-AC-3 ─────────────────────────────────────────────────
    #[test]
    fn test_lint_ac_3_orphan_episode_reported() {
        let mut i = healthy_inputs();
        i.orphan_episode_ids.push("ep-77".into());
        let out = run_lint(&i, &LintThresholds::default());
        assert!(out.iter().any(
            |x| x.metric == "episode.orphan" && x.severity == Severity::Info
        ));
    }

    // ── LINT-AC-4 ─────────────────────────────────────────────────
    #[test]
    fn test_lint_ac_4_broken_link_in_wiki_page_flagged() {
        let mut i = healthy_inputs();
        i.broken_link_pages.push("page-x".into());
        let out = run_lint(&i, &LintThresholds::default());
        assert!(out.iter().any(|x| x.metric == "wiki.broken_link"));
    }

    // ── LINT-AC-5 ─────────────────────────────────────────────────
    #[test]
    fn test_lint_ac_5_recall_p50_exceeds_500ms_flagged() {
        let mut i = healthy_inputs();
        i.recall_p50_ms = 650.0;
        let out = run_lint(&i, &LintThresholds::default());
        assert!(out.iter().any(|x| x.metric == "retrieval.p50_latency"));
    }

    // ── LINT-AC-6 ─────────────────────────────────────────────────
    #[test]
    fn test_lint_ac_6_json_output_parseable_by_jq_surrogate() {
        // jq uses serde_json's grammar — if serde_json round-trips, jq
        // will too.
        let mut i = healthy_inputs();
        i.orphan_episode_ids.push("x".into());
        let out = run_lint(&i, &LintThresholds::default());
        let j = render_json(&out);
        let back: Vec<LintIssue> = serde_json::from_str(&j).unwrap();
        assert_eq!(back, out);
        // Smoke-check: the rendered JSON is a non-empty array.
        assert!(j.starts_with('['));
        assert!(j.ends_with(']'));
    }

    #[test]
    fn healthy_system_has_no_issues() {
        let out = run_lint(&healthy_inputs(), &LintThresholds::default());
        assert!(out.is_empty(), "healthy inputs produced issues: {out:?}");
    }

    #[test]
    fn p95_overage_is_critical() {
        let mut i = healthy_inputs();
        i.recall_p95_ms = 3000.0;
        let out = run_lint(&i, &LintThresholds::default());
        assert!(out.iter().any(
            |x| x.metric == "retrieval.p95_latency" && x.severity == Severity::Critical
        ));
    }

    #[test]
    fn severity_ordering_is_info_concern_warning_critical() {
        assert!(Severity::Info < Severity::Concern);
        assert!(Severity::Concern < Severity::Warning);
        assert!(Severity::Warning < Severity::Critical);
    }
}
