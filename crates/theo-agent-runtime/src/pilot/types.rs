//! Pilot domain types — exit reasons, results, circuit-breaker state.
//!
//! Split out of `pilot/mod.rs` (REMEDIATION_PLAN T4.* — production-LOC
//! trim toward the per-file 500-line target). Re-exported from `mod.rs`
//! to keep public paths byte-identical.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone)]
pub enum ExitReason {
    PromiseFulfilled,
    FixPlanComplete,
    RateLimitExhausted,
    CircuitBreakerOpen(String),
    MaxCallsReached,
    UserInterrupt,
    Error(String),
}

impl std::fmt::Display for ExitReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExitReason::PromiseFulfilled => write!(f, "Promise fulfilled"),
            ExitReason::FixPlanComplete => write!(f, "Fix plan complete"),
            ExitReason::RateLimitExhausted => write!(f, "Rate limit exhausted"),
            ExitReason::CircuitBreakerOpen(reason) => write!(f, "Circuit breaker: {reason}"),
            ExitReason::MaxCallsReached => write!(f, "Max calls reached"),
            ExitReason::UserInterrupt => write!(f, "User interrupt"),
            ExitReason::Error(e) => write!(f, "Error: {e}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PilotResult {
    pub success: bool,
    pub reason: ExitReason,
    pub loops_completed: usize,
    pub total_tokens: u64,
    pub files_edited: Vec<String>,
    pub promise: String,
}
