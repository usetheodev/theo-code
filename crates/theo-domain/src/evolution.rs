//! Evolution loop domain types — structured retry with reflection.
//!
//! Provides pure data types for tracking attempt history and generating
//! structured reflections between retry attempts. The runtime `EvolutionLoop`
//! in `theo-agent-runtime` consumes these types.

use serde::{Deserialize, Serialize};

use crate::retry_policy::CorrectionStrategy;

/// Hard cap for evolution loop attempts. Invariant: never exceeded.
pub const MAX_EVOLUTION_ATTEMPTS: u32 = 5;

/// Outcome of a single attempt within an evolution loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttemptOutcome {
    Success,
    Failure,
    Partial,
}

impl std::fmt::Display for AttemptOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "Success"),
            Self::Failure => write!(f, "Failure"),
            Self::Partial => write!(f, "Partial"),
        }
    }
}

/// Record of a single attempt within an evolution loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptRecord {
    /// 1-indexed attempt number.
    pub attempt_number: u32,
    /// Strategy used for this attempt.
    pub strategy_used: CorrectionStrategy,
    /// Outcome of the attempt.
    pub outcome: AttemptOutcome,
    /// Files edited during this attempt.
    pub files_edited: Vec<String>,
    /// Summary of the error (if failed or partial).
    pub error_summary: Option<String>,
    /// Duration of the attempt in milliseconds.
    pub duration_ms: u64,
    /// Tokens consumed during this attempt.
    pub tokens_used: u64,
}

/// Structured reflection between attempts.
///
/// Generated after a failed/partial attempt to guide the next attempt
/// with a revised strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    /// Which attempt this reflection is about.
    pub prior_attempt: u32,
    /// What went wrong.
    pub what_failed: String,
    /// Root cause analysis.
    pub why_it_failed: String,
    /// What to change in the next attempt.
    pub what_to_change: String,
    /// Recommended strategy for the next attempt.
    pub recommended_strategy: CorrectionStrategy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_outcome_display() {
        assert_eq!(format!("{}", AttemptOutcome::Success), "Success");
        assert_eq!(format!("{}", AttemptOutcome::Failure), "Failure");
        assert_eq!(format!("{}", AttemptOutcome::Partial), "Partial");
    }

    #[test]
    fn attempt_record_serde_roundtrip() {
        let record = AttemptRecord {
            attempt_number: 1,
            strategy_used: CorrectionStrategy::RetryLocal,
            outcome: AttemptOutcome::Failure,
            files_edited: vec!["src/main.rs".into()],
            error_summary: Some("type mismatch".into()),
            duration_ms: 5000,
            tokens_used: 10000,
        };
        let json = serde_json::to_string(&record).unwrap();
        let back: AttemptRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.attempt_number, 1);
        assert_eq!(back.outcome, AttemptOutcome::Failure);
        assert_eq!(back.error_summary.as_deref(), Some("type mismatch"));
    }

    #[test]
    fn reflection_serde_roundtrip() {
        let reflection = Reflection {
            prior_attempt: 2,
            what_failed: "Edit did not fix the root cause".into(),
            why_it_failed: "Edited wrong function".into(),
            what_to_change: "Target validate_input instead of parse_input".into(),
            recommended_strategy: CorrectionStrategy::Replan,
        };
        let json = serde_json::to_string(&reflection).unwrap();
        let back: Reflection = serde_json::from_str(&json).unwrap();
        assert_eq!(back.prior_attempt, 2);
        assert_eq!(back.recommended_strategy, CorrectionStrategy::Replan);
    }

    #[test]
    fn max_evolution_attempts_is_five() {
        assert_eq!(MAX_EVOLUTION_ATTEMPTS, 5);
    }
}
