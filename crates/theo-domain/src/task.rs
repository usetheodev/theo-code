use serde::{Deserialize, Serialize};

use crate::identifiers::TaskId;
use crate::session::SessionId;

use crate::error::TransitionError;

// ---------------------------------------------------------------------------
// TaskState — State Machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    Pending,
    Ready,
    Running,
    WaitingTool,
    WaitingInput,
    Blocked,
    Completed,
    Failed,
    Cancelled,
}

impl TaskState {
    /// Returns whether transitioning from `self` to `target` is valid.
    ///
    /// All match arms are exhaustive — no wildcards.
    pub fn can_transition_to(&self, target: TaskState) -> bool {
        match self {
            TaskState::Pending => match target {
                TaskState::Ready => true,
                TaskState::Cancelled => true,
                TaskState::Pending => false,
                TaskState::Running => false,
                TaskState::WaitingTool => false,
                TaskState::WaitingInput => false,
                TaskState::Blocked => false,
                TaskState::Completed => false,
                TaskState::Failed => false,
            },
            TaskState::Ready => match target {
                TaskState::Running => true,
                TaskState::Cancelled => true,
                TaskState::Pending => false,
                TaskState::Ready => false,
                TaskState::WaitingTool => false,
                TaskState::WaitingInput => false,
                TaskState::Blocked => false,
                TaskState::Completed => false,
                TaskState::Failed => false,
            },
            TaskState::Running => match target {
                TaskState::WaitingTool => true,
                TaskState::WaitingInput => true,
                TaskState::Blocked => true,
                TaskState::Completed => true,
                TaskState::Failed => true,
                TaskState::Cancelled => true,
                TaskState::Pending => false,
                TaskState::Ready => false,
                TaskState::Running => false,
            },
            TaskState::WaitingTool => match target {
                TaskState::Running => true,
                TaskState::Failed => true,
                TaskState::Cancelled => true,
                TaskState::Pending => false,
                TaskState::Ready => false,
                TaskState::WaitingTool => false,
                TaskState::WaitingInput => false,
                TaskState::Blocked => false,
                TaskState::Completed => false,
            },
            TaskState::WaitingInput => match target {
                TaskState::Running => true,
                TaskState::Failed => true,
                TaskState::Cancelled => true,
                TaskState::Pending => false,
                TaskState::Ready => false,
                TaskState::WaitingTool => false,
                TaskState::WaitingInput => false,
                TaskState::Blocked => false,
                TaskState::Completed => false,
            },
            TaskState::Blocked => match target {
                TaskState::Running => true,
                TaskState::Failed => true,
                TaskState::Cancelled => true,
                TaskState::Pending => false,
                TaskState::Ready => false,
                TaskState::WaitingTool => false,
                TaskState::WaitingInput => false,
                TaskState::Blocked => false,
                TaskState::Completed => false,
            },
            TaskState::Completed => match target {
                TaskState::Pending => false,
                TaskState::Ready => false,
                TaskState::Running => false,
                TaskState::WaitingTool => false,
                TaskState::WaitingInput => false,
                TaskState::Blocked => false,
                TaskState::Completed => false,
                TaskState::Failed => false,
                TaskState::Cancelled => false,
            },
            TaskState::Failed => match target {
                TaskState::Pending => false,
                TaskState::Ready => false,
                TaskState::Running => false,
                TaskState::WaitingTool => false,
                TaskState::WaitingInput => false,
                TaskState::Blocked => false,
                TaskState::Completed => false,
                TaskState::Failed => false,
                TaskState::Cancelled => false,
            },
            TaskState::Cancelled => match target {
                TaskState::Pending => false,
                TaskState::Ready => false,
                TaskState::Running => false,
                TaskState::WaitingTool => false,
                TaskState::WaitingInput => false,
                TaskState::Blocked => false,
                TaskState::Completed => false,
                TaskState::Failed => false,
                TaskState::Cancelled => false,
            },
        }
    }

