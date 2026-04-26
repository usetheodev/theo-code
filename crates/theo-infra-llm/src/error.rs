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

    /// Phase 61 (headless-error-classification-plan): provider rejected
    /// the request because the account has exhausted its hard usage
    /// quota (monthly credits, paid tier limit, free trial cap).
    /// Distinct from `RateLimited` because retrying does NOT help —
    /// the quota only resets at the billing cycle boundary. Theo fails
    /// fast on this variant (`is_retryable() == false`).
    #[error("quota exceeded ({provider}): {message}")]
    QuotaExceeded { provider: String, message: String },

    #[error("unknown provider: {0}")]
    ProviderNotFound(String),

    #[error("request timeout")]
    Timeout,

    #[error("service unavailable")]
    ServiceUnavailable,

    #[error("context overflow from {provider}: {message}")]
    ContextOverflow { provider: String, message: String },

    /// Emitted when the routing fallback cascade has exhausted its hop
    /// budget without a successful call. Carries the list of model ids
    /// that were tried, in order, so the caller can diagnose which tier
    /// ultimately failed. Plan ref: outputs/smart-model-routing-plan.md §R5.
    #[error("routing fallback exhausted after trying {attempted:?}")]
    FallbackExhausted { attempted: Vec<String> },
}

impl LlmError {
    /// Whether this error is retryable.
    ///
    /// Note: `QuotaExceeded` is INTENTIONALLY non-retryable. Hitting a
    /// hard quota means the next retry will hit the same wall — only the
    /// billing cycle boundary clears it. Failing fast saves the trial
    /// budget and surfaces the issue immediately.
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

    /// Map an LLM error to a routing failure hint so the router can
    /// decide whether to retry, fall back, or surface the error.
    /// Returns `None` for errors that should bubble up without retry
    /// (auth, parse, generic 4xx).
    pub fn to_routing_hint(&self) -> Option<theo_domain::routing::RoutingFailureHint> {
        use theo_domain::routing::RoutingFailureHint;
        match self {
            LlmError::ContextOverflow { .. } => Some(RoutingFailureHint::ContextOverflow),
            LlmError::RateLimited { .. } => Some(RoutingFailureHint::RateLimit),
            // Phase 61: quota exhaustion is permanent within the billing
            // cycle — surface it as RateLimit hint so the router treats it
            // similar to throttling at the cascade level (it WILL fall
            // through to the next provider if one is configured).
            LlmError::QuotaExceeded { .. } => Some(RoutingFailureHint::RateLimit),
            LlmError::Timeout | LlmError::ServiceUnavailable | LlmError::Network(_) => {
                Some(RoutingFailureHint::Transient)
            }
            LlmError::AuthFailed(_)
            | LlmError::Api { .. }
            | LlmError::Parse(_)
            | LlmError::StreamEnded
            | LlmError::ProviderNotFound(_)
            | LlmError::FallbackExhausted { .. } => None,
        }
    }

    /// Create from HTTP status code.
    ///
    /// Phase 61 (headless-error-classification-plan): when status=429,
    /// we inspect the response body for quota-exhaustion keywords. If
    /// the body looks like a quota error (insufficient_quota, billing
    /// limit, usage exceeded, etc.), we map to `QuotaExceeded` (NOT
    /// retryable) instead of `RateLimited` (retryable). This avoids
    /// burning 8 minutes of retries on a problem that won't clear
    /// until the billing cycle resets.
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
            429 => {
                if is_quota_exhaustion(&message) {
                    LlmError::QuotaExceeded {
                        provider: String::new(),
                        message,
                    }
                } else {
                    LlmError::RateLimited { retry_after: None }
                }
            }
            503 => LlmError::ServiceUnavailable,
            504 => LlmError::Timeout,
            _ => LlmError::Api { status, message },
        }
    }
}

