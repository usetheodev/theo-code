//! Evolution loop — structured retry with reflection between attempts.
//!
//! Wraps `CorrectionEngine` and `HeuristicReflector` to provide a disciplined
//! retry loop with:
//! - Attempt tracking with lineage (attempt N references attempt N-1)
//! - Structured reflection between attempts
//! - Hard cap enforcement (MAX_EVOLUTION_ATTEMPTS = 5)
//! - Evolution prompt building for LLM context

use theo_domain::evolution::{
    AttemptOutcome, AttemptRecord, Reflection, MAX_EVOLUTION_ATTEMPTS,
};
use theo_domain::retry_policy::CorrectionStrategy;

/// Orchestrates structured retry with reflection between attempts.
///
/// The evolution loop tracks attempt history, generates reflections after
/// failures, and enforces a hard cap on the number of attempts.
pub struct EvolutionLoop {
    attempts: Vec<AttemptRecord>,
    reflections: Vec<Reflection>,
    max_attempts: u32,
}

impl EvolutionLoop {
    /// Create a new evolution loop with the default cap.
    pub fn new() -> Self {
        Self {
            attempts: Vec::new(),
            reflections: Vec::new(),
            max_attempts: MAX_EVOLUTION_ATTEMPTS,
        }
    }

    /// Create with a custom cap (for testing). Cap is clamped to MAX_EVOLUTION_ATTEMPTS.
    pub fn with_max_attempts(max: u32) -> Self {
        Self {
            attempts: Vec::new(),
            reflections: Vec::new(),
            max_attempts: max.min(MAX_EVOLUTION_ATTEMPTS),
        }
    }

    /// Record the outcome of an attempt.
    ///
    /// Returns the recorded `AttemptRecord`. Does nothing if cap is reached.
    pub fn record_attempt(
        &mut self,
        strategy: CorrectionStrategy,
        outcome: AttemptOutcome,
        files_edited: Vec<String>,
        error_summary: Option<String>,
        duration_ms: u64,
        tokens_used: u64,
    ) -> Option<AttemptRecord> {
        if self.is_exhausted() {
            return None;
        }

        let record = AttemptRecord {
            attempt_number: self.current_attempt(),
            strategy_used: strategy,
            outcome,
            files_edited,
            error_summary,
            duration_ms,
            tokens_used,
        };

        self.attempts.push(record.clone());
        Some(record)
    }

    /// Generate a structured reflection based on the most recent failed attempt.
    ///
    /// Returns `None` if:
    /// - No attempts have been recorded
    /// - The last attempt was successful
    /// - The cap is reached
    pub fn reflect(&mut self) -> Option<Reflection> {
        let last = self.attempts.last()?;
        if last.outcome == AttemptOutcome::Success || self.is_exhausted() {
            return None;
        }

        let what_failed = last
            .error_summary
            .clone()
            .unwrap_or_else(|| format!("Attempt {} failed", last.attempt_number));

        let why_it_failed = self.infer_failure_reason();
        let recommended_strategy = self.recommend_next_strategy();
        let what_to_change = self.suggest_change(&recommended_strategy);

        let reflection = Reflection {
            prior_attempt: last.attempt_number,
            what_failed,
            why_it_failed,
            what_to_change,
            recommended_strategy,
        };

        self.reflections.push(reflection.clone());
        Some(reflection)
    }

    /// Check if the evolution loop has exhausted its attempts.
    pub fn is_exhausted(&self) -> bool {
        self.attempts.len() as u32 >= self.max_attempts
    }

    /// Current attempt number (1-indexed). Returns 1 if no attempts recorded.
    pub fn current_attempt(&self) -> u32 {
        self.attempts.len() as u32 + 1
    }

    /// Number of attempts recorded so far.
    pub fn attempt_count(&self) -> usize {
        self.attempts.len()
    }

