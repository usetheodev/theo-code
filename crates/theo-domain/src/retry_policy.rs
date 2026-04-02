use serde::{Deserialize, Serialize};

/// Policy for retry behavior with exponential backoff and optional jitter.
///
/// Pure data type — no async, no IO. Execution is handled by RetryExecutor
/// in theo-agent-runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: bool,
}

impl RetryPolicy {
    /// Computes the delay for a given attempt using exponential backoff.
    ///
    /// Formula: min(base_delay_ms * 2^attempt, max_delay_ms)
    /// If jitter is enabled, the result is multiplied by a random factor in [0.0, 1.0].
    ///
    /// The delay never exceeds max_delay_ms.
    pub fn delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        let shift = if attempt >= 64 { u64::MAX } else { 1u64 << attempt };
        let exponential = self.base_delay_ms.saturating_mul(shift);
        let capped = exponential.min(self.max_delay_ms);

        let delay_ms = if self.jitter {
            let jitter_factor = simple_random_f64();
            ((capped as f64) * jitter_factor) as u64
        } else {
            capped
        };

        std::time::Duration::from_millis(delay_ms.min(self.max_delay_ms))
    }

    /// Default retry policy for LLM calls.
    /// 3 retries, 1000ms base, 30000ms max, jitter enabled.
    pub fn default_llm() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1000,
            max_delay_ms: 30_000,
            jitter: true,
        }
    }

    /// Default retry policy for tool execution.
    /// 2 retries, 200ms base, 5000ms max, jitter enabled.
    pub fn default_tool() -> Self {
        Self {
            max_retries: 2,
            base_delay_ms: 200,
            max_delay_ms: 5_000,
            jitter: true,
        }
    }
}

/// Strategy for correcting failed operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CorrectionStrategy {
    /// Retry the same operation locally.
    RetryLocal,
    /// Replan the entire approach.
    Replan,
    /// Break into a subtask.
    Subtask,
    /// Switch to a different agent type.
    AgentSwap,
}

impl std::fmt::Display for CorrectionStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CorrectionStrategy::RetryLocal => write!(f, "RetryLocal"),
            CorrectionStrategy::Replan => write!(f, "Replan"),
            CorrectionStrategy::Subtask => write!(f, "Subtask"),
            CorrectionStrategy::AgentSwap => write!(f, "AgentSwap"),
        }
    }
}

/// Simple random f64 in [0.0, 1.0) using system entropy.
fn simple_random_f64() -> f64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_nanos();

    let stack_var: u8 = 0;
    let addr = std::ptr::addr_of!(stack_var) as u64;

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    addr.hash(&mut hasher);
    let hash = hasher.finish();

    (hash as f64) / (u64::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_attempt_0_returns_base() {
        let policy = RetryPolicy {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 10_000,
            jitter: false,
        };
        assert_eq!(policy.delay_for_attempt(0).as_millis(), 100);
    }

    #[test]
    fn delay_attempt_3_returns_base_times_8() {
        let policy = RetryPolicy {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 10_000,
            jitter: false,
        };
        // 100 * 2^3 = 800
        assert_eq!(policy.delay_for_attempt(3).as_millis(), 800);
    }

    #[test]
    fn delay_never_exceeds_max() {
        let policy = RetryPolicy {
            max_retries: 10,
            base_delay_ms: 1000,
            max_delay_ms: 5000,
            jitter: false,
        };
        for attempt in 0..20 {
            let delay = policy.delay_for_attempt(attempt);
            assert!(
                delay.as_millis() <= 5000,
                "attempt {}: delay {} exceeds max 5000",
                attempt,
                delay.as_millis()
            );
        }
    }

    #[test]
    fn jitter_stays_within_bounds() {
        let policy = RetryPolicy {
            max_retries: 3,
            base_delay_ms: 1000,
            max_delay_ms: 30_000,
            jitter: true,
        };
        for _ in 0..1000 {
            let delay = policy.delay_for_attempt(1);
            // base * 2^1 = 2000, jitter in [0, 2000]
            assert!(delay.as_millis() <= 2000, "jitter exceeded bound: {}", delay.as_millis());
        }
    }

    #[test]
    fn default_llm_values() {
        let p = RetryPolicy::default_llm();
        assert_eq!(p.max_retries, 3);
        assert_eq!(p.base_delay_ms, 1000);
        assert_eq!(p.max_delay_ms, 30_000);
        assert!(p.jitter);
    }

    #[test]
    fn default_tool_values() {
        let p = RetryPolicy::default_tool();
        assert_eq!(p.max_retries, 2);
        assert_eq!(p.base_delay_ms, 200);
        assert_eq!(p.max_delay_ms, 5_000);
        assert!(p.jitter);
    }

    #[test]
    fn zero_retries_still_computes_delay() {
        let policy = RetryPolicy {
            max_retries: 0,
            base_delay_ms: 500,
            max_delay_ms: 1000,
            jitter: false,
        };
        assert_eq!(policy.delay_for_attempt(0).as_millis(), 500);
    }

    #[test]
    fn correction_strategy_serde_roundtrip() {
        let strategies = [
            CorrectionStrategy::RetryLocal,
            CorrectionStrategy::Replan,
            CorrectionStrategy::Subtask,
            CorrectionStrategy::AgentSwap,
        ];
        for s in &strategies {
            let json = serde_json::to_string(s).unwrap();
            let back: CorrectionStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, back);
        }
    }

    #[test]
    fn retry_policy_serde_roundtrip() {
        let p = RetryPolicy::default_llm();
        let json = serde_json::to_string(&p).unwrap();
        let back: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back.max_retries, p.max_retries);
        assert_eq!(back.base_delay_ms, p.base_delay_ms);
    }

    #[test]
    fn display_correction_strategy() {
        assert_eq!(format!("{}", CorrectionStrategy::RetryLocal), "RetryLocal");
        assert_eq!(format!("{}", CorrectionStrategy::Replan), "Replan");
        assert_eq!(format!("{}", CorrectionStrategy::Subtask), "Subtask");
        assert_eq!(format!("{}", CorrectionStrategy::AgentSwap), "AgentSwap");
    }
}
