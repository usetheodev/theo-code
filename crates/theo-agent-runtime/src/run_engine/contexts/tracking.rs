//! `TrackingContext` — bundle of state-tracking + dispatch handles.
//!
//! T3.1 PR3 of the AgentRunEngine god-object split. Per
//! `docs/plans/T3.1-god-object-split-roadmap.md`.
//!
//! Note: `task_id` and `task_manager` are kept as separate accessors
//! on `AgentRunEngine` (used everywhere). `done_attempts` and
//! `plan_mode_nudged` are mutable counters specific to the run loop.
//! This bundle groups the ones that travel together for state
//! tracking.

use std::sync::atomic::AtomicBool;

use crate::failure_tracker::FailurePatternTracker;

/// Per-run state-tracking counters + flags.
pub struct TrackingContext {
    pub done_attempts: u32,
    /// One-shot guard: in Plan mode, if the model converges with text only and
    /// no plan file on disk, we inject a corrective reminder once. After that
    /// we let it converge normally to avoid infinite reminder loops.
    pub plan_mode_nudged: bool,
    pub failure_tracker: FailurePatternTracker,
    /// Whether a checkpoint snapshot has already been taken this turn.
    /// Reset at the start of every turn; set to `true` on first
    /// mutating-tool dispatch. One snapshot per turn max.
    pub checkpoint_taken_this_turn: AtomicBool,
}

impl TrackingContext {
    pub fn new(failure_tracker: FailurePatternTracker) -> Self {
        Self {
            done_attempts: 0,
            plan_mode_nudged: false,
            failure_tracker,
            checkpoint_taken_this_turn: AtomicBool::new(false),
        }
    }
}