    /// Get all recorded attempts.
    pub fn attempts(&self) -> &[AttemptRecord] {
        &self.attempts
    }

    /// Get all recorded reflections.
    pub fn reflections(&self) -> &[Reflection] {
        &self.reflections
    }

    /// Build an evolution context prompt for the LLM.
    ///
    /// Summarizes prior attempts and reflections so the LLM can learn from
    /// previous failures and adjust its approach.
    pub fn build_evolution_prompt(&self) -> String {
        if self.attempts.is_empty() {
            return String::new();
        }

        let mut prompt = String::from("## Evolution Context\n\n");
        prompt.push_str(&format!(
            "This is attempt {}/{}.\n\n",
            self.current_attempt(),
            self.max_attempts
        ));

        prompt.push_str("### Prior Attempts\n\n");
        for attempt in &self.attempts {
            prompt.push_str(&format!(
                "- **Attempt {}** ({}): {} | Strategy: {} | Files: {}\n",
                attempt.attempt_number,
                attempt.outcome,
                attempt
                    .error_summary
                    .as_deref()
                    .unwrap_or("no error details"),
                attempt.strategy_used,
                if attempt.files_edited.is_empty() {
                    "none".to_string()
                } else {
                    attempt.files_edited.join(", ")
                },
            ));
        }

        if !self.reflections.is_empty() {
            prompt.push_str("\n### Reflections\n\n");
            for reflection in &self.reflections {
                prompt.push_str(&format!(
                    "- After attempt {}: {} → Change: {} (use {})\n",
                    reflection.prior_attempt,
                    reflection.why_it_failed,
                    reflection.what_to_change,
                    reflection.recommended_strategy,
                ));
            }
        }

        prompt.push_str(
            "\n**IMPORTANT**: Do NOT repeat the same approach that failed. \
             Use the reflections above to guide a different strategy.\n",
        );

        prompt
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Infer why the last attempt failed based on attempt history.
    fn infer_failure_reason(&self) -> String {
        let attempt_count = self.attempts.len();
        if attempt_count <= 1 {
            return "First attempt failed — initial approach was incorrect".to_string();
        }

        // Check for repeated strategies
        let last_strategy = self.attempts.last().map(|a| a.strategy_used);
        let prev_strategy = self.attempts.get(attempt_count - 2).map(|a| a.strategy_used);
        if last_strategy == prev_strategy {
            return format!(
                "Same strategy ({}) used twice without success — approach needs fundamental change",
                last_strategy.map(|s| s.to_string()).unwrap_or_default()
            );
        }

        // Check for repeated files
        let last_files: std::collections::HashSet<_> = self
            .attempts
            .last()
            .map(|a| a.files_edited.iter().collect())
            .unwrap_or_default();
        let prev_files: std::collections::HashSet<_> = self
            .attempts
            .get(attempt_count - 2)
            .map(|a| a.files_edited.iter().collect())
            .unwrap_or_default();
        if !last_files.is_empty() && last_files == prev_files {
            return "Same files edited without progress — bug may be in a different location".to_string();
        }

        format!("Attempt {} failed after strategy change — deeper investigation needed", attempt_count)
    }

    /// Recommend a strategy for the next attempt that differs from the last.
    fn recommend_next_strategy(&self) -> CorrectionStrategy {
        let last_strategy = self
            .attempts
            .last()
            .map(|a| a.strategy_used)
            .unwrap_or(CorrectionStrategy::RetryLocal);

        // Escalation ladder: RetryLocal → Replan → Subtask → AgentSwap
        match last_strategy {
            CorrectionStrategy::RetryLocal => CorrectionStrategy::Replan,
            CorrectionStrategy::Replan => CorrectionStrategy::Subtask,
            CorrectionStrategy::Subtask => CorrectionStrategy::AgentSwap,
            CorrectionStrategy::AgentSwap => CorrectionStrategy::Replan, // cycle back
        }
    }

    /// Suggest what to change based on the recommended strategy.
    fn suggest_change(&self, strategy: &CorrectionStrategy) -> String {
        match strategy {
            CorrectionStrategy::RetryLocal => {
                "Retry with minor adjustments to the same approach".to_string()
            }
            CorrectionStrategy::Replan => {
                "Re-analyze the problem from scratch — read more context, form new hypothesis".to_string()
            }
            CorrectionStrategy::Subtask => {
                "Break the problem into smaller subtasks and solve incrementally".to_string()
            }
            CorrectionStrategy::AgentSwap => {
                "Delegate to a specialized sub-agent with different capabilities".to_string()
            }
        }
    }
}

impl Default for EvolutionLoop {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evolution_loop_caps_at_5_attempts() {
        // Arrange
        let mut evolution = EvolutionLoop::new();

