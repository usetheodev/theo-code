pub mod agent_run;
pub mod agent_spec;
pub mod budget;
pub mod capability;
pub mod code_intel;
pub mod episode;
pub mod error;
pub mod event;
pub mod evolution;
pub mod graph_context;
pub mod identifiers;
pub mod memory;
pub mod permission;
pub mod priority;
pub mod retry_policy;
pub mod routing;
pub mod safe_json;
pub mod sandbox;
pub mod session;
pub mod session_search;
pub mod session_summary;
pub mod task;
pub mod tokens;
pub mod tool;
pub mod tool_call;
pub mod truncate;
pub mod wiki_backend;
pub mod working_set;

/// Trait for state machines with validated transitions.
pub trait StateMachine: Copy + PartialEq + std::fmt::Debug {
    fn can_transition_to(&self, target: Self) -> bool;
    fn is_terminal(&self) -> bool;
}

/// Atomic transition: mutates state ONLY if valid.
/// On Err, the original state is preserved intact.
pub fn transition<S: StateMachine>(
    current: &mut S,
    target: S,
) -> Result<(), error::TransitionError> {
    if current.can_transition_to(target) {
        *current = target;
        Ok(())
    } else {
        Err(error::TransitionError::InvalidTransition {
            from: format!("{:?}", current),
            to: format!("{:?}", target),
        })
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use agent_run::RunState;
    use task::TaskState;
    use tool_call::ToolCallState;

    /// P1: Every terminal state rejects all transitions.
    #[test]
    fn p1_terminal_states_reject_all_transitions() {
        let task_terminals = [
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Cancelled,
        ];
        let task_all = [
            TaskState::Pending,
            TaskState::Ready,
            TaskState::Running,
            TaskState::WaitingTool,
            TaskState::WaitingInput,
            TaskState::Blocked,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Cancelled,
        ];
        for t in &task_terminals {
            for target in &task_all {
                assert!(
                    !t.can_transition_to(*target),
                    "TaskState::{:?} → {:?}",
                    t,
                    target
                );
            }
        }

        let tc_terminals = [
            ToolCallState::Succeeded,
            ToolCallState::Failed,
            ToolCallState::Timeout,
            ToolCallState::Cancelled,
        ];
        let tc_all = [
            ToolCallState::Queued,
            ToolCallState::Dispatched,
            ToolCallState::Running,
            ToolCallState::Succeeded,
            ToolCallState::Failed,
            ToolCallState::Timeout,
            ToolCallState::Cancelled,
        ];
        for t in &tc_terminals {
            for target in &tc_all {
                assert!(
                    !t.can_transition_to(*target),
                    "ToolCallState::{:?} → {:?}",
                    t,
                    target
                );
            }
        }

        let run_terminals = [RunState::Converged, RunState::Aborted];
        let run_all = [
            RunState::Initialized,
            RunState::Planning,
            RunState::Executing,
            RunState::Evaluating,
            RunState::Converged,
            RunState::Replanning,
            RunState::Waiting,
            RunState::Aborted,
        ];
        for t in &run_terminals {
            for target in &run_all {
                assert!(
                    !t.can_transition_to(*target),
                    "RunState::{:?} → {:?}",
                    t,
                    target
                );
            }
        }
    }

    /// P2: Every non-terminal state accepts at least one transition.
    #[test]
    fn p2_non_terminal_states_have_at_least_one_valid_transition() {
        let task_non_terminals = [
            TaskState::Pending,
            TaskState::Ready,
            TaskState::Running,
            TaskState::WaitingTool,
            TaskState::WaitingInput,
            TaskState::Blocked,
        ];
        let task_all = [
            TaskState::Pending,
            TaskState::Ready,
            TaskState::Running,
            TaskState::WaitingTool,
            TaskState::WaitingInput,
            TaskState::Blocked,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Cancelled,
        ];
        for s in &task_non_terminals {
            let has_valid = task_all.iter().any(|t| s.can_transition_to(*t));
            assert!(has_valid, "TaskState::{:?} has no valid transitions", s);
        }

        let tc_non_terminals = [
            ToolCallState::Queued,
            ToolCallState::Dispatched,
            ToolCallState::Running,
        ];
        let tc_all = [
            ToolCallState::Queued,
            ToolCallState::Dispatched,
            ToolCallState::Running,
            ToolCallState::Succeeded,
            ToolCallState::Failed,
            ToolCallState::Timeout,
            ToolCallState::Cancelled,
        ];
        for s in &tc_non_terminals {
            let has_valid = tc_all.iter().any(|t| s.can_transition_to(*t));
            assert!(has_valid, "ToolCallState::{:?} has no valid transitions", s);
        }

        let run_non_terminals = [
            RunState::Initialized,
            RunState::Planning,
            RunState::Executing,
            RunState::Evaluating,
            RunState::Replanning,
            RunState::Waiting,
        ];
        let run_all = [
            RunState::Initialized,
            RunState::Planning,
            RunState::Executing,
            RunState::Evaluating,
            RunState::Converged,
            RunState::Replanning,
            RunState::Waiting,
            RunState::Aborted,
        ];
        for s in &run_non_terminals {
            let has_valid = run_all.iter().any(|t| s.can_transition_to(*t));
            assert!(has_valid, "RunState::{:?} has no valid transitions", s);
        }
    }

    /// P3: Valid transition updates state to target.
    #[test]
    fn p3_valid_transition_updates_state() {
        let mut ts = TaskState::Pending;
        transition(&mut ts, TaskState::Ready).unwrap();
        assert_eq!(ts, TaskState::Ready);

        let mut tc = ToolCallState::Queued;
        transition(&mut tc, ToolCallState::Dispatched).unwrap();
        assert_eq!(tc, ToolCallState::Dispatched);

        let mut rs = RunState::Initialized;
        transition(&mut rs, RunState::Planning).unwrap();
        assert_eq!(rs, RunState::Planning);
    }

    /// P4: Invalid transition preserves original state (atomicity).
    #[test]
    fn p4_invalid_transition_preserves_state() {
        let mut ts = TaskState::Completed;
        let _ = transition(&mut ts, TaskState::Running);
        assert_eq!(ts, TaskState::Completed);

        let mut tc = ToolCallState::Succeeded;
        let _ = transition(&mut tc, ToolCallState::Running);
        assert_eq!(tc, ToolCallState::Succeeded);

        let mut rs = RunState::Converged;
        let _ = transition(&mut rs, RunState::Planning);
        assert_eq!(rs, RunState::Converged);
    }

    #[test]
    fn transition_error_contains_from_and_to() {
        let mut state = TaskState::Completed;
        let err = transition(&mut state, TaskState::Running).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Completed"),
            "error should contain 'from' state: {}",
            msg
        );
        assert!(
            msg.contains("Running"),
            "error should contain 'to' state: {}",
            msg
        );
    }
}
