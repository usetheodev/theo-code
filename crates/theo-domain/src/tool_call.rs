use serde::{Deserialize, Serialize};

use crate::identifiers::{CallId, TaskId};

// ---------------------------------------------------------------------------
// ToolCallState — State Machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallState {
    Queued,
    Dispatched,
    Running,
    Succeeded,
    Failed,
    Timeout,
    Cancelled,
}

impl ToolCallState {
    /// Returns whether transitioning from `self` to `target` is valid.
    ///
    /// All match arms are exhaustive — no wildcards.
    pub fn can_transition_to(&self, target: ToolCallState) -> bool {
        match self {
            ToolCallState::Queued => match target {
                ToolCallState::Dispatched => true,
                ToolCallState::Cancelled => true,
                ToolCallState::Queued => false,
                ToolCallState::Running => false,
                ToolCallState::Succeeded => false,
                ToolCallState::Failed => false,
                ToolCallState::Timeout => false,
            },
            ToolCallState::Dispatched => match target {
                ToolCallState::Running => true,
                ToolCallState::Cancelled => true,
                ToolCallState::Queued => false,
                ToolCallState::Dispatched => false,
                ToolCallState::Succeeded => false,
                ToolCallState::Failed => false,
                ToolCallState::Timeout => false,
            },
            ToolCallState::Running => match target {
                ToolCallState::Succeeded => true,
                ToolCallState::Failed => true,
                ToolCallState::Timeout => true,
                ToolCallState::Cancelled => true,
                ToolCallState::Queued => false,
                ToolCallState::Dispatched => false,
                ToolCallState::Running => false,
            },
            ToolCallState::Succeeded => match target {
                ToolCallState::Queued => false,
                ToolCallState::Dispatched => false,
                ToolCallState::Running => false,
                ToolCallState::Succeeded => false,
                ToolCallState::Failed => false,
                ToolCallState::Timeout => false,
                ToolCallState::Cancelled => false,
            },
            ToolCallState::Failed => match target {
                ToolCallState::Queued => false,
                ToolCallState::Dispatched => false,
                ToolCallState::Running => false,
                ToolCallState::Succeeded => false,
                ToolCallState::Failed => false,
                ToolCallState::Timeout => false,
                ToolCallState::Cancelled => false,
            },
            ToolCallState::Timeout => match target {
                ToolCallState::Queued => false,
                ToolCallState::Dispatched => false,
                ToolCallState::Running => false,
                ToolCallState::Succeeded => false,
                ToolCallState::Failed => false,
                ToolCallState::Timeout => false,
                ToolCallState::Cancelled => false,
            },
            ToolCallState::Cancelled => match target {
                ToolCallState::Queued => false,
                ToolCallState::Dispatched => false,
                ToolCallState::Running => false,
                ToolCallState::Succeeded => false,
                ToolCallState::Failed => false,
                ToolCallState::Timeout => false,
                ToolCallState::Cancelled => false,
            },
        }
    }

    pub fn is_terminal(&self) -> bool {
        match self {
            ToolCallState::Succeeded => true,
            ToolCallState::Failed => true,
            ToolCallState::Timeout => true,
            ToolCallState::Cancelled => true,
            ToolCallState::Queued => false,
            ToolCallState::Dispatched => false,
            ToolCallState::Running => false,
        }
    }
}

impl std::fmt::Display for ToolCallState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCallState::Queued => write!(f, "Queued"),
            ToolCallState::Dispatched => write!(f, "Dispatched"),
            ToolCallState::Running => write!(f, "Running"),
            ToolCallState::Succeeded => write!(f, "Succeeded"),
            ToolCallState::Failed => write!(f, "Failed"),
            ToolCallState::Timeout => write!(f, "Timeout"),
            ToolCallState::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl super::StateMachine for ToolCallState {
    fn can_transition_to(&self, target: Self) -> bool {
        ToolCallState::can_transition_to(self, target)
    }
    fn is_terminal(&self) -> bool {
        ToolCallState::is_terminal(self)
    }
}

// ---------------------------------------------------------------------------
// ToolCallRecord
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub call_id: CallId,
    pub task_id: TaskId,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub state: ToolCallState,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// ToolResultRecord — historical execution record.
