use serde::{Deserialize, Serialize};

use crate::identifiers::{RunId, TaskId};

use crate::error::TransitionError;

// ---------------------------------------------------------------------------
// RunState — State Machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunState {
    Initialized,
    Planning,
    Executing,
    Evaluating,
    Converged,
    Replanning,
    Waiting,
    Aborted,
}

impl RunState {
    /// Returns whether transitioning from `self` to `target` is valid.
    ///
    /// All match arms are exhaustive — no wildcards.
    ///
    /// Note: the Evaluating → Replanning → Planning cycle has no circuit
    /// breaker in the type. Iteration limits are enforced by the orchestrator
    /// via `AgentRun.max_iterations` (Phase 07 BudgetEnforcer).
    pub fn can_transition_to(&self, target: RunState) -> bool {
        match self {
            RunState::Initialized => match target {
                RunState::Planning => true,
                RunState::Aborted => true,
                RunState::Initialized => false,
                RunState::Executing => false,
                RunState::Evaluating => false,
                RunState::Converged => false,
                RunState::Replanning => false,
                RunState::Waiting => false,
            },
            RunState::Planning => match target {
                RunState::Executing => true,
                RunState::Aborted => true,
                RunState::Initialized => false,
                RunState::Planning => false,
                RunState::Evaluating => false,
                RunState::Converged => false,
                RunState::Replanning => false,
                RunState::Waiting => false,
            },
            RunState::Executing => match target {
                RunState::Evaluating => true,
                RunState::Aborted => true,
                RunState::Initialized => false,
                RunState::Planning => false,
                RunState::Executing => false,
                RunState::Converged => false,
                RunState::Replanning => false,
                RunState::Waiting => false,
            },
            RunState::Evaluating => match target {
                RunState::Converged => true,
                RunState::Replanning => true,
                RunState::Waiting => true,
                RunState::Aborted => true,
                RunState::Initialized => false,
                RunState::Planning => false,
                RunState::Executing => false,
                RunState::Evaluating => false,
            },
            RunState::Replanning => match target {
                RunState::Planning => true,
                RunState::Aborted => true,
                RunState::Initialized => false,
                RunState::Executing => false,
                RunState::Evaluating => false,
                RunState::Converged => false,
                RunState::Replanning => false,
                RunState::Waiting => false,
            },
            RunState::Waiting => match target {
                RunState::Planning => true,
                RunState::Aborted => true,
                RunState::Initialized => false,
                RunState::Executing => false,
                RunState::Evaluating => false,
                RunState::Converged => false,
                RunState::Replanning => false,
                RunState::Waiting => false,
            },
            RunState::Converged => match target {
                RunState::Initialized => false,
                RunState::Planning => false,
                RunState::Executing => false,
                RunState::Evaluating => false,
                RunState::Converged => false,
                RunState::Replanning => false,
                RunState::Waiting => false,
                RunState::Aborted => false,
            },
            RunState::Aborted => match target {
                RunState::Initialized => false,
                RunState::Planning => false,
                RunState::Executing => false,
                RunState::Evaluating => false,
                RunState::Converged => false,
                RunState::Replanning => false,
                RunState::Waiting => false,
                RunState::Aborted => false,
            },
        }
    }

    pub fn is_terminal(&self) -> bool {
        match self {
            RunState::Converged => true,
            RunState::Aborted => true,
            RunState::Initialized => false,
            RunState::Planning => false,
            RunState::Executing => false,
            RunState::Evaluating => false,
            RunState::Replanning => false,
            RunState::Waiting => false,
        }
    }
}

impl std::fmt::Display for RunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunState::Initialized => write!(f, "Initialized"),
            RunState::Planning => write!(f, "Planning"),
            RunState::Executing => write!(f, "Executing"),
            RunState::Evaluating => write!(f, "Evaluating"),
            RunState::Converged => write!(f, "Converged"),
            RunState::Replanning => write!(f, "Replanning"),
            RunState::Waiting => write!(f, "Waiting"),
            RunState::Aborted => write!(f, "Aborted"),
        }
    }
}

impl super::StateMachine for RunState {
    fn can_transition_to(&self, target: Self) -> bool {
        RunState::can_transition_to(self, target)
    }
    fn is_terminal(&self) -> bool {
        RunState::is_terminal(self)
    }
}

// ---------------------------------------------------------------------------
// AgentRun — aggregate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRun {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub state: RunState,
    pub iteration: usize,
    pub max_iterations: usize,
    pub created_at: u64,
    pub updated_at: u64,
}

