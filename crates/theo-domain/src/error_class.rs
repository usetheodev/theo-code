//! Phase 58 (headless-error-classification-plan) — typed classification
//! of agent-run outcomes.
//!
//! `ErrorClass` is the canonical "why did this run end the way it did?"
//! domain type. It lives in `theo-domain` (zero deps) so the runtime,
//! CLI, and benchmark wrapper can all agree on the contract.
//!
//! Invariant enforced by consumers: `success=true ⇔ class == Solved`.
//! The invariant is NOT encoded in the type because `AgentResult.success`
//! is a separate field for backcompat with the v2 headless schema.

use serde::{Deserialize, Serialize};

/// Classification of an agent run outcome.
///
/// Serializes to snake_case so the JSON surface matches the headless v3
/// schema emitted by `theo --headless`:
///
/// ```json
/// {"schema": "theo.headless.v3", "error_class": "rate_limited", ...}
/// ```
///
/// Marked `#[non_exhaustive]` — downstream consumers MUST include a
/// catch-all `_ =>` arm so new variants can be added without breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorClass {
    /// Agent completed the task successfully. This is the ONLY variant
    /// that pairs with `AgentResult::success == true`.
    Solved,
    /// Agent ran out of iterations, tokens, or budget without producing
    /// a successful `done`. Not an infra failure — the agent genuinely
    /// struggled.
    Exhausted,
    /// Provider returned 429 (rate limit / throttling) and the retry
    /// loop failed to recover. Distinct from `QuotaExceeded`: rate-limit
    /// is a transient TPM/RPM ceiling that resets in seconds/minutes.
    /// Infra failure — the task outcome is unknown.
    RateLimited,
    /// Provider account hit its hard usage quota (monthly credit, paid
    /// tier limit, free trial cap). Distinct from `RateLimited`:
    /// retrying does NOT help — the quota only resets at the billing
    /// cycle boundary. Theo fails fast on this class. Infra failure.
    QuotaExceeded,
    /// Provider returned 401/403. Credentials missing or invalid.
    AuthFailed,
    /// Provider reported prompt exceeds the model's context window.
    /// Usually means the task history grew unbounded.
    ContextOverflow,
    /// A tool was denied by the sandbox cascade (bwrap/landlock/noop).
    /// Distinct from tool runtime errors — this is "operation not
    /// permitted in this environment."
    SandboxDenied,
    /// User or parent agent cooperatively cancelled the run (Ctrl+C,
    /// parent abort, CancellationTree::cancel_all). NOT a failure.
    Cancelled,
    /// Run terminated in an unrecoverable error that doesn't map to any
    /// other variant (internal invariant violation, parse error, etc.).
    /// Catch-all for "something went wrong inside theo."
    Aborted,
    /// The task description itself couldn't be parsed or validated
    /// (empty string, malformed JSON when structured, etc.).
    InvalidTask,
}

impl ErrorClass {
    /// Whether the class represents a non-success outcome.
    /// `Solved` is the only variant that returns false.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Solved)
    }

    /// Whether the class represents an infrastructure failure (as opposed
    /// to an agent-level failure or a legitimate outcome).
    ///
    /// Infra failures are excluded from paired statistical comparisons
    /// because they reflect provider/sandbox state, not agent behavior.
    pub fn is_infra(&self) -> bool {
        matches!(
            self,
            Self::RateLimited
                | Self::QuotaExceeded
                | Self::AuthFailed
                | Self::ContextOverflow
                | Self::SandboxDenied
        )
    }
}

