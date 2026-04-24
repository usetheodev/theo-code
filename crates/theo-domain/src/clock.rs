//! Unified clock helpers.
//!
//! Single source of truth for `now_millis()` across the workspace. Prior to
//! this module, three crates (`theo-agent-runtime::task_manager`,
//! `theo-agent-runtime::tool_call_manager`, `theo-agent-runtime::run_engine`)
//! each carried a private copy that `panic!`ed with
//! `expect("system clock before UNIX epoch")` on clock skew. That violates
//! the no-panic-in-production rule from `rust-conventions.md`.
//!
//! This implementation returns `0` on skew instead of panicking — the
//! downstream consumers use the value for ordering, not correctness.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current wall-clock time as unix millis.
///
/// Returns `0` if the system clock is before UNIX_EPOCH (clock skew in
/// containers, misconfigured VMs). Never panics.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_millis_is_nonzero_in_practice() {
        let a = now_millis();
        assert!(a > 0, "real clock should be past UNIX_EPOCH");
    }

    #[test]
    fn now_millis_is_monotonic_nondecreasing() {
        let a = now_millis();
        let b = now_millis();
        assert!(b >= a);
    }
}
