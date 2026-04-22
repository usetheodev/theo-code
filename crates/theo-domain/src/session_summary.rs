//! Structured session handoff — compact artifact persisted across sessions.
//!
//! Anthropic + OpenAI consensus in `docs/pesquisas/`: long-term state lives
//! as versioned artifacts in the repo. `SessionSummary` is the lightweight
//! bridge (~2k tokens budget) injected at session boot to eliminate the
//! "cold start" where a new agent spends 5-10 turns reconstructing context.
//!
//! References:
//! - `docs/pesquisas/effective-harnesses-for-long-running-agents.md:32-38`
//!   (Anthropic's claude-progress.txt + feature_list.json pattern)
//! - `docs/pesquisas/harness-engineering-openai.md:133-137`
//!   (OpenAI exec-plans/active/ pattern)

#![allow(clippy::field_reassign_with_default)] // Test builders tweak individual fields for readability.

use serde::{Deserialize, Serialize};

/// Max chars allowed per field to keep serialized size under the 2k-token budget.
pub const MAX_OBJECTIVE_CHARS: usize = 500;
pub const MAX_STEPS: usize = 20;
pub const MAX_PENDING: usize = 10;
pub const MAX_FILES: usize = 30;
pub const MAX_ERRORS: usize = 5;
pub const MAX_STEP_CHARS: usize = 120;
pub const MAX_ERROR_CHARS: usize = 200;

/// Compact summary of the just-completed session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Single-sentence objective (what the session was trying to achieve).
    pub task_objective: String,
    /// Completed milestones, each ≤`MAX_STEP_CHARS`.
    pub completed_steps: Vec<String>,
    /// Pending work to resume next session.
    pub pending_steps: Vec<String>,
    /// File paths modified (for the next agent to orient quickly).
    pub files_modified: Vec<String>,
    /// Recent errors/warnings that might still be relevant.
    pub errors_encountered: Vec<String>,
}

impl SessionSummary {
    /// Enforce field caps, truncating in-place. Called before persistence.
    pub fn enforce_caps(&mut self) {
        if self.task_objective.chars().count() > MAX_OBJECTIVE_CHARS {
            self.task_objective = truncate_chars(&self.task_objective, MAX_OBJECTIVE_CHARS);
        }
        cap_vec(&mut self.completed_steps, MAX_STEPS, MAX_STEP_CHARS);
        cap_vec(&mut self.pending_steps, MAX_PENDING, MAX_STEP_CHARS);
        self.files_modified.truncate(MAX_FILES);
        cap_vec(&mut self.errors_encountered, MAX_ERRORS, MAX_ERROR_CHARS);
    }

    /// Render as compact plaintext for injection into the system prompt.
    /// Sections use `##` headers so the model parses them reliably.
    pub fn as_prompt_block(&self) -> String {
        let mut out = String::new();
        out.push_str("## Previous Session Handoff\n");
        if !self.task_objective.is_empty() {
            out.push_str(&format!("Objective: {}\n", self.task_objective));
        }
        if !self.completed_steps.is_empty() {
            out.push_str("Completed:\n");
            for s in &self.completed_steps {
                out.push_str(&format!("- {s}\n"));
            }
        }
        if !self.pending_steps.is_empty() {
            out.push_str("Pending:\n");
            for s in &self.pending_steps {
                out.push_str(&format!("- {s}\n"));
            }
        }
        if !self.files_modified.is_empty() {
            out.push_str(&format!(
                "Files touched: {}\n",
                self.files_modified.join(", ")
            ));
        }
        if !self.errors_encountered.is_empty() {
            out.push_str("Errors:\n");
            for e in &self.errors_encountered {
                out.push_str(&format!("- {e}\n"));
            }
        }
        out
    }

    /// Rough token estimate for budget checks (uses theo-domain::tokens).
    pub fn estimated_tokens(&self) -> usize {
        crate::tokens::estimate_tokens(&self.as_prompt_block())
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn cap_vec(v: &mut Vec<String>, max_len: usize, max_chars: usize) {
    v.truncate(max_len);
    for item in v.iter_mut() {
        if item.chars().count() > max_chars {
            *item = truncate_chars(item, max_chars);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_summary() -> SessionSummary {
        SessionSummary {
            task_objective: "Fix login bug in auth flow".into(),
            completed_steps: vec!["parsed error logs".into(), "isolated to session.rs".into()],
            pending_steps: vec!["patch validate_session".into()],
            files_modified: vec!["src/auth/session.rs".into()],
            errors_encountered: vec!["SignatureMismatch at line 42".into()],
        }
    }

    #[test]
    fn default_summary_is_empty() {
        let s = SessionSummary::default();
        assert!(s.task_objective.is_empty());
        assert!(s.completed_steps.is_empty());
    }

    #[test]
    fn prompt_block_contains_objective_and_steps() {
        let s = full_summary();
        let block = s.as_prompt_block();
        assert!(block.contains("Fix login bug"));
        assert!(block.contains("parsed error logs"));
        assert!(block.contains("patch validate_session"));
        assert!(block.contains("SignatureMismatch"));
    }

    #[test]
    fn enforce_caps_truncates_objective() {
        let mut s = SessionSummary::default();
        s.task_objective = "x".repeat(MAX_OBJECTIVE_CHARS * 2);
        s.enforce_caps();
        assert_eq!(s.task_objective.chars().count(), MAX_OBJECTIVE_CHARS);
    }

    #[test]
    fn enforce_caps_truncates_step_lists() {
        let mut s = SessionSummary::default();
        s.completed_steps = (0..MAX_STEPS * 2).map(|i| format!("step {i}")).collect();
        s.enforce_caps();
        assert_eq!(s.completed_steps.len(), MAX_STEPS);
    }

    #[test]
    fn enforce_caps_truncates_files_list() {
        let mut s = SessionSummary::default();
        s.files_modified = (0..MAX_FILES * 2).map(|i| format!("f{i}.rs")).collect();
        s.enforce_caps();
        assert_eq!(s.files_modified.len(), MAX_FILES);
    }

    #[test]
    fn enforce_caps_truncates_long_steps() {
        let mut s = SessionSummary::default();
        s.completed_steps = vec!["x".repeat(MAX_STEP_CHARS * 3)];
        s.enforce_caps();
        assert_eq!(s.completed_steps[0].chars().count(), MAX_STEP_CHARS);
    }

    #[test]
    fn estimated_tokens_stays_under_2k_with_full_caps() {
        let mut s = SessionSummary {
            task_objective: "x".repeat(MAX_OBJECTIVE_CHARS),
            completed_steps: (0..MAX_STEPS).map(|_| "y".repeat(MAX_STEP_CHARS)).collect(),
            pending_steps: (0..MAX_PENDING).map(|_| "y".repeat(MAX_STEP_CHARS)).collect(),
            files_modified: (0..MAX_FILES).map(|i| format!("f{i}.rs")).collect(),
            errors_encountered: (0..MAX_ERRORS).map(|_| "e".repeat(MAX_ERROR_CHARS)).collect(),
        };
        s.enforce_caps();
        assert!(
            s.estimated_tokens() <= 2000,
            "summary exceeds 2k token budget: {}",
            s.estimated_tokens()
        );
    }

    #[test]
    fn serde_roundtrip_preserves_content() {
        let s = full_summary();
        let json = serde_json::to_string(&s).unwrap();
        let back: SessionSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}