impl std::fmt::Display for ErrorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Solved => "solved",
            Self::Exhausted => "exhausted",
            Self::RateLimited => "rate_limited",
            Self::QuotaExceeded => "quota_exceeded",
            Self::AuthFailed => "auth_failed",
            Self::ContextOverflow => "context_overflow",
            Self::SandboxDenied => "sandbox_denied",
            Self::Cancelled => "cancelled",
            Self::Aborted => "aborted",
            Self::InvalidTask => "invalid_task",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_class_solved_is_not_terminal() {
        assert!(!ErrorClass::Solved.is_terminal());
        assert!(ErrorClass::Exhausted.is_terminal());
        assert!(ErrorClass::RateLimited.is_terminal());
        assert!(ErrorClass::Aborted.is_terminal());
    }

    #[test]
    fn error_class_serializes_as_snake_case() {
        let variants = [
            (ErrorClass::Solved, "\"solved\""),
            (ErrorClass::RateLimited, "\"rate_limited\""),
            (ErrorClass::QuotaExceeded, "\"quota_exceeded\""),
            (ErrorClass::AuthFailed, "\"auth_failed\""),
            (ErrorClass::ContextOverflow, "\"context_overflow\""),
            (ErrorClass::SandboxDenied, "\"sandbox_denied\""),
            (ErrorClass::InvalidTask, "\"invalid_task\""),
            (ErrorClass::Cancelled, "\"cancelled\""),
            (ErrorClass::Aborted, "\"aborted\""),
            (ErrorClass::Exhausted, "\"exhausted\""),
        ];
        for (variant, expected) in variants {
            let got = serde_json::to_string(&variant).unwrap();
            assert_eq!(got, expected, "variant {:?}", variant);
        }
    }

    #[test]
    fn error_class_rate_limited_is_infra() {
        assert!(ErrorClass::RateLimited.is_infra());
        assert!(ErrorClass::QuotaExceeded.is_infra());
        assert!(ErrorClass::AuthFailed.is_infra());
        assert!(ErrorClass::ContextOverflow.is_infra());
        assert!(ErrorClass::SandboxDenied.is_infra());
    }

    #[test]
    fn error_class_quota_exceeded_is_distinct_from_rate_limited() {
        // Two HTTP-429 cases that need OPPOSITE strategies:
        //   - RateLimited → retry with backoff (window resets in seconds)
        //   - QuotaExceeded → fail fast (resets at billing cycle)
        // Conflating them would burn 8min waiting for nothing on quota
        // exhaustion. Type-level distinction enforces the difference.
        assert_ne!(ErrorClass::RateLimited, ErrorClass::QuotaExceeded);
        // Both still classified as infra so ab_compare excludes both
        // from paired statistical analysis.
        assert!(ErrorClass::QuotaExceeded.is_infra());
        assert!(ErrorClass::RateLimited.is_infra());
    }

    #[test]
    fn error_class_aborted_is_terminal_but_not_infra() {
        // Aborted = internal invariant violation; not an infra problem,
        // but definitely not a success. Must stay OUT of is_infra so that
        // ab_compare doesn't silently drop genuine theo bugs.
        assert!(ErrorClass::Aborted.is_terminal());
        assert!(!ErrorClass::Aborted.is_infra());
        assert!(ErrorClass::Exhausted.is_terminal());
        assert!(!ErrorClass::Exhausted.is_infra());
        assert!(ErrorClass::Cancelled.is_terminal());
        assert!(!ErrorClass::Cancelled.is_infra());
    }

    #[test]
    fn error_class_round_trips_through_serde() {
        for variant in [
            ErrorClass::Solved,
            ErrorClass::Exhausted,
            ErrorClass::RateLimited,
            ErrorClass::AuthFailed,
            ErrorClass::ContextOverflow,
            ErrorClass::SandboxDenied,
            ErrorClass::Cancelled,
            ErrorClass::Aborted,
            ErrorClass::InvalidTask,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: ErrorClass = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back, "roundtrip {:?}", variant);
        }
    }

    #[test]
    fn error_class_display_matches_serde() {
        // Display impl MUST agree with serde snake_case rename.
        // If they drift, the headless JSON emission would use one string
        // while CLI logs use another — hellish to debug.
        for variant in [
            ErrorClass::Solved,
            ErrorClass::RateLimited,
            ErrorClass::ContextOverflow,
        ] {
            let display_str = format!("{}", variant);
            let serde_str = serde_json::to_string(&variant)
                .unwrap()
                .trim_matches('"')
                .to_string();
            assert_eq!(display_str, serde_str, "variant {:?}", variant);
        }
    }
}
