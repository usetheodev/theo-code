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