// Distinct from ToolResult<T> in error.rs (type alias for Result<T, ToolError>).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub call_id: CallId,
    pub output: String,
    pub status: ToolCallState,
    pub error: Option<String>,
    pub duration_ms: u64,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [ToolCallState; 7] = [
        ToolCallState::Queued,
        ToolCallState::Dispatched,
        ToolCallState::Running,
        ToolCallState::Succeeded,
        ToolCallState::Failed,
        ToolCallState::Timeout,
        ToolCallState::Cancelled,
    ];

    const VALID_TRANSITIONS: &[(ToolCallState, ToolCallState)] = &[
        (ToolCallState::Queued, ToolCallState::Dispatched),
        (ToolCallState::Queued, ToolCallState::Cancelled),
        (ToolCallState::Dispatched, ToolCallState::Running),
        (ToolCallState::Dispatched, ToolCallState::Cancelled),
        (ToolCallState::Running, ToolCallState::Succeeded),
        (ToolCallState::Running, ToolCallState::Failed),
        (ToolCallState::Running, ToolCallState::Timeout),
        (ToolCallState::Running, ToolCallState::Cancelled),
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
    fn happy_path_queued_to_succeeded() {
        let mut state = ToolCallState::Queued;
        assert!(super::super::transition(&mut state, ToolCallState::Dispatched).is_ok());
        assert!(super::super::transition(&mut state, ToolCallState::Running).is_ok());
        assert!(super::super::transition(&mut state, ToolCallState::Succeeded).is_ok());
        assert_eq!(state, ToolCallState::Succeeded);
    }

    #[test]
    fn running_to_failed() {
        let mut state = ToolCallState::Running;
        assert!(super::super::transition(&mut state, ToolCallState::Failed).is_ok());
        assert_eq!(state, ToolCallState::Failed);
    }

    #[test]
    fn running_to_timeout() {
        let mut state = ToolCallState::Running;
        assert!(super::super::transition(&mut state, ToolCallState::Timeout).is_ok());
    }

    #[test]
    fn running_to_cancelled() {
        assert!(ToolCallState::Running.can_transition_to(ToolCallState::Cancelled));
    }

    #[test]
    fn queued_to_cancelled() {
        assert!(ToolCallState::Queued.can_transition_to(ToolCallState::Cancelled));
    }

    #[test]
    fn succeeded_rejects_all() {
        for t in &ALL_STATES {
            assert!(!ToolCallState::Succeeded.can_transition_to(*t));
        }
    }

    #[test]
    fn failed_rejects_all() {
        for t in &ALL_STATES {
            assert!(!ToolCallState::Failed.can_transition_to(*t));
        }
    }

    #[test]
    fn timeout_rejects_all() {
        for t in &ALL_STATES {
            assert!(!ToolCallState::Timeout.can_transition_to(*t));
        }
    }

    #[test]
    fn cancelled_rejects_all() {
        for t in &ALL_STATES {
            assert!(!ToolCallState::Cancelled.can_transition_to(*t));
        }
    }

    #[test]
    fn is_terminal_correct() {
        assert!(!ToolCallState::Queued.is_terminal());
        assert!(!ToolCallState::Dispatched.is_terminal());
        assert!(!ToolCallState::Running.is_terminal());
        assert!(ToolCallState::Succeeded.is_terminal());
        assert!(ToolCallState::Failed.is_terminal());
        assert!(ToolCallState::Timeout.is_terminal());
        assert!(ToolCallState::Cancelled.is_terminal());
    }

    #[test]
    fn transition_atomicity_state_preserved_on_error() {
        let mut state = ToolCallState::Succeeded;
        let result = super::super::transition(&mut state, ToolCallState::Running);
        assert!(result.is_err());
        assert_eq!(state, ToolCallState::Succeeded);
    }

    #[test]
    fn serde_roundtrip_all_variants() {
        for state in &ALL_STATES {
            let json = serde_json::to_string(state).unwrap();
            let back: ToolCallState = serde_json::from_str(&json).unwrap();
            assert_eq!(*state, back);
        }
    }

    #[test]
    fn display_all_variants() {
        let expected = [
            "Queued",
            "Dispatched",
            "Running",
            "Succeeded",
            "Failed",
            "Timeout",
            "Cancelled",
        ];
        for (s, name) in ALL_STATES.iter().zip(expected.iter()) {
            assert_eq!(format!("{}", s), *name);
        }
    }

    #[test]
    fn tool_call_record_serde_roundtrip() {
        let record = ToolCallRecord {
            call_id: CallId::new("c-1"),
            task_id: TaskId::new("t-1"),
            tool_name: "read_file".into(),
            input: serde_json::json!({"path": "/tmp/test"}),
            state: ToolCallState::Queued,
            created_at: 1000,
            started_at: None,
            completed_at: None,
        };
        let json = serde_json::to_string(&record).unwrap();
        let back: ToolCallRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.call_id, record.call_id);
        assert_eq!(back.state, record.state);
    }

    #[test]
    fn tool_result_record_serde_roundtrip() {
        let result = ToolResultRecord {
            call_id: CallId::new("c-1"),
            output: "file contents".into(),
            status: ToolCallState::Succeeded,
            error: None,
            duration_ms: 42,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ToolResultRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.call_id, result.call_id);
        assert_eq!(back.status, result.status);
        assert_eq!(back.duration_ms, 42);
    }
}
