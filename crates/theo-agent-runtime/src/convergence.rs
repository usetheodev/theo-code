use std::path::Path;

use serde::{Deserialize, Serialize};

/// Trait for convergence criteria that determine when an agent run is complete.
pub trait ConvergenceCriterion: Send + Sync {
    fn name(&self) -> &str;
    fn is_converged(&self, context: &ConvergenceContext) -> bool;
}

/// Context passed to convergence criteria for evaluation.
pub struct ConvergenceContext {
    pub has_git_changes: bool,
    pub edits_succeeded: usize,
    pub done_requested: bool,
    pub iteration: usize,
    pub max_iterations: usize,
}

/// Convergence criterion: git diff is non-empty (real changes exist).
pub struct GitDiffConvergence;

impl ConvergenceCriterion for GitDiffConvergence {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn is_converged(&self, context: &ConvergenceContext) -> bool {
        context.has_git_changes && context.done_requested
    }
}

/// Convergence criterion: at least one successful edit was made.
pub struct EditSuccessConvergence;

impl ConvergenceCriterion for EditSuccessConvergence {
    fn name(&self) -> &str {
        "edit_success"
    }

    fn is_converged(&self, context: &ConvergenceContext) -> bool {
        context.edits_succeeded > 0 && context.done_requested
    }
}

/// How multiple convergence criteria are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConvergenceMode {
    /// All criteria must be met.
    AllOf,
    /// Any single criterion is sufficient.
    AnyOf,
}

/// Evaluates convergence using a set of criteria and a combination mode.
pub struct ConvergenceEvaluator {
    criteria: Vec<Box<dyn ConvergenceCriterion>>,
    mode: ConvergenceMode,
}

impl ConvergenceEvaluator {
    pub fn new(criteria: Vec<Box<dyn ConvergenceCriterion>>, mode: ConvergenceMode) -> Self {
        Self { criteria, mode }
    }

    /// Evaluates whether convergence is achieved.
    ///
    /// - AllOf: all criteria must return true.
    /// - AnyOf: at least one criterion must return true.
    /// - Empty criteria list: returns true (vacuous truth).
    pub fn evaluate(&self, context: &ConvergenceContext) -> bool {
        if self.criteria.is_empty() {
            return true;
        }

        match self.mode {
            ConvergenceMode::AllOf => self.criteria.iter().all(|c| c.is_converged(context)),
            ConvergenceMode::AnyOf => self.criteria.iter().any(|c| c.is_converged(context)),
        }
    }

    /// Returns the names of criteria that are NOT yet met.
    pub fn pending_criteria(&self, context: &ConvergenceContext) -> Vec<&str> {
        self.criteria
            .iter()
            .filter(|c| !c.is_converged(context))
            .map(|c| c.name())
            .collect()
    }
}

/// Check if the project has real uncommitted changes via git diff.
pub async fn check_git_changes(project_dir: &Path) -> bool {
    let output = tokio::process::Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(project_dir)
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            !stdout.trim().is_empty()
        }
        Err(_) => true, // If git fails, assume changes exist
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(has_git: bool, edits: usize, done: bool) -> ConvergenceContext {
        ConvergenceContext {
            has_git_changes: has_git,
            edits_succeeded: edits,
            done_requested: done,
            iteration: 5,
            max_iterations: 30,
        }
    }

    #[test]
    fn git_diff_converged_with_changes_and_done() {
        let criterion = GitDiffConvergence;
        assert!(criterion.is_converged(&ctx(true, 0, true)));
    }

    #[test]
    fn git_diff_not_converged_without_changes() {
        let criterion = GitDiffConvergence;
        assert!(!criterion.is_converged(&ctx(false, 0, true)));
    }

    #[test]
    fn git_diff_not_converged_without_done() {
        let criterion = GitDiffConvergence;
        assert!(!criterion.is_converged(&ctx(true, 0, false)));
    }

    #[test]
    fn edit_success_converged_with_edits_and_done() {
        let criterion = EditSuccessConvergence;
        assert!(criterion.is_converged(&ctx(false, 1, true)));
    }

    #[test]
    fn edit_success_not_converged_without_edits() {
        let criterion = EditSuccessConvergence;
        assert!(!criterion.is_converged(&ctx(false, 0, true)));
    }

    #[test]
    fn allof_mode_requires_all() {
        let evaluator = ConvergenceEvaluator::new(
            vec![
                Box::new(GitDiffConvergence),
                Box::new(EditSuccessConvergence),
            ],
            ConvergenceMode::AllOf,
        );
        // Only git changes, no edits → not converged
        assert!(!evaluator.evaluate(&ctx(true, 0, true)));
        // Both met → converged
        assert!(evaluator.evaluate(&ctx(true, 1, true)));
    }

    #[test]
    fn anyof_mode_sufficient_with_one() {
        let evaluator = ConvergenceEvaluator::new(
            vec![
                Box::new(GitDiffConvergence),
                Box::new(EditSuccessConvergence),
            ],
            ConvergenceMode::AnyOf,
        );
        // Only git changes → converged
        assert!(evaluator.evaluate(&ctx(true, 0, true)));
        // Only edits → converged
        assert!(evaluator.evaluate(&ctx(false, 1, true)));
        // Neither → not converged
        assert!(!evaluator.evaluate(&ctx(false, 0, true)));
    }

    #[test]
    fn empty_criteria_vacuous_truth() {
        let evaluator = ConvergenceEvaluator::new(vec![], ConvergenceMode::AllOf);
        assert!(evaluator.evaluate(&ctx(false, 0, false)));
    }

    #[test]
    fn pending_criteria_lists_unmet() {
        let evaluator = ConvergenceEvaluator::new(
            vec![
                Box::new(GitDiffConvergence),
                Box::new(EditSuccessConvergence),
            ],
            ConvergenceMode::AllOf,
        );
        let pending = evaluator.pending_criteria(&ctx(true, 0, true));
        assert_eq!(pending, vec!["edit_success"]);
    }

    #[test]
    fn convergence_mode_serde_roundtrip() {
        let modes = [ConvergenceMode::AllOf, ConvergenceMode::AnyOf];
        for mode in &modes {
            let json = serde_json::to_string(mode).unwrap();
            let back: ConvergenceMode = serde_json::from_str(&json).unwrap();
            assert_eq!(*mode, back);
        }
    }
}
