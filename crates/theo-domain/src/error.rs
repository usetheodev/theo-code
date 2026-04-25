use thiserror::Error;

#[derive(Error, Debug)]
pub enum OpenCodeError {
    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    Execution(String),

    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    InvalidArgs(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, OpenCodeError>;
pub type ToolResult<T> = std::result::Result<T, ToolError>;

#[derive(Error, Debug, Clone)]
pub enum TransitionError {
    #[error("invalid transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },
}

impl TransitionError {
    /// Returns `true` when this error represents a "no-op" transition
    /// (the state machine was asked to move to the state it is already
    /// in). Such transitions are semantically idempotent and callers
    /// frequently want to ignore them while still propagating *real*
    /// transition rejections via observability.
    ///
    /// Added in T1.4 of the agent-runtime remediation plan to replace
    /// the `let _ = task_manager.transition(...)` pattern that was
    /// silently discarding *both* same-state and genuinely-invalid
    /// transitions (find_p4_005, INV-002).
    pub fn is_already_in_state(&self) -> bool {
        match self {
            TransitionError::InvalidTransition { from, to } => from == to,
        }
    }
}

#[cfg(test)]
mod tests_transition_error {
    use super::*;

    #[test]
    fn is_already_in_state_true_when_from_equals_to() {
        let err = TransitionError::InvalidTransition {
            from: "Running".into(),
            to: "Running".into(),
        };
        assert!(err.is_already_in_state());
    }

    #[test]
    fn is_already_in_state_false_for_genuine_invalid() {
        let err = TransitionError::InvalidTransition {
            from: "Pending".into(),
            to: "Completed".into(),
        };
        assert!(!err.is_already_in_state());
    }

    #[test]
    fn is_already_in_state_distinguishes_case_sensitive() {
        // Defensive: format!("{:?}", state) is the source-of-truth string,
        // and Debug output is case-sensitive — make the contract explicit.
        let err = TransitionError::InvalidTransition {
            from: "running".into(),
            to: "Running".into(),
        };
        assert!(!err.is_already_in_state());
    }
}
