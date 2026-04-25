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

// ---------------------------------------------------------------------------
// Done-gate sandbox limits (T1.1)
// ---------------------------------------------------------------------------

/// Max CPU time (seconds) the done-gate `cargo test`/`cargo check` command
/// may consume. Hard kill via RLIMIT_CPU if exceeded. Separate from the
/// async wall-clock timeout — provides a kernel-enforced ceiling even if
/// the tokio timer is starved.
pub const DONE_GATE_CPU_SECONDS: u64 = 180;

/// Max virtual address space (bytes) for the done-gate subprocess.
/// 2 GiB — ample for `cargo check` / small `cargo test`, bounded enough to
/// stop `build.rs` runaway allocations.
pub const DONE_GATE_MEM_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Max file size (bytes) the done-gate subprocess may write. Prevents a
/// malicious `build.rs` from filling `/tmp` or `target/`.
pub const DONE_GATE_FSIZE_BYTES: u64 = 512 * 1024 * 1024;

/// Max concurrent child processes the done-gate subprocess may spawn.
/// Enough for parallel rustc/jobserver workers without allowing fork-bombs.
pub const DONE_GATE_NPROC: u32 = 128;

#[cfg(test)]
mod tests {
    use super::*;

    /// These assertions are deliberate guards: each one fires only if a
    /// future PR raises/lowers a sanity-bound constant past the
    /// documented invariant. Clippy flags them as "constant" because at
    /// the current commit every operand is a compile-time literal — but
    /// that's the point of the test (compile-time invariant pinning).
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn constants_are_within_sanity_bounds() {
        assert!(MAX_DONE_ATTEMPTS >= 1 && MAX_DONE_ATTEMPTS <= 10);
        assert!(MAX_BATCH_SIZE >= 1 && MAX_BATCH_SIZE <= 100);
        assert!(TOOL_PREVIEW_BYTES > 0 && TOOL_PREVIEW_BYTES < TOOL_INPUT_TRUNCATE_BYTES);
        assert!(EMERGENCY_COMPACT_RATIO > 0.0 && EMERGENCY_COMPACT_RATIO < 1.0);
        assert!(DONE_GATE_TEST_TIMEOUT > DONE_GATE_CHECK_FALLBACK_TIMEOUT);
    }
}
