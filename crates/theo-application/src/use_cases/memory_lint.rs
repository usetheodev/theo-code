//! Memory-lint use case — re-exports the infra surface so apps never
//! import `theo-infra-memory` directly (per ADR-004 bounded-context
//! rules).
//!
//! Keeping a 3-line re-export here satisfies the dependency rule AND
//! lets us add a future wrapper (caching, telemetry) without touching
//! call sites.

pub use theo_infra_memory::{
    LessonMetric, LintInputs, LintIssue, LintThresholds, Severity, render_json, run_lint,
};