        // Act: record 5 failed attempts
        for i in 0..5 {
            let result = evolution.record_attempt(
                CorrectionStrategy::RetryLocal,
                AttemptOutcome::Failure,
                vec![format!("file{i}.rs")],
                Some(format!("error {i}")),
                1000,
                5000,
            );
            assert!(result.is_some(), "Attempt {} should be recorded", i + 1);
        }

        // Assert: 6th attempt is rejected
        assert!(evolution.is_exhausted());
        let result = evolution.record_attempt(
            CorrectionStrategy::RetryLocal,
            AttemptOutcome::Failure,
            vec![],
            None,
            0,
            0,
        );
        assert!(result.is_none(), "6th attempt should be rejected");
        assert_eq!(evolution.attempt_count(), 5);
    }

    #[test]
    fn test_reflection_references_prior_attempt() {
        // Arrange
        let mut evolution = EvolutionLoop::new();
        evolution.record_attempt(
            CorrectionStrategy::RetryLocal,
            AttemptOutcome::Failure,
            vec!["src/main.rs".into()],
            Some("compilation error".into()),
            2000,
            8000,
        );

        // Act
        let reflection = evolution.reflect();

        // Assert
        assert!(reflection.is_some());
        let r = reflection.unwrap();
        assert_eq!(r.prior_attempt, 1);
        assert!(!r.what_failed.is_empty());
        assert!(!r.why_it_failed.is_empty());
        assert!(!r.what_to_change.is_empty());
    }

    #[test]
    fn test_reflection_strategy_differs_from_failed() {
        // Arrange
        let mut evolution = EvolutionLoop::new();
        evolution.record_attempt(
            CorrectionStrategy::RetryLocal,
            AttemptOutcome::Failure,
            vec![],
            Some("failed".into()),
            1000,
            5000,
        );

        // Act
        let reflection = evolution.reflect().unwrap();

        // Assert: should recommend something other than RetryLocal
        assert_ne!(
            reflection.recommended_strategy,
            CorrectionStrategy::RetryLocal,
            "Reflection should recommend a different strategy than what just failed"
        );
    }

    #[test]
    fn test_evolution_prompt_contains_attempt_history() {
        // Arrange
        let mut evolution = EvolutionLoop::new();
        evolution.record_attempt(
            CorrectionStrategy::RetryLocal,
            AttemptOutcome::Failure,
            vec!["auth.rs".into()],
            Some("type error".into()),
            1000,
            5000,
        );
        evolution.reflect();
        evolution.record_attempt(
            CorrectionStrategy::Replan,
            AttemptOutcome::Partial,
            vec!["auth.rs".into(), "config.rs".into()],
            Some("partial fix".into()),
            2000,
            10000,
        );

        // Act
        let prompt = evolution.build_evolution_prompt();

        // Assert
        assert!(prompt.contains("Attempt 1"));
        assert!(prompt.contains("Attempt 2"));
        assert!(prompt.contains("type error"));
        assert!(prompt.contains("partial fix"));
        assert!(prompt.contains("auth.rs"));
        assert!(prompt.contains("3/5")); // next attempt is 3 of 5
        assert!(prompt.contains("Do NOT repeat"));
    }

