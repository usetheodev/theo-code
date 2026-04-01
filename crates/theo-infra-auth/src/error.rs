use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("token expired")]
    TokenExpired,

    #[error("callback timeout: no response within {0} seconds")]
    CallbackTimeout(u64),

    #[error("CSRF state mismatch")]
    StateMismatch,

    #[error("storage error: {0}")]
    Storage(String),

    #[error("browser open failed: {0}")]
    BrowserOpen(String),

    #[error("device flow pending")]
    DevicePending,

    #[error("device flow expired")]
    DeviceExpired,
}
