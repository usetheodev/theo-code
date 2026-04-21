//! `theo memory lint` CLI subcommand — adapter around
//! `theo_application::use_cases::memory_lint`.
//!
//! Per ADR-004, apps never import engine/infra crates directly; the
//! re-export lives in `theo-application`.
//!
//! Plan: `outputs/agent-memory-plan.md` §"Health monitoring — theo memory lint".

use theo_application::use_cases::memory_lint::{
    LintInputs, LintThresholds, Severity, render_json, run_lint,
};

/// Output format for the lint results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintFormat {
    Text,
    Json,
}

impl LintFormat {
    pub fn from_str_opt(s: Option<&str>) -> Self {
        match s.map(str::to_ascii_lowercase).as_deref() {
            Some("json") => Self::Json,
            _ => Self::Text,
        }
    }
}

/// Entry point. Caller pre-collects `inputs` so measurement strategy
/// stays out of the core. Returns the CLI's exit code.
pub fn run(inputs: LintInputs, format: LintFormat) -> i32 {
    let thresholds = LintThresholds::default();
    let issues = run_lint(&inputs, &thresholds);
    match format {
        LintFormat::Json => {
            println!("{}", render_json(&issues));
        }
        LintFormat::Text => {
            if issues.is_empty() {
                println!("memory lint: OK (no issues)");
            } else {
                for i in &issues {
                    println!(
                        "{}  {}  {}",
                        severity_tag(i.severity),
                        i.metric,
                        i.message
                    );
                }
            }
        }
    }
    if issues.iter().any(|i| i.severity == Severity::Critical) {
        2
    } else if issues.iter().any(|i| i.severity == Severity::Warning) {
        1
    } else {
        0
    }
}

fn severity_tag(s: Severity) -> &'static str {
    match s {
        Severity::Info => "[info]    ",
        Severity::Concern => "[concern] ",
        Severity::Warning => "[warning] ",
        Severity::Critical => "[crit]    ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_application::use_cases::memory_lint::LessonMetric;

    fn inputs_with_p50(ms: f32) -> LintInputs {
        LintInputs {
            seconds_since_last_compile: 0,
            lessons: Vec::new(),
            orphan_episode_ids: Vec::new(),
            broken_link_pages: Vec::new(),
            recall_p50_ms: ms,
            recall_p95_ms: 100.0,
        }
    }

    #[test]
    fn format_from_str_maps_json_and_defaults_to_text() {
        assert_eq!(LintFormat::from_str_opt(Some("json")), LintFormat::Json);
        assert_eq!(LintFormat::from_str_opt(Some("JSON")), LintFormat::Json);
        assert_eq!(LintFormat::from_str_opt(Some("text")), LintFormat::Text);
        assert_eq!(LintFormat::from_str_opt(None), LintFormat::Text);
    }

    #[test]
    fn healthy_input_exits_zero() {
        let code = run(inputs_with_p50(100.0), LintFormat::Text);
        assert_eq!(code, 0);
    }

    #[test]
    fn warning_input_exits_one() {
        let code = run(inputs_with_p50(600.0), LintFormat::Text);
        assert_eq!(code, 1);
    }

    #[test]
    fn critical_input_exits_two() {
        let mut i = inputs_with_p50(100.0);
        i.recall_p95_ms = 3000.0;
        let code = run(i, LintFormat::Text);
        assert_eq!(code, 2);
    }

    #[test]
    fn concern_only_still_exits_zero() {
        let mut i = inputs_with_p50(100.0);
        i.lessons.push(LessonMetric {
            id: "l".into(),
            age_seconds: 60 * 86_400,
            hit_count: 0,
        });
        let code = run(i, LintFormat::Text);
        assert_eq!(code, 0, "concern alone does not gate CI");
    }
}