    pub fn is_terminal(&self) -> bool {
        match self {
            TaskState::Completed => true,
            TaskState::Failed => true,
            TaskState::Cancelled => true,
            TaskState::Pending => false,
            TaskState::Ready => false,
            TaskState::Running => false,
            TaskState::WaitingTool => false,
            TaskState::WaitingInput => false,
            TaskState::Blocked => false,
        }
    }
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskState::Pending => write!(f, "Pending"),
            TaskState::Ready => write!(f, "Ready"),
            TaskState::Running => write!(f, "Running"),
            TaskState::WaitingTool => write!(f, "WaitingTool"),
            TaskState::WaitingInput => write!(f, "WaitingInput"),
            TaskState::Blocked => write!(f, "Blocked"),
            TaskState::Completed => write!(f, "Completed"),
            TaskState::Failed => write!(f, "Failed"),
            TaskState::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl super::StateMachine for TaskState {
    fn can_transition_to(&self, target: Self) -> bool {
        TaskState::can_transition_to(self, target)
    }
    fn is_terminal(&self) -> bool {
        TaskState::is_terminal(self)
    }
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    Coder,
    Reviewer,
    Planner,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub name: String,
    pub path: String,
    pub artifact_type: String,
}

// ---------------------------------------------------------------------------
// Task — aggregate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub state: TaskState,
    pub agent_type: AgentType,
    pub objective: String,
    pub artifacts: Vec<Artifact>,
    pub created_at: u64,
    pub updated_at: u64,
    pub completed_at: Option<u64>,
}