/// Phase 61 — heuristic quota-vs-rate-limit detection for HTTP 429
/// response bodies. Returns true if the body indicates the account has
/// hit its hard usage quota (vs. transient throttling).
///
/// Patterns drawn from observed provider responses:
///   - OpenAI: `{"error":{"code":"insufficient_quota", ...}}`
///   - Codex (OAuth-backed ChatGPT): same as OpenAI
///   - Anthropic: `{"error":{"type":"billing_error", ...}}`
///   - Generic: any body containing "quota", "billing", "usage limit",
///     "credit", or "exceeded" (case-insensitive).
///
/// False positives are preferred over false negatives — misclassifying
/// a true rate-limit as quota costs us a single retry attempt; the
/// reverse costs ~8 minutes per trial.
fn is_quota_exhaustion(body: &str) -> bool {
    let lower = body.to_lowercase();
    const QUOTA_KEYWORDS: &[&str] = &[
        "insufficient_quota",
        "insufficient quota",
        "quota exceeded",
        "quota_exceeded",
        "billing",
        "usage limit",
        "credit balance",
        "exceeded your current quota",
        "out of credits",
        "limit reached",
    ];
    QUOTA_KEYWORDS.iter().any(|k| lower.contains(k))
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

    // -----------------------------------------------------------------------
    // Phase 61 (headless-error-classification-plan) — quota detection
    // -----------------------------------------------------------------------

    #[test]
    fn from_status_429_with_insufficient_quota_returns_quota_exceeded() {
        // OpenAI/Codex canonical body
        let body = r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota"}}"#;
        let e = LlmError::from_status(429, body.into());
        assert!(matches!(e, LlmError::QuotaExceeded { .. }));
    }

    #[test]
    fn from_status_429_with_billing_keyword_returns_quota_exceeded() {
        // Anthropic-style billing error
        let body = r#"{"error":{"type":"billing_error","message":"Account credit balance is too low"}}"#;
        let e = LlmError::from_status(429, body.into());
        assert!(matches!(e, LlmError::QuotaExceeded { .. }));
    }

    #[test]
    fn from_status_429_without_quota_keywords_returns_rate_limited() {
        // Plain throttling — generic 429 with no quota indicators
        let body = "Too many requests";
        let e = LlmError::from_status(429, body.into());
        assert!(
            matches!(e, LlmError::RateLimited { .. }),
            "plain 429 must map to RateLimited (retryable), not QuotaExceeded"
        );
    }

    #[test]
    fn quota_exceeded_is_not_retryable() {
        // The whole point of distinguishing quota from rate-limit:
        // retrying on quota burns trial budget for nothing.
        let e = LlmError::QuotaExceeded {
            provider: "openai".into(),
            message: "insufficient_quota".into(),
        };
        assert!(
            !e.is_retryable(),
            "quota exhaustion does not clear via retry — must be non-retryable"
        );
    }

    #[test]
    fn rate_limited_remains_retryable() {
        // Sanity check — rate-limit (transient throttle) MUST stay retryable
        // so the agent can recover within the TPM/RPM window.
        let e = LlmError::RateLimited { retry_after: Some(5) };
        assert!(e.is_retryable());
        let e = LlmError::RateLimited { retry_after: None };
        assert!(e.is_retryable());
    }

    #[test]
    fn quota_exceeded_routing_hint_is_rate_limit() {
        // Router should treat quota the same as rate-limit at the
        // cascade level — try next provider if configured.
        let e = LlmError::QuotaExceeded {
            provider: "openai".into(),
            message: "insufficient_quota".into(),
        };
        assert!(e.to_routing_hint().is_some());
    }

    #[test]
    fn is_quota_exhaustion_case_insensitive() {
        // Provider responses vary in casing — match must be tolerant
        assert!(super::is_quota_exhaustion("INSUFFICIENT_QUOTA"));
        assert!(super::is_quota_exhaustion("Insufficient Quota"));
        assert!(super::is_quota_exhaustion("you have exceeded your current quota"));
        assert!(super::is_quota_exhaustion("Account billing issue"));
        assert!(!super::is_quota_exhaustion("temporary throttle"));
        assert!(!super::is_quota_exhaustion("rate limit hit"));
    }
}