impl AgentRun {
    pub fn transition(&mut self, target: RunState) -> Result<(), TransitionError> {
        super::transition(&mut self.state, target)?;
        self.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_millis() as u64;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [RunState; 8] = [
        RunState::Initialized,
        RunState::Planning,
        RunState::Executing,
        RunState::Evaluating,
        RunState::Converged,
        RunState::Replanning,
        RunState::Waiting,
        RunState::Aborted,
    ];

    const VALID_TRANSITIONS: &[(RunState, RunState)] = &[
        (RunState::Initialized, RunState::Planning),
        (RunState::Initialized, RunState::Aborted),
        (RunState::Planning, RunState::Executing),
        (RunState::Planning, RunState::Aborted),
        (RunState::Executing, RunState::Evaluating),
        (RunState::Executing, RunState::Aborted),
        (RunState::Evaluating, RunState::Converged),
        (RunState::Evaluating, RunState::Replanning),
        (RunState::Evaluating, RunState::Waiting),
        (RunState::Evaluating, RunState::Aborted),
        (RunState::Replanning, RunState::Planning),
        (RunState::Replanning, RunState::Aborted),
        (RunState::Waiting, RunState::Planning),
        (RunState::Waiting, RunState::Aborted),
    ];

    #[test]
    fn transition_table_exhaustive() {
        for from in &ALL_STATES {
            for to in &ALL_STATES {
                let expected = VALID_TRANSITIONS.contains(&(*from, *to));
                assert_eq!(
                    from.can_transition_to(*to),
                    expected,
                    "{:?} → {:?}: expected {}",
                    from,
                    to,
                    expected
                );
            }
        }
    }

    #[test]
    fn happy_path_to_converged() {
        let mut state = RunState::Initialized;
        assert!(super::super::transition(&mut state, RunState::Planning).is_ok());
        assert!(super::super::transition(&mut state, RunState::Executing).is_ok());
        assert!(super::super::transition(&mut state, RunState::Evaluating).is_ok());
        assert!(super::super::transition(&mut state, RunState::Converged).is_ok());
        assert_eq!(state, RunState::Converged);
    }

    #[test]
    fn replanning_cycle() {
        let mut state = RunState::Evaluating;
        assert!(super::super::transition(&mut state, RunState::Replanning).is_ok());
        assert!(super::super::transition(&mut state, RunState::Planning).is_ok());
        assert!(super::super::transition(&mut state, RunState::Executing).is_ok());
        assert!(super::super::transition(&mut state, RunState::Evaluating).is_ok());
        assert!(super::super::transition(&mut state, RunState::Converged).is_ok());
    }

    #[test]
    fn waiting_to_planning() {
        let mut state = RunState::Waiting;
        assert!(super::super::transition(&mut state, RunState::Planning).is_ok());
        assert_eq!(state, RunState::Planning);
    }

    #[test]
    fn any_non_terminal_can_abort() {
        for state in &ALL_STATES {
            if !state.is_terminal() {
                assert!(
                    state.can_transition_to(RunState::Aborted),
                    "{:?} should be abortable",
                    state
                );
            }
        }
    }

    #[test]
    fn converged_rejects_all() {
        for t in &ALL_STATES {
            assert!(!RunState::Converged.can_transition_to(*t));
        }
    }

    #[test]
    fn aborted_rejects_all() {
        for t in &ALL_STATES {
            assert!(!RunState::Aborted.can_transition_to(*t));
        }
    }

    #[test]
    fn is_terminal_correct() {
        assert!(RunState::Converged.is_terminal());
        assert!(RunState::Aborted.is_terminal());
        assert!(!RunState::Initialized.is_terminal());
        assert!(!RunState::Planning.is_terminal());
        assert!(!RunState::Executing.is_terminal());
        assert!(!RunState::Evaluating.is_terminal());
        assert!(!RunState::Replanning.is_terminal());
        assert!(!RunState::Waiting.is_terminal());
    }

    #[test]
    fn transition_atomicity_state_preserved_on_error() {
        let mut state = RunState::Converged;
        let result = super::super::transition(&mut state, RunState::Planning);
        assert!(result.is_err());
        assert_eq!(state, RunState::Converged);
    }

    #[test]
    fn serde_roundtrip_all_variants() {
        for state in &ALL_STATES {
            let json = serde_json::to_string(state).unwrap();
            let back: RunState = serde_json::from_str(&json).unwrap();
            assert_eq!(*state, back);
        }
    }

    #[test]
    fn display_all_variants() {
        let expected = [
            "Initialized", "Planning", "Executing", "Evaluating",
            "Converged", "Replanning", "Waiting", "Aborted",
        ];
        for (s, name) in ALL_STATES.iter().zip(expected.iter()) {
            assert_eq!(format!("{}", s), *name);
        }
    }

    #[test]
    fn agent_run_serde_roundtrip() {
        let run = AgentRun {
            run_id: RunId::new("r-1"),
            task_id: TaskId::new("t-1"),
            state: RunState::Executing,
            iteration: 5,
            max_iterations: 30,
            created_at: 1000,
            updated_at: 2000,
        };
        let json = serde_json::to_string(&run).unwrap();
        let back: AgentRun = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_id, run.run_id);
        assert_eq!(back.state, run.state);
        assert_eq!(back.iteration, 5);
    }

    #[test]
    fn agent_run_transition_updates_timestamp() {
        let mut run = AgentRun {
            run_id: RunId::new("r-1"),
            task_id: TaskId::new("t-1"),
            state: RunState::Initialized,
            iteration: 0,
            max_iterations: 30,
            created_at: 1000,
            updated_at: 1000,
        };
        run.transition(RunState::Planning).unwrap();
        assert!(run.updated_at >= 1000);
        assert_eq!(run.state, RunState::Planning);
    }
}
