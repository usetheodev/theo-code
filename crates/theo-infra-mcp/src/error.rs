use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("server error (code {code}): {message}")]
    ServerError { code: i32, message: String },
    #[error("server returned no result and no error")]
    EmptyResponse,
    #[error("transport closed unexpectedly")]
    TransportClosed,
    #[error("operation timed out after {0:?}")]
    Timeout(std::time::Duration),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}