impl Task {
    pub fn transition(&mut self, target: TaskState) -> Result<(), TransitionError> {
        super::transition(&mut self.state, target)?;
        self.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_millis() as u64;
        if target.is_terminal() {
            self.completed_at = Some(self.updated_at);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [TaskState; 9] = [
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

    const VALID_TRANSITIONS: &[(TaskState, TaskState)] = &[
        (TaskState::Pending, TaskState::Ready),
        (TaskState::Pending, TaskState::Cancelled),
        (TaskState::Ready, TaskState::Running),
        (TaskState::Ready, TaskState::Cancelled),
        (TaskState::Running, TaskState::WaitingTool),
        (TaskState::Running, TaskState::WaitingInput),
        (TaskState::Running, TaskState::Blocked),
        (TaskState::Running, TaskState::Completed),
        (TaskState::Running, TaskState::Failed),
        (TaskState::Running, TaskState::Cancelled),
        (TaskState::WaitingTool, TaskState::Running),
        (TaskState::WaitingTool, TaskState::Failed),
        (TaskState::WaitingTool, TaskState::Cancelled),
        (TaskState::WaitingInput, TaskState::Running),
        (TaskState::WaitingInput, TaskState::Failed),
        (TaskState::WaitingInput, TaskState::Cancelled),
        (TaskState::Blocked, TaskState::Running),
        (TaskState::Blocked, TaskState::Failed),
        (TaskState::Blocked, TaskState::Cancelled),
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
    fn happy_path_pending_to_completed() {
        let mut state = TaskState::Pending;
        assert!(super::super::transition(&mut state, TaskState::Ready).is_ok());
        assert!(super::super::transition(&mut state, TaskState::Running).is_ok());
        assert!(super::super::transition(&mut state, TaskState::Completed).is_ok());
        assert_eq!(state, TaskState::Completed);
    }

    #[test]
    fn waiting_tool_to_running() {
        let mut state = TaskState::WaitingTool;
        assert!(super::super::transition(&mut state, TaskState::Running).is_ok());
        assert_eq!(state, TaskState::Running);
    }

    #[test]
    fn waiting_tool_to_failed() {
        let mut state = TaskState::WaitingTool;
        assert!(super::super::transition(&mut state, TaskState::Failed).is_ok());
        assert_eq!(state, TaskState::Failed);
    }

    #[test]
    fn waiting_input_to_failed() {
        let mut state = TaskState::WaitingInput;
        assert!(super::super::transition(&mut state, TaskState::Failed).is_ok());
    }

    #[test]
    fn blocked_to_running() {
        assert!(TaskState::Blocked.can_transition_to(TaskState::Running));
    }

    #[test]
    fn blocked_to_failed() {
        assert!(TaskState::Blocked.can_transition_to(TaskState::Failed));
    }

    #[test]
    fn blocked_to_cancelled() {
        assert!(TaskState::Blocked.can_transition_to(TaskState::Cancelled));
    }

    #[test]
    fn invariant_4_completed_cannot_go_to_running() {
        assert!(!TaskState::Completed.can_transition_to(TaskState::Running));
    }

    #[test]
    fn completed_rejects_all() {
        for target in &ALL_STATES {
            assert!(
                !TaskState::Completed.can_transition_to(*target),
                "Completed should reject {:?}",
                target
            );
        }
    }

    #[test]
    fn failed_rejects_all() {
        for target in &ALL_STATES {
            assert!(
                !TaskState::Failed.can_transition_to(*target),
                "Failed should reject {:?}",
                target
            );
        }
    }

    #[test]
    fn cancelled_rejects_all() {
        for target in &ALL_STATES {
            assert!(
                !TaskState::Cancelled.can_transition_to(*target),
                "Cancelled should reject {:?}",
                target
            );
        }
    }

    #[test]
    fn is_terminal_correct() {
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(TaskState::Cancelled.is_terminal());
        assert!(!TaskState::Pending.is_terminal());
        assert!(!TaskState::Ready.is_terminal());
        assert!(!TaskState::Running.is_terminal());
        assert!(!TaskState::WaitingTool.is_terminal());
        assert!(!TaskState::WaitingInput.is_terminal());
        assert!(!TaskState::Blocked.is_terminal());
    }

    #[test]
    fn transition_atomicity_state_preserved_on_error() {
        let mut state = TaskState::Completed;
        let result = super::super::transition(&mut state, TaskState::Running);
        assert!(result.is_err());
        assert_eq!(
            state,
            TaskState::Completed,
            "state must not change on error"
        );
    }

    #[test]
    fn serde_roundtrip_all_variants() {
        for state in &ALL_STATES {
            let json = serde_json::to_string(state).unwrap();
            let back: TaskState = serde_json::from_str(&json).unwrap();
            assert_eq!(*state, back, "serde roundtrip failed for {:?}", state);
        }
    }

    #[test]
    fn display_all_variants() {
        let expected = [
            "Pending",
            "Ready",
            "Running",
            "WaitingTool",
            "WaitingInput",
            "Blocked",
            "Completed",
            "Failed",
            "Cancelled",
        ];
        for (state, name) in ALL_STATES.iter().zip(expected.iter()) {
            assert_eq!(format!("{}", state), *name);
        }
    }

    #[test]
    fn task_serde_roundtrip() {
        let task = Task {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            state: TaskState::Running,
            agent_type: AgentType::Coder,
            objective: "fix bug".to_string(),
            artifacts: vec![Artifact {
                name: "main.rs".into(),
                path: "src/main.rs".into(),
                artifact_type: "file".into(),
            }],
            created_at: 1000,
            updated_at: 2000,
            completed_at: None,
        };
        let json = serde_json::to_string(&task).unwrap();
        let back: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task_id, task.task_id);
        assert_eq!(back.state, task.state);
        assert_eq!(back.objective, task.objective);
    }

    #[test]
    fn task_transition_updates_timestamps() {
        let mut task = Task {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            state: TaskState::Running,
            agent_type: AgentType::Coder,
            objective: "test".into(),
            artifacts: vec![],
            created_at: 1000,
            updated_at: 1000,
            completed_at: None,
        };
        task.transition(TaskState::Completed).unwrap();
        assert!(task.updated_at >= 1000);
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn agent_type_serde_roundtrip() {
        let types = [
            AgentType::Coder,
            AgentType::Reviewer,
            AgentType::Planner,
            AgentType::Custom("my-agent".into()),
        ];
        for t in &types {
            let json = serde_json::to_string(t).unwrap();
            let back: AgentType = serde_json::from_str(&json).unwrap();
            assert_eq!(*t, back);
        }
    }
}
