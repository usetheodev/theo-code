//! Shared numeric constants for the agent runtime.
//!
//! Centralizes magic numbers previously inlined across `run_engine.rs`,
//! `tool_call_manager.rs`, and `compaction*.rs`. Each constant carries a
//! short justification so reviewers can tell ergonomics from policy.
//!
//! When values become configurable, migrate them to `AgentConfig` (or a
//! dedicated sub-config). This module is the "policy is a constant" layer,
//! not a config replacement.

use std::time::Duration;

// ---------------------------------------------------------------------------
// Done gate
// ---------------------------------------------------------------------------

/// Hard cap on consecutive `done` attempts before the gate accepts with a
/// warning. Prevents the LLM from burning the entire budget trying to end.
pub const MAX_DONE_ATTEMPTS: u32 = 3;

/// Timeout for the `cargo test` / workspace test command invoked by the
/// done gate. Workspace tests on a cold build may exceed this — the fallback
/// `cargo check` handles that.
pub const DONE_GATE_TEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Fallback timeout when `cargo test` times out. `cargo check` is much
/// cheaper and acts as a "did the tree at least compile?" sensor.
pub const DONE_GATE_CHECK_FALLBACK_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Batch tool
// ---------------------------------------------------------------------------

/// Maximum number of sub-calls the `batch` meta-tool accepts per invocation.
/// Prevents fan-outs that would swamp the tool registry or the LLM's ability
/// to reason about N parallel results.
pub const MAX_BATCH_SIZE: usize = 25;

// ---------------------------------------------------------------------------
// Event payload previews
// ---------------------------------------------------------------------------

/// Byte cap on preview strings attached to `ToolCallCompleted` events
/// (output, batch results). Keeps event payloads small enough to log
/// cheaply while preserving enough signal for debugging.
pub const TOOL_PREVIEW_BYTES: usize = 200;

/// Byte cap on each string field inside `ToolCallCompleted.input`. Longer
/// strings are truncated with an ellipsis at the nearest char boundary.
pub const TOOL_INPUT_TRUNCATE_BYTES: usize = 500;

/// Byte cap on the error/stderr preview emitted when the done gate's
/// `cargo test` fails. Long compiler output gets an ellipsis suffix.
pub const DONE_GATE_ERROR_PREVIEW_BYTES: usize = 2000;

/// Byte cap on sensor output injected into the next LLM turn's context.
pub const SENSOR_OUTPUT_PREVIEW_BYTES: usize = 1000;

// ---------------------------------------------------------------------------
// Compaction / recovery
// ---------------------------------------------------------------------------

/// Fraction of the model context window retained after emergency compaction
/// on context-overflow errors. 0.5 = drop half; the remaining headroom is
/// for the retry completion.
pub const EMERGENCY_COMPACT_RATIO: f64 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_within_sanity_bounds() {
        assert!(MAX_DONE_ATTEMPTS >= 1 && MAX_DONE_ATTEMPTS <= 10);
        assert!(MAX_BATCH_SIZE >= 1 && MAX_BATCH_SIZE <= 100);
        assert!(TOOL_PREVIEW_BYTES > 0 && TOOL_PREVIEW_BYTES < TOOL_INPUT_TRUNCATE_BYTES);
        assert!(EMERGENCY_COMPACT_RATIO > 0.0 && EMERGENCY_COMPACT_RATIO < 1.0);
        assert!(DONE_GATE_TEST_TIMEOUT > DONE_GATE_CHECK_FALLBACK_TIMEOUT);
    }
}
