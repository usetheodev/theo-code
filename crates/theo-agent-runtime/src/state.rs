use std::collections::HashSet;

/// Phase of the agent state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Explore,
    Edit,
    Verify,
    Done,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Explore => write!(f, "EXPLORE"),
            Phase::Edit => write!(f, "EDIT"),
            Phase::Verify => write!(f, "VERIFY"),
            Phase::Done => write!(f, "DONE"),
        }
    }
}

/// Tracks agent progress through the task.
#[derive(Debug, Clone)]
pub struct AgentState {
    pub phase: Phase,
    pub files_read: HashSet<String>,
    pub searches_done: usize,
    pub edit_attempts: usize,
    pub edit_failures: Vec<String>,
    pub edits_succeeded: usize,
    pub edits_files: Vec<String>,
    pub done_blocked: usize,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            phase: Phase::Explore,
            files_read: HashSet::new(),
            searches_done: 0,
            edit_attempts: 0,
            edit_failures: Vec::new(),
            edits_succeeded: 0,
            edits_files: Vec::new(),
            done_blocked: 0,
        }
    }
}

impl AgentState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check and apply phase transitions based on iteration progress.
    pub fn maybe_transition(&mut self, iteration: usize, max_iterations: usize) {
        let one_third = max_iterations / 3;
        let two_thirds = (max_iterations * 2) / 3;

        match self.phase {
            Phase::Explore => {
                if iteration >= one_third || self.searches_done >= 3 {
                    self.phase = Phase::Edit;
                }
            }
            Phase::Edit => {
                if self.edits_succeeded > 0 {
                    self.phase = Phase::Verify;
                }
            }
            Phase::Verify | Phase::Done => {}
        }

        // Emergency: if we're past 2/3 and no edits, stay in Edit phase
        // (the context loop will inject urgency)
        if iteration >= two_thirds && self.edits_succeeded == 0 && self.phase != Phase::Explore {
            self.phase = Phase::Edit;
        }
    }

    /// Record that a file was read.
    pub fn record_read(&mut self, path: &str) {
        self.files_read.insert(path.to_string());
    }

    /// Record a search operation.
    pub fn record_search(&mut self) {
        self.searches_done += 1;
    }

    /// Record an edit attempt.
    pub fn record_edit_attempt(&mut self, file: &str, success: bool, failure_reason: Option<String>) {
        self.edit_attempts += 1;
        if success {
            self.edits_succeeded += 1;
            self.edits_files.push(file.to_string());
        } else if let Some(reason) = failure_reason {
            self.edit_failures.push(reason);
        }
    }

    /// Record that done() was blocked (no real changes).
    pub fn record_done_blocked(&mut self) {
        self.done_blocked += 1;
    }

    /// Build the context loop diagnostic message.
    pub fn build_context_loop(&self, iteration: usize, max_iterations: usize, task: &str) -> String {
        let remaining = max_iterations.saturating_sub(iteration);
        let files_read: Vec<&str> = self.files_read.iter().map(|s| s.as_str()).collect();
        let files_edited: Vec<&str> = self.edits_files.iter().map(|s| s.as_str()).collect();

        let mut problems = Vec::new();
        if self.edit_attempts > 0 && self.edits_succeeded == 0 {
            problems.push(format!(
                "All {} edit attempts failed. Reasons: {}",
                self.edit_attempts,
                self.edit_failures.join("; ")
            ));
        }
        if self.done_blocked > 0 {
            problems.push(format!(
                "done() blocked {} time(s) — no real changes detected in git diff.",
                self.done_blocked
            ));
        }
        if self.searches_done > 3 && self.edit_attempts == 0 {
            problems.push("Too many searches without editing. Stop exploring and start editing.".to_string());
        }
        if self.files_read.len() > 5 && self.edit_attempts == 0 {
            problems.push("Read many files but never edited. Time to act.".to_string());
        }

        let problems_str = if problems.is_empty() {
            "None".to_string()
        } else {
            problems.join("\n  - ")
        };

        let urgency = if remaining <= 3 {
            "\n⚠️ EMERGENCY: Very few iterations left. Make your changes NOW or call done()."
        } else if remaining <= 5 {
            "\n⚠️ WARNING: Running low on iterations. Focus on completing the task."
        } else {
            ""
        };

        format!(
            "── CONTEXT LOOP (iteration {iteration}/{max_iterations}, {remaining} remaining) ──\n\
             TASK: {task}\n\
             PHASE: {phase}\n\
             DONE: read {n_read} files, {n_search} searches, {n_edit} edit attempts ({n_success} succeeded)\n\
             FILES READ: {files_read}\n\
             FILES EDITED: {files_edited}\n\
             PROBLEMS: {problems_str}{urgency}\n\
             ──",
            phase = self.phase,
            n_read = self.files_read.len(),
            n_search = self.searches_done,
            n_edit = self.edit_attempts,
            n_success = self.edits_succeeded,
            files_read = if files_read.is_empty() { "none".to_string() } else { files_read.join(", ") },
            files_edited = if files_edited.is_empty() { "none".to_string() } else { files_edited.join(", ") },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let state = AgentState::new();
        assert_eq!(state.phase, Phase::Explore);
        assert_eq!(state.searches_done, 0);
        assert_eq!(state.edit_attempts, 0);
    }

    #[test]
    fn test_explore_to_edit_by_iteration() {
        let mut state = AgentState::new();
        state.maybe_transition(5, 15); // 5 >= 15/3 = 5
        assert_eq!(state.phase, Phase::Edit);
    }

    #[test]
    fn test_explore_to_edit_by_searches() {
        let mut state = AgentState::new();
        state.searches_done = 3;
        state.maybe_transition(1, 15);
        assert_eq!(state.phase, Phase::Edit);
    }

    #[test]
    fn test_edit_to_verify_on_success() {
        let mut state = AgentState::new();
        state.phase = Phase::Edit;
        state.edits_succeeded = 1;
        state.maybe_transition(6, 15);
        assert_eq!(state.phase, Phase::Verify);
    }

    #[test]
    fn test_context_loop_message() {
        let mut state = AgentState::new();
        state.record_read("src/main.py");
        state.record_search();
        let msg = state.build_context_loop(5, 15, "Fix the bug");
        assert!(msg.contains("iteration 5/15"));
        assert!(msg.contains("src/main.py"));
        assert!(msg.contains("Fix the bug"));
    }

    #[test]
    fn test_context_loop_emergency() {
        let state = AgentState::new();
        let msg = state.build_context_loop(13, 15, "task");
        assert!(msg.contains("EMERGENCY"));
    }
}
