use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("parse error: {0}")]
    Parse(String),

    #[error("stream ended unexpectedly")]
    StreamEnded,

    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("rate limited (retry after {retry_after:?}s)")]
    RateLimited { retry_after: Option<u64> },

    #[error("unknown provider: {0}")]
    ProviderNotFound(String),

    #[error("request timeout")]
    Timeout,

    #[error("service unavailable")]
    ServiceUnavailable,

    #[error("context overflow from {provider}: {message}")]
    ContextOverflow { provider: String, message: String },
}

impl LlmError {
    /// Whether this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LlmError::Network(_)
                | LlmError::RateLimited { .. }
                | LlmError::ServiceUnavailable
                | LlmError::Timeout
        )
    }

    /// Extract retry-after seconds from a rate limit error.
    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            LlmError::RateLimited { retry_after } => *retry_after,
            _ => None,
        }
    }

    /// Whether this error is a context overflow (prompt too long for model).
    pub fn is_context_overflow(&self) -> bool {
        matches!(self, LlmError::ContextOverflow { .. })
    }

    /// Create from HTTP status code.
    pub fn from_status(status: u16, message: String) -> Self {
        // Check for context overflow before generic classification
        if crate::overflow::is_context_overflow(&message) {
            return LlmError::ContextOverflow {
                provider: String::new(),
                message,
            };
        }
        match status {
            401 | 403 => LlmError::AuthFailed(message),
            429 => LlmError::RateLimited { retry_after: None },
            503 => LlmError::ServiceUnavailable,
            504 => LlmError::Timeout,
            _ => LlmError::Api { status, message },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_errors() {
        assert!(
            LlmError::RateLimited {
                retry_after: Some(5)
            }
            .is_retryable()
        );
        assert!(LlmError::ServiceUnavailable.is_retryable());
        assert!(LlmError::Timeout.is_retryable());
    }

    #[test]
    fn non_retryable_errors() {
        assert!(!LlmError::AuthFailed("bad key".to_string()).is_retryable());
        assert!(!LlmError::Parse("bad json".to_string()).is_retryable());
        assert!(!LlmError::ProviderNotFound("x".to_string()).is_retryable());
    }

    #[test]
    fn from_status_maps_correctly() {
        assert!(matches!(
            LlmError::from_status(401, "".into()),
            LlmError::AuthFailed(_)
        ));
        assert!(matches!(
            LlmError::from_status(429, "".into()),
            LlmError::RateLimited { .. }
        ));
        assert!(matches!(
            LlmError::from_status(503, "".into()),
            LlmError::ServiceUnavailable
        ));
        assert!(matches!(
            LlmError::from_status(504, "".into()),
            LlmError::Timeout
        ));
        assert!(matches!(
            LlmError::from_status(500, "err".into()),
            LlmError::Api { .. }
        ));
    }

    #[test]
    fn retry_after_secs() {
        let e = LlmError::RateLimited {
            retry_after: Some(30),
        };
        assert_eq!(e.retry_after_secs(), Some(30));
        assert_eq!(LlmError::Timeout.retry_after_secs(), None);
    }

    #[test]
    fn context_overflow_is_not_retryable() {
        let e = LlmError::ContextOverflow {
            provider: "openai".into(),
            message: "prompt is too long".into(),
        };
        assert!(!e.is_retryable());
        assert!(e.is_context_overflow());
    }

    #[test]
    fn from_status_detects_context_overflow() {
        let e = LlmError::from_status(
            400,
            "This model's maximum context length is 128000 tokens. \
             Please reduce the length of the messages."
                .into(),
        );
        assert!(e.is_context_overflow());
    }

    #[test]
    fn from_status_does_not_false_positive_overflow() {
        let e = LlmError::from_status(500, "internal server error".into());
        assert!(!e.is_context_overflow());
    }
}