    #[test]
    fn test_attempt_lineage_traceable() {
        // Arrange
        let mut evolution = EvolutionLoop::new();

        // Act: 3 attempts with reflections
        evolution.record_attempt(
            CorrectionStrategy::RetryLocal,
            AttemptOutcome::Failure,
            vec!["a.rs".into()],
            Some("error A".into()),
            1000,
            3000,
        );
        let r1 = evolution.reflect().unwrap();

        evolution.record_attempt(
            r1.recommended_strategy,
            AttemptOutcome::Failure,
            vec!["b.rs".into()],
            Some("error B".into()),
            2000,
            4000,
        );
        let r2 = evolution.reflect().unwrap();

        evolution.record_attempt(
            r2.recommended_strategy,
            AttemptOutcome::Success,
            vec!["c.rs".into()],
            None,
            1500,
            3500,
        );

        // Assert: lineage is traceable
        let attempts = evolution.attempts();
        assert_eq!(attempts.len(), 3);
        assert_eq!(attempts[0].attempt_number, 1);
        assert_eq!(attempts[1].attempt_number, 2);
        assert_eq!(attempts[2].attempt_number, 3);

        // Each reflection references the prior attempt
        let reflections = evolution.reflections();
        assert_eq!(reflections.len(), 2);
        assert_eq!(reflections[0].prior_attempt, 1);
        assert_eq!(reflections[1].prior_attempt, 2);

        // Strategies escalated
        assert_ne!(attempts[0].strategy_used, attempts[1].strategy_used);
        assert_ne!(attempts[1].strategy_used, attempts[2].strategy_used);
    }

    #[test]
    fn test_no_reflection_after_success() {
        // Arrange
        let mut evolution = EvolutionLoop::new();
        evolution.record_attempt(
            CorrectionStrategy::RetryLocal,
            AttemptOutcome::Success,
            vec!["fixed.rs".into()],
            None,
            500,
            2000,
        );

        // Act
        let reflection = evolution.reflect();

        // Assert
        assert!(reflection.is_none(), "No reflection needed after success");
    }

    #[test]
    fn test_empty_evolution_prompt_when_no_attempts() {
        let evolution = EvolutionLoop::new();
        assert!(evolution.build_evolution_prompt().is_empty());
    }

    #[test]
    fn test_strategy_escalation_ladder() {
        let mut evolution = EvolutionLoop::new();

        // Attempt 1: RetryLocal fails
        evolution.record_attempt(
            CorrectionStrategy::RetryLocal,
            AttemptOutcome::Failure,
            vec![],
            None,
            0,
            0,
        );
        let r1 = evolution.reflect().unwrap();
        assert_eq!(r1.recommended_strategy, CorrectionStrategy::Replan);

        // Attempt 2: Replan fails
        evolution.record_attempt(
            CorrectionStrategy::Replan,
            AttemptOutcome::Failure,
            vec![],
            None,
            0,
            0,
        );
        let r2 = evolution.reflect().unwrap();
        assert_eq!(r2.recommended_strategy, CorrectionStrategy::Subtask);

        // Attempt 3: Subtask fails
        evolution.record_attempt(
            CorrectionStrategy::Subtask,
            AttemptOutcome::Failure,
            vec![],
            None,
            0,
            0,
        );
        let r3 = evolution.reflect().unwrap();
        assert_eq!(r3.recommended_strategy, CorrectionStrategy::AgentSwap);
    }

    #[test]
    fn test_with_max_attempts_clamped() {
        let evolution = EvolutionLoop::with_max_attempts(100);
        // Should be clamped to MAX_EVOLUTION_ATTEMPTS
        assert_eq!(evolution.max_attempts, MAX_EVOLUTION_ATTEMPTS);
    }
}
